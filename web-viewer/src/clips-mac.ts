// Runs with bun. Clip points (切り抜き) computed on the Mac, the default: generate() with clef and
// LFM3-1.7B served by `python -m replay.clip_models` on 127.0.0.1, stored through the viewer's
// bearer PUT. No Cloudflare model runs; Laya is served only for the surrogate judge.
import { createHash } from "node:crypto";
import { z } from "zod";
import {
  type Ai,
  type ClipRecord,
  clipRecordSchema,
  GATE_MODEL,
  generate,
  inputHash,
  LAYA_MODEL,
  READ_MODEL,
  RECORD_SCHEMA,
  REGION_MODEL,
  SCREEN_MODEL,
  SUMMARY_MODEL,
  type VideoData,
} from "./clips.ts";

export interface Viewer {
  /** The record viewers see now, or null when there is none for the published movie. */
  current(videoId: string): Promise<ClipRecord | null>;
  store(videoId: string, record: ClipRecord): Promise<void>;
}
export interface MacRun {
  videos: readonly VideoData[];
  /** A user's recomputation: compute even when the stored record matches the data. */
  force: boolean;
  ai: Ai;
  viewer: Viewer;
  now: Date;
}
/** Answers kept between runs, by request key. */
export interface AnswerStore {
  read(key: string): Promise<string | null>;
  write(key: string, value: string): Promise<void>;
}
export interface MacResult {
  videoId: string;
  outcome: "written" | "current";
  clips: number;
}
type Fetcher = (
  url: string,
  init: { method: string; headers: Record<string, string>; body?: string; timeout?: boolean },
) => Promise<{
  ok: boolean;
  status: number;
  json(): Promise<unknown>;
}>;

/** The strict local judge of `clips:eval`, asked under this id. */
export const BONSAI_MODEL: string = "prism-ml/Ternary-Bonsai-2-9B-mlx-2bit";
/** The local server's route for each model the stages ask for; any other model is not on the Mac. */
const ROUTES: Record<string, string> = {
  [REGION_MODEL]: "clef",
  [READ_MODEL]: "clef",
  [GATE_MODEL]: "clef",
  [SCREEN_MODEL]: "clef-flash",
  [LAYA_MODEL]: "laya",
  [SUMMARY_MODEL]: "lfm3",
  [BONSAI_MODEL]: "bonsai",
};
/**
 * The deciders (clef, clef-flash): they answer an identical request identically, so their answers
 * are kept between runs. The editor (LFM3) is always asked again.
 */
const DECIDERS: Set<string> = new Set(
  Object.entries(ROUTES).flatMap(([model, route]) => (route.startsWith("clef") ? [model] : [])),
);
/** Replaced by the published movie's key when the Worker stores the record. */
const MAC_MOVIE_KEY: string = "mac";
/**
 * PUTs of one record: R2 now and then fails a write with a transient internal error (10001), which
 * the Worker answers with 500 (10-29: 10/26's record, computed for 45 minutes, was lost to one).
 * The record is the same each time, so sending it again is safe.
 */
const STORE_ATTEMPTS: number = 3;
const STORE_RETRY_MS: number = 30_000;
const SERVER_ERROR: number = 500;
const lookupSchema: z.ZodType<{ status: string; record?: unknown; schema?: number | undefined }> =
  z.object({
    status: z.string(),
    record: z.unknown().optional(),
    schema: z.number().optional(),
  });
const refusalSchema = z.object({ error: z.string() });

/** Answers every stage from the local server; a model it does not serve fails instead of reaching Cloudflare. */
export function localAi(fetcher: Fetcher, base: string): Ai {
  return {
    run: async (model, input) => {
      const route = ROUTES[model];
      if (route === undefined) throw new Error(`${model} is not served on the Mac`);
      const response = await fetcher(`${base}/${route}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(input),
        // A long region request on a busy Mac outlasts bun's default 5-minute fetch timeout.
        timeout: false,
      });
      if (!response.ok) throw new Error(`local ${route} answered ${response.status}`);
      return response.json();
    },
  };
}

/**
 * The deciders' answers kept in `store` across runs, keyed by `identity` (the server code and model
 * files), the model and the request: a run that failed late (10-28: three PUTs refused after 45
 * minutes each) or a re-run after a gate change asks the Mac again only what it has not answered,
 * and gets the same answers, so the same clips.
 */
export function persisted(ai: Ai, store: AnswerStore, identity: string): Ai & { hits(): number } {
  let hits = 0;
  return {
    hits: () => hits,
    run: async (model, input) => {
      if (!DECIDERS.has(model)) return ai.run(model, input);
      const key = createHash("sha256")
        .update(`${identity}\n${model}\n${JSON.stringify(input)}`)
        .digest("hex");
      const known = await store.read(key);
      if (known !== null) {
        hits += 1;
        return JSON.parse(known);
      }
      const answer = await ai.run(model, input);
      await store.write(key, JSON.stringify(answer));
      return answer;
    },
  };
}

/** The viewer's clips API with the CLIPS_API_TOKEN bearer. */
export function viewerOf(
  fetcher: Fetcher,
  origin: string,
  token: string,
  pause: (ms: number) => Promise<void>,
): Viewer {
  const headers = { Authorization: `Bearer ${token}`, "User-Agent": "clips-mac/1" };
  return {
    current: async (videoId) => {
      const response = await fetcher(`${origin}/api/clips/${videoId}.json`, {
        method: "GET",
        headers,
      });
      const found = lookupSchema.safeParse(await response.json());
      // Asked before computing: a Worker behind this record shape would refuse it hours later.
      const deployed = found.success ? (found.data.schema ?? 1) : 1;
      if (deployed !== RECORD_SCHEMA) {
        throw new Error(
          `Worker clip schema v${deployed} deployed, v${RECORD_SCHEMA} required: deploy web-viewer`,
        );
      }
      const record = found.success && found.data.status === "ready" ? found.data.record : null;
      const parsed = clipRecordSchema.safeParse(record);
      return parsed.success ? parsed.data : null;
    },
    store: async (videoId, record) => {
      const attempt = async (left: number): Promise<void> => {
        const response = await fetcher(`${origin}/api/clips/${videoId}.json`, {
          method: "PUT",
          headers: { ...headers, "Content-Type": "application/json" },
          body: JSON.stringify(record),
        });
        if (response.ok) return;
        if (response.status >= SERVER_ERROR && left > 1) {
          await pause(STORE_RETRY_MS);
          return attempt(left - 1);
        }
        const refused = refusalSchema.safeParse(await response.json().catch(() => null));
        const reason = refused.success ? `: ${refused.data.error}` : "";
        throw new Error(`storing clips for ${videoId} answered ${response.status}${reason}`);
      };
      return attempt(STORE_ATTEMPTS);
    },
  };
}

/**
 * Whether a record was made from this data, on either place: then the Mac leaves it, so records
 * Cloudflare made are not redone just for the move.
 */
export async function matches(record: ClipRecord, data: VideoData): Promise<boolean> {
  const hashes = [await inputHash(data, "mac"), await inputHash(data, "cloudflare")];
  return hashes.includes(record.inputHash);
}

/** Computes and stores each video without a matching record, in turn; unchanged summaries are reused. */
export async function runMac(run: MacRun): Promise<MacResult[]> {
  return run.videos.reduce<Promise<MacResult[]>>(async (done, data) => {
    const results = await done;
    const previous = await run.viewer.current(data.id);
    if (!run.force && previous !== null && (await matches(previous, data))) {
      results.push({ videoId: data.id, outcome: "current", clips: previous.clips.length });
      return results;
    }
    const record = await generate({
      ai: run.ai,
      place: "mac",
      data,
      movieKey: MAC_MOVIE_KEY,
      previous,
      now: run.now,
    });
    await run.viewer.store(data.id, record);
    results.push({ videoId: data.id, outcome: "written", clips: record.clips.length });
    return results;
  }, Promise.resolve([]));
}
