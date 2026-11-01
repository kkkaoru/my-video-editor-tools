// Runs with bun. Entry point: `bun run clips:eval -- --label v5 --sets SETS.json --out DIR
// --cache DIR [--records DIR] [--previous DIR] [--calibrate]` evaluates clip points offline.
// Without --records it runs the Mac's generate() on web-viewer/data/<id>.json against a running
// `python -m replay.clip_models --judges` (port 8791), so no Cloudflare model runs. The judge is Bonsai
// under STRICT_JUDGE, or DeepSeek V4.1 Flash on OpenCode with OPENCODE_API_KEY from --opencode-env. Every call is
// cached, and nothing is written to R2.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { z } from "zod";
import {
  type Ai,
  type ClipRecord,
  clipRecordSchema,
  generate,
  utterances,
  videoDataSchema,
} from "../clips.ts";
import {
  cachedAi,
  calibrate,
  calibration,
  evaluate,
  JUDGE_MODEL,
  LOCAL_JUDGE,
  type Meter,
  markdown,
  openCodeAi,
  type Store,
  type Subset,
  type SubsetSummary,
  strictJudge,
  summarize,
  type VideoEval,
} from "../clips-eval.ts";
import { BONSAI_MODEL, localAi } from "../clips-mac.ts";
import { parseEnvFile } from "../sync/envFile.ts";

const LOCAL_SERVER: string = "http://127.0.0.1:8791";
const UNKNOWN_KEY: string = "0".repeat(32);
const SUBSETS: Subset[] = ["dev", "heldOut"];
const setsSchema = z.object({ dev: z.array(z.string()), heldOut: z.array(z.string()) });
const runSchema = z.object({ label: z.string(), subsets: z.record(z.string(), z.unknown()) });

const projectDirectory = resolve(import.meta.dirname, "..", "..");
const argsSchema = z.object({
  label: z.string().regex(/^[A-Za-z0-9_.]+$/),
  sets: z.string(),
  out: z.string(),
  cache: z.string(),
  records: z.string().optional(),
  previous: z.string().optional(),
  calibrate: z.boolean().optional(),
  "opencode-env": z.string().optional(),
});
const { values } = parseArgs({
  options: {
    label: { type: "string" },
    sets: { type: "string" },
    out: { type: "string" },
    cache: { type: "string" },
    records: { type: "string" },
    previous: { type: "string" },
    calibrate: { type: "boolean" },
    "opencode-env": { type: "string" },
  },
});
const parsed = argsSchema.safeParse(values);
if (!parsed.success) {
  console.error(
    "usage: bun run clips:eval -- --label L --sets SETS.json --out DIR --cache DIR [--opencode-env FILE]",
  );
  process.exit(2);
}
const options = parsed.data;
const { label, sets, out, cache } = options;
mkdirSync(out, { recursive: true });
mkdirSync(cache, { recursive: true });

const readJson = (path: string): unknown => JSON.parse(readFileSync(path, "utf8"));
const store: Store = {
  read: async (key) => {
    const path = join(cache, `${key}.json`);
    return existsSync(path) ? readFileSync(path, "utf8") : null;
  },
  write: async (key, text) => writeFileSync(join(cache, `${key}.json`), text),
};

const local = localAi(fetch, LOCAL_SERVER);
// The judge: Bonsai on the Mac under STRICT_JUDGE (its own model name, so its answers are cached
// apart), or DeepSeek on OpenCode with --opencode-env (the key is read from that file, never printed).
const openCodeFile = options["opencode-env"];
const openCodeKey = openCodeFile
  ? parseEnvFile(readFileSync(openCodeFile, "utf8")).OPENCODE_API_KEY
  : undefined;
if (openCodeFile && !openCodeKey)
  throw new Error("the --opencode-env file sets no OPENCODE_API_KEY");
const judge: Ai = openCodeKey
  ? openCodeAi(fetch, openCodeKey)
  : { run: (_model, input) => local.run(BONSAI_MODEL, input) };
const remote: Ai = {
  run: (model, input) =>
    [JUDGE_MODEL, LOCAL_JUDGE].includes(model) ? judge.run(model, input) : local.run(model, input),
};
/** What the judge is asked as: the local judge under its own name and with the strict instructions. */
const judged = (ai: Ai): Ai =>
  openCodeKey
    ? ai
    : {
        run: (model, input) =>
          ai.run(
            model === JUDGE_MODEL ? LOCAL_JUDGE : model,
            model === JUDGE_MODEL ? strictJudge(input) : input,
          ),
      };

async function recordOf(id: string, meter: Meter): Promise<ClipRecord> {
  const data = videoDataSchema.parse(readJson(join(projectDirectory, "data", `${id}.json`)));
  const saved = options.previous ? join(options.previous, `${id}.json`) : "";
  return options.records
    ? clipRecordSchema.parse(readJson(join(options.records, `${id}.json`)))
    : generate({
        place: "mac",
        ai: cachedAi(remote, store, meter),
        data,
        movieKey: UNKNOWN_KEY,
        previous: saved && existsSync(saved) ? clipRecordSchema.parse(readJson(saved)) : null,
        now: new Date(),
      });
}

async function videoOf(id: string, subset: Subset): Promise<VideoEval> {
  const meter: Meter = { calls: 0, fresh: 0 };
  const started = Date.now();
  const record = await recordOf(id, meter);
  writeFileSync(join(out, `${label}-${id}.record.json`), JSON.stringify(record, null, 1));
  const data = videoDataSchema.parse(readJson(join(projectDirectory, "data", `${id}.json`)));
  const judgeMeter: Meter = { calls: 0, fresh: 0 };
  const result = await evaluate({
    ai: judged(cachedAi(remote, store, judgeMeter)),
    record,
    said: utterances(data.cues),
    subset,
    calls: meter.calls,
  });
  console.error(
    `${subset} ${id.slice(0, 40)} clips ${record.clips.length} calls ${meter.calls} fresh ${meter.fresh} ${Math.round((Date.now() - started) / 1000)} s`,
  );
  return result;
}

const chosen = setsSchema.parse(readJson(sets));
const videos: VideoEval[] = [];
for (const subset of SUBSETS) {
  for (const id of chosen[subset]) videos.push(await videoOf(id, subset));
}
const subsets: Record<Subset, SubsetSummary> = {
  dev: summarize(videos.filter((v) => v.subset === "dev")),
  heldOut: summarize(videos.filter((v) => v.subset === "heldOut")),
};
writeFileSync(join(out, `${label}.json`), JSON.stringify({ label, subsets, videos }, null, 1));

if (options.calibrate) {
  const pairs = [];
  for (const id of chosen.dev) {
    const data = videoDataSchema.parse(readJson(join(projectDirectory, "data", `${id}.json`)));
    const record = clipRecordSchema.parse(readJson(join(out, `${label}-${id}.record.json`)));
    const meter: Meter = { calls: 0, fresh: 0 };
    pairs.push(
      ...(await calibrate({
        ai: judged(cachedAi(remote, store, meter)),
        clips: record.clips,
        said: utterances(data.cues),
      })),
    );
  }
  writeFileSync(join(out, `calibration-${label}.json`), JSON.stringify(pairs, null, 1));
  console.log(JSON.stringify(calibration(pairs)));
}

const runs = readdirSync(out)
  .filter((f) => /^[^-]+\.json$/.test(f) && !f.startsWith("calibration"))
  .sort()
  .map((f) => runSchema.parse(readJson(join(out, f))))
  .map((r) => ({
    label: r.label,
    subsets: { dev: summaryOf(r.subsets.dev), heldOut: summaryOf(r.subsets.heldOut) },
  }));
console.log(markdown(runs));

function summaryOf(value: unknown): SubsetSummary {
  return z
    .object({
      videos: z.number(),
      clips: z.number(),
      clipsPerVideo: z.number(),
      problems: z.number(),
      cleanStartRate: z.number(),
      cleanEndRate: z.number(),
      firstCleanRate: z.number(),
      means: z.object({
        topic: z.number(),
        standalone: z.number(),
        hook: z.number(),
        ending: z.number(),
        flow: z.number(),
      }),
      goodRate: z.number(),
      goodPerVideo: z.number(),
      unjudged: z.number(),
      callsPerVideo: z.number(),
    })
    .parse(value);
}
