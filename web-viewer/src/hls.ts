// Runs with bun. Publishes an HLS ladder for one delivered movie, next to (never instead of) its
// MP4: ffmpeg encodes three H.264 renditions plus one AAC track, each a single fMP4 file whose
// segments the playlists address by byte range, so R2 holds a few objects per movie and the
// Worker serves them with its ranged media path and chunk cache. Uploaded files are checked by
// size against the bucket listing, hls/index.json then names every movie with a ladder, and the
// local encode is always removed: the bucket is the only copy (the MP4 stays the source).
import { basename, join } from "node:path";
import { listPrefix, putJson } from "./sync/cf.ts";
import { LOCK_FILE, type LockServices, withSyncLock } from "./sync/lock.ts";
import type { FileSystem, HttpClient, ProcessRunner } from "./sync/ports.ts";
import { deleteUploader, deployUploader, uploadFile, waitUntilReady } from "./sync/uploader.ts";

interface Rendition {
  width: number;
  height: number;
  kbps: number;
}

export interface HlsServices extends LockServices {
  fs: FileSystem;
  runner: ProcessRunner;
  http: HttpClient;
  newToken: () => string;
  projectDirectory: string;
}

export interface HlsRequest {
  services: HlsServices;
  bucket: string;
  uploaderWorker: string;
  // The delivered MP4 and its file name in the catalog (media/<movie>).
  source: string;
  movie: string;
  // An empty work directory for this movie's encode; removed afterwards.
  directory: string;
  lockWaitMinutes: number;
}

export interface HlsIndex {
  version: 1;
  movies: string[];
}

// 9:16 phone ladder (2026-11-04): x264 at 3 Mbps measured VMAF 93 against the 15 Mbps delivery,
// VideoToolbox H.264/HEVC 86/88, so x264 is used; peaks are capped at 1.5x the average.
export const LADDER: readonly Rendition[] = [
  { width: 540, height: 960, kbps: 800 },
  { width: 720, height: 1280, kbps: 1800 },
  { width: 1080, height: 1920, kbps: 3500 },
];
export const SEGMENT_SECONDS = 6;
// Apple's HLS authoring spec: IDR every 2 s keeps every 6 s segment independently decodable.
const KEYFRAME_SECONDS = 2;
const AUDIO_KBPS = 160;
const PEAK_RATIO = 1.5;
const BUFFER_RATIO = 2;
export const HLS_PREFIX = "hls/";
export const INDEX_KEY = "hls/index.json";
export const MASTER = "master.m3u8";
const PLAYLIST = /\.(m3u8|m4s)$/;
const MP4 = /\.mp4$/i;

export const stemOf = (movie: string): string => movie.replace(MP4, "");

export function hlsArgs({ source, directory }: { source: string; directory: string }): string[] {
  const split = LADDER.map((_, index) => `[s${index}]`).join("");
  const scales = LADDER.map(
    ({ width, height }, index) => `[s${index}]scale=${width}:${height}[v${index}]`,
  );
  const rates = LADDER.flatMap(({ kbps }, index) => [
    `-b:v:${index}`,
    `${kbps}k`,
    `-maxrate:v:${index}`,
    `${Math.round(kbps * PEAK_RATIO)}k`,
    `-bufsize:v:${index}`,
    `${kbps * BUFFER_RATIO}k`,
  ]);
  const streams = LADDER.map((_, index) => `v:${index},agroup:aud`).join(" ");
  return [
    "-hide_banner",
    "-nostdin",
    "-v",
    "error",
    "-i",
    source,
    "-filter_complex",
    [`[0:v]split=${LADDER.length}${split}`, ...scales].join(";"),
    ...LADDER.flatMap((_, index) => ["-map", `[v${index}]`]),
    "-map",
    "0:a:0",
    "-c:v",
    "libx264",
    "-preset",
    "medium",
    "-profile:v",
    "high",
    "-pix_fmt",
    "yuv420p",
    "-force_key_frames",
    `expr:gte(t,n_forced*${KEYFRAME_SECONDS})`,
    "-sc_threshold",
    "0",
    ...rates,
    "-c:a",
    "aac",
    "-b:a",
    `${AUDIO_KBPS}k`,
    "-ac",
    "2",
    "-f",
    "hls",
    "-hls_time",
    String(SEGMENT_SECONDS),
    "-hls_playlist_type",
    "vod",
    "-hls_segment_type",
    "fmp4",
    "-hls_flags",
    "single_file+independent_segments",
    "-master_pl_name",
    MASTER,
    "-var_stream_map",
    `${streams} a:0,agroup:aud,default:yes`,
    join(directory, "stream_%v.m3u8"),
  ];
}

// Movies whose master playlist is in the bucket, from a listing of HLS_PREFIX.
export function indexOf(remote: Record<string, number>): HlsIndex {
  const movies = Object.keys(remote)
    .filter((key) => key.endsWith(`/${MASTER}`))
    .map((key) => key.slice(HLS_PREFIX.length, -MASTER.length - 1))
    .sort();
  return { version: 1, movies };
}

// Keys whose bucket size differs from the local file (or is missing).
export const mismatched = (
  local: Record<string, number>,
  remote: Record<string, number>,
): string[] => Object.keys(local).filter((key) => remote[key] !== local[key]);

async function encode(request: HlsRequest): Promise<Record<string, number>> {
  const { fs, runner, projectDirectory } = request.services;
  await fs.mkdir(request.directory);
  const result = await runner.run({
    command: "ffmpeg",
    args: hlsArgs({ source: request.source, directory: request.directory }),
    cwd: projectDirectory,
  });
  if (result.code !== 0) {
    throw new Error(`ffmpeg hls failed: ${result.stderr.slice(-400)}`);
  }
  const names = (await fs.readdir(request.directory)).filter((name) => PLAYLIST.test(name));
  if (!names.includes(MASTER)) {
    throw new Error(`ffmpeg wrote no ${MASTER} in ${request.directory}`);
  }
  return Object.fromEntries(
    await Promise.all(
      names.map(async (name) => [
        `${HLS_PREFIX}${stemOf(request.movie)}/${name}`,
        Number((await fs.stat(join(request.directory, name)))?.size),
      ]),
    ),
  );
}

async function upload(request: HlsRequest, local: Record<string, number>): Promise<void> {
  const { fs, runner, http, sleep, newToken, projectDirectory } = request.services;
  const project = {
    directory: join(projectDirectory, "uploader"),
    projectDirectory,
    name: request.uploaderWorker,
    bucket: request.bucket,
  };
  const token = newToken();
  try {
    const url = await deployUploader({ services: { fs, runner }, project, token });
    await waitUntilReady({ http, sleep, url, token });
    // One file at a time: each upload already sends its parts in parallel.
    await Object.entries(local).reduce(async (previous, [key, size]) => {
      await previous;
      const path = join(request.directory, basename(key));
      await uploadFile({ services: { fs, http }, sleep, url, token, key, path, size });
      request.services.logger.info(`uploaded ${key} (${size} B)`);
    }, Promise.resolve());
  } finally {
    await deleteUploader({ services: { fs, runner }, project });
  }
}

async function publish(request: HlsRequest, local: Record<string, number>): Promise<HlsIndex> {
  const { fs, runner, projectDirectory } = request.services;
  await upload(request, local);
  const remote = await listPrefix({
    runner,
    cwd: projectDirectory,
    bucket: request.bucket,
    prefix: HLS_PREFIX,
  });
  const missing = mismatched(local, remote);
  if (missing.length > 0) {
    throw new Error(`HLS upload incomplete: ${missing.join(", ")}`);
  }
  const index = indexOf(remote);
  const path = join(request.directory, "index.json");
  await fs.writeText({ path, text: `${JSON.stringify(index)}\n`, mode: 0o644 });
  await putJson({ runner, cwd: projectDirectory, bucket: request.bucket, key: INDEX_KEY, path });
  return index;
}

// The encode runs outside the sync lock (it takes minutes per hour of movie); the upload and the
// index update hold it, since the uploader Worker name and the bucket are shared with sync.
export async function publishHls(request: HlsRequest): Promise<HlsIndex> {
  const { services } = request;
  try {
    const local = await encode(request);
    services.logger.info(`encoded ${Object.keys(local).length} HLS files for ${request.movie}`);
    return await withSyncLock(
      {
        services,
        path: join(services.projectDirectory, LOCK_FILE),
        waitMinutes: request.lockWaitMinutes,
      },
      () => publish(request, local),
    );
  } finally {
    await services.fs.removeTree(request.directory);
  }
}
