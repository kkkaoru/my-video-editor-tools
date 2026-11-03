// Runs with bun. Entry point: `bun run clips:mac -- [--force] <videoId|data.json>...` computes clip points on
// the Mac (the default) for web-viewer/data/<id>.json (or a copy of it) and stores them through the
// viewer's bearer PUT.
// It reuses a model server already on PORT or starts `python -m replay.clip_models` in its own kept
// uv environment (edits/work/clips-env; the shared .venv is left alone) and stops it afterwards. No
// Cloudflare model runs.
import { type ChildProcess, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { z } from "zod";
import { videoDataSchema } from "../clips.ts";
import { type AnswerStore, localAi, persisted, runMac, viewerOf } from "../clips-mac.ts";
import { parseEnvFile } from "../sync/envFile.ts";

const PORT: number = 8791;
const BASE: string = `http://127.0.0.1:${PORT}`;
const READY_TIMEOUT_MS: number = 20 * 60_000;
const SERVER: string[] = [
  "run",
  "--locked",
  "--extra",
  "clips",
  "python",
  "-B",
  "-m",
  "replay.clip_models",
  "--port",
  String(PORT),
];
const projectDirectory = resolve(import.meta.dirname, "..", "..");
const checkout = resolve(projectDirectory, "..");
/**
 * The server's environment, kept between runs: an isolated one was rebuilt every run (mlx-vlm built
 * from git, about two minutes of the 30-minute budget, and none without the network).
 */
const SERVER_ENVIRONMENT: string = resolve(checkout, "edits/work/clips-env");
/** The deciders' answers of earlier runs (a few kB each, about 1 MB per broadcast). */
const ANSWERS: string = resolve(checkout, "edits/work/clips-answers");
/** What the deciders' answers depend on: the server's code and the model folders' files. */
const IDENTITY_FILES: string[] = [resolve(checkout, "src/replay/clip_models.py")];
const MODEL_FOLDERS: string[] = ["clef-OptiQ-4bit", "clef-flash-OptiQ-4bit"].map((name) =>
  resolve(checkout, "edits/models", name),
);
const secretsSchema = z.object({ CLIPS_API_TOKEN: z.string().min(1) });

/** The code and every model file (name, size, change time) the answers come from. */
function identity(): string {
  const hash = createHash("sha256");
  for (const file of IDENTITY_FILES) hash.update(readFileSync(file));
  for (const folder of MODEL_FOLDERS) {
    for (const name of readdirSync(folder).sort()) {
      const stat = statSync(resolve(folder, name));
      hash.update(`${folder}/${name} ${stat.size} ${stat.mtimeMs}\n`);
    }
  }
  return hash.digest("hex");
}

/** One file per answer under ANSWERS, written whole (renamed into place) or not at all. */
const answerStore: AnswerStore = {
  read: (key) =>
    readFile(resolve(ANSWERS, key.slice(0, 2), `${key}.json`), "utf8").catch(() => null),
  write: async (key, value) => {
    const path = resolve(ANSWERS, key.slice(0, 2), `${key}.json`);
    await mkdir(dirname(path), { recursive: true, mode: 0o700 });
    await writeFile(`${path}.${process.pid}.partial`, value, { mode: 0o600 });
    await rename(`${path}.${process.pid}.partial`, path);
  },
};

/** Our server answers a body it cannot read with 400; nothing listening throws. */
async function serving(): Promise<boolean> {
  const answer = await fetch(`${BASE}/clef`, { method: "POST", body: "{}" }).catch(() => null);
  return answer?.status === 400;
}

function started(): Promise<ChildProcess> {
  const child = spawn("uv", SERVER, {
    cwd: checkout,
    env: { ...process.env, UV_PROJECT_ENVIRONMENT: SERVER_ENVIRONMENT },
    stdio: ["ignore", "pipe", "inherit"],
  });
  return new Promise((done, failed) => {
    const timer = setTimeout(
      () => failed(new Error("the local model server was not ready in time")),
      READY_TIMEOUT_MS,
    );
    child.stdout?.on("data", (chunk: Buffer) => {
      if (!String(chunk).includes(`ready ${PORT}`)) return;
      clearTimeout(timer);
      done(child);
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      failed(new Error(`the local model server exited (${code})`));
    });
  });
}

const force = process.argv.includes("--force");
const ids = process.argv.slice(2).filter((a) => a !== "--force");
if (ids.length === 0) {
  console.error("usage: bun run clips:mac -- [--force] <videoId|data.json>...");
  process.exit(2);
}
const host = parseEnvFile(readFileSync(`${projectDirectory}/deploy.env`, "utf8"))
  .VIEWER_DOMAIN?.split(",")[0]
  ?.trim();
const { CLIPS_API_TOKEN } = secretsSchema.parse(
  JSON.parse(readFileSync(`${projectDirectory}/secrets.json`, "utf8")),
);
if (host === undefined || host === "") throw new Error("VIEWER_DOMAIN must be set in deploy.env");
const videos = ids.map((id) =>
  videoDataSchema.parse(
    JSON.parse(
      readFileSync(id.endsWith(".json") ? id : `${projectDirectory}/data/${id}.json`, "utf8"),
    ),
  ),
);
const server = (await serving()) ? null : await started();
// Its death mid-run shows only as ECONNRESET in the client; record how it ended (10-29: 10/29).
server?.on("exit", (code, signal) =>
  console.error(`local model server exited (${code}, ${signal})`),
);
const ai = persisted(localAi(fetch, BASE), answerStore, identity());
try {
  const results = await runMac({
    videos,
    force,
    ai,
    viewer: viewerOf(fetch, `https://${host}`, CLIPS_API_TOKEN, (ms) => sleep(ms)),
    now: new Date(),
  });
  console.log(results.map((r) => JSON.stringify(r)).join("\n"));
  console.log(JSON.stringify({ answersReused: ai.hits() }));
} finally {
  server?.kill("SIGTERM");
}
