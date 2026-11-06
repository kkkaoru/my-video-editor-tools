// Runs with bun. Entry point: `bun run hls <catalog video id>` publishes the HLS ladder of that
// video's delivered movie, encoding it unless `bun run hls --encode <source> <movie>` already did
// (src/hls.ts). The encode goes to the approved external scratch root (AGENTS.md) and is removed
// after the publication; it refuses to start without room for it.
import { randomBytes } from "node:crypto";
import { statfsSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { z } from "zod";
import { encodeHls, type HlsRequest, LADDER, publishHls, stemOf } from "../hls.ts";
import { createNodeFs } from "../sync/nodeFs.ts";
import { createNodeHttp } from "../sync/nodeHttp.ts";
import { createNodeRunner } from "../sync/nodeProcess.ts";
import { configSchema } from "../sync/schemas.ts";

interface Target {
  source: string;
  movie: string;
  needed: number;
}

const TOKEN_BYTES = 36;
const ENCODE = "--encode";
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

// A movie of the catalog: its ladder's bytes follow from its duration.
async function catalogTarget(id: string | undefined): Promise<Target> {
  const video = publishedSchema
    .parse(JSON.parse(await fs.readText(join(projectDirectory, "catalog.published.json"))))
    .videos.find((entry) => entry.id === id);
  if (video?.movie == null || video.localPath == null || !video.duration) {
    console.error("usage: bun run hls <id> (an edited video of catalog.published.json)");
    process.exit(2);
  }
  const bitsPerSecond = LADDER.reduce((sum, { kbps }) => sum + kbps * 1000, AUDIO_BITS);
  return {
    source: video.localPath,
    movie: video.movie,
    needed: (bitsPerSecond / 8) * video.duration * HEADROOM,
  };
}

// A movie not in the catalog yet: the ladder averages well under a delivery's bitrate.
async function encodeTarget(
  source: string | undefined,
  movie: string | undefined,
): Promise<Target> {
  const found = source === undefined ? null : await fs.stat(source);
  if (source === undefined || movie === undefined || found === null) {
    console.error(`usage: bun run hls ${ENCODE} <source> <movie>`);
    process.exit(2);
  }
  return { source, movie, needed: found.size };
}

const encodeOnly = process.argv[2] === ENCODE;
const target = encodeOnly
  ? await encodeTarget(process.argv[3], process.argv[4])
  : await catalogTarget(process.argv[2]);
const volume = statfsSync(SCRATCH_ROOT);
const free = volume.bavail * volume.bsize;
if (free - target.needed < FLOOR_BYTES) {
  console.error(
    `hls: ${WORK_ROOT} has ${free} B free; the encode needs ${Math.round(target.needed)} B`,
  );
  process.exit(1);
}
const config = configSchema.parse(
  JSON.parse(await fs.readText(join(projectDirectory, "sync.config.json"))),
);
const request: HlsRequest = {
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
  source: target.source,
  movie: target.movie,
  directory: join(WORK_ROOT, stemOf(target.movie)),
  lockWaitMinutes: LOCK_WAIT_MINUTES,
};
if (encodeOnly) {
  await encodeHls(request);
} else {
  const index = await publishHls(request);
  console.log(`hls/index.json lists ${index.movies.length} movie(s)`);
}
