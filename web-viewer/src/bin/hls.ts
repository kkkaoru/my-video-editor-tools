// Runs with bun. Entry point: `bun run hls <catalog video id>` encodes and publishes the HLS ladder
// of that video's delivered movie (src/hls.ts). The encode goes to the approved external scratch
// root (AGENTS.md) and is removed afterwards; it refuses to start without room for it.
import { randomBytes } from "node:crypto";
import { statfsSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { z } from "zod";
import { LADDER, publishHls, stemOf } from "../hls.ts";
import { createNodeFs } from "../sync/nodeFs.ts";
import { createNodeHttp } from "../sync/nodeHttp.ts";
import { createNodeRunner } from "../sync/nodeProcess.ts";
import { configSchema } from "../sync/schemas.ts";

const TOKEN_BYTES = 36;
const SCRATCH_ROOT = "/Volumes/m.2-ssd/movie/replay-work";
const WORK_ROOT = join(SCRATCH_ROOT, "web-hls");
const LOCK_WAIT_MINUTES = 60;
// Ladder bytes per second plus audio, with headroom; and the volume's floor kept free.
const AUDIO_BITS = 160_000;
const HEADROOM = 1.5;
const FLOOR_BYTES = 30 * 1024 ** 3;
const publishedSchema = z.object({
  videos: z.array(
    z.object({
      id: z.string(),
      movie: z.string().nullable(),
      duration: z.number().nullable().optional(),
      localPath: z.string().nullable(),
    }),
  ),
});

const projectDirectory = resolve(import.meta.dirname, "..", "..");
const fs = createNodeFs();
const id = process.argv[2];
const video = publishedSchema
  .parse(JSON.parse(await fs.readText(join(projectDirectory, "catalog.published.json"))))
  .videos.find((entry) => entry.id === id);
if (video?.movie == null || video.localPath == null || !video.duration) {
  console.error("usage: bun run hls <id> (an edited video of catalog.published.json)");
  process.exit(2);
}
const bitsPerSecond = LADDER.reduce((sum, { kbps }) => sum + kbps * 1000, AUDIO_BITS);
const needed = (bitsPerSecond / 8) * video.duration * HEADROOM;
const volume = statfsSync(SCRATCH_ROOT);
const free = volume.bavail * volume.bsize;
if (free - needed < FLOOR_BYTES) {
  console.error(`hls: ${WORK_ROOT} has ${free} B free; the encode needs ${Math.round(needed)} B`);
  process.exit(1);
}
const config = configSchema.parse(
  JSON.parse(await fs.readText(join(projectDirectory, "sync.config.json"))),
);
const index = await publishHls({
  services: {
    fs,
    runner: createNodeRunner(process.env),
    http: createNodeHttp(),
    logger: { info: (message) => console.log(message) },
    sleep: (milliseconds) => delay(milliseconds),
    newToken: () => randomBytes(TOKEN_BYTES).toString("base64url"),
    projectDirectory,
    now: () => Date.now(),
    pid: process.pid,
    alive: (pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch (error) {
        return error instanceof Error && "code" in error && error.code === "EPERM";
      }
    },
  },
  bucket: config.bucket,
  uploaderWorker: config.uploaderWorker,
  source: video.localPath,
  movie: video.movie,
  directory: join(WORK_ROOT, stemOf(video.movie)),
  lockWaitMinutes: LOCK_WAIT_MINUTES,
});
console.log(`hls/index.json lists ${index.movies.length} movie(s)`);
