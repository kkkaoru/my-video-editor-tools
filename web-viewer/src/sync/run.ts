// Runs with bun. Publishes new or re-edited videos to the private viewer: rebuilds the catalog,
// fingerprints every source file (movie, original, transcript, summary and neighbours), plans
// the changed objects and uploads media first, then data JSON, then the catalog.
import { basename, join, resolve } from "node:path";
import { z } from "zod";
import { listRemoteSizes, putJson } from "./cf.ts";
import { collectSources, fingerprintPath } from "./fingerprint.ts";
import type { SyncOptions } from "./options.ts";
import { type Change, isMediaChange, type MediaChange, planChanges, settledKey } from "./plan.ts";
import type { FileSystem, HttpClient, Logger, ProcessRunner } from "./ports.ts";
import {
  type Catalog,
  catalogSchema,
  configSchema,
  type Sources,
  type SyncConfig,
  type SyncState,
} from "./schemas.ts";
import { loadState, nextState, saveState } from "./state.ts";
import {
  deleteUploader,
  deployUploader,
  ensureProxy,
  uploadFile,
  waitUntilReady,
} from "./uploader.ts";

export interface SyncServices {
  fs: FileSystem;
  runner: ProcessRunner;
  http: HttpClient;
  logger: Logger;
  sleep: (milliseconds: number) => Promise<void>;
  newToken: () => string;
  projectDirectory: string;
  // Milliseconds; when given, each phase logs one "PHASE <name> <seconds> s" line.
  clock?: () => number;
}

export interface SyncRequest {
  options: SyncOptions;
  services: SyncServices;
}

interface Built {
  config: SyncConfig;
  catalog: Catalog;
  nextDirectory: string;
}

interface Measured {
  current: Record<string, Sources>;
  dataSha: Record<string, string>;
  catalogSha: string;
}

interface MediaRequest {
  services: SyncServices;
  config: SyncConfig;
  media: MediaChange[];
  onUploaded: (
    change: MediaChange,
    uploaded: { size: number; sha256: string; etag: string },
  ) => Promise<void>;
}

const CONFIG_FILE = "sync.config.json";
const MILLISECONDS = 1000;
const MEGABYTE = 1_000_000;

async function timed<T>(
  services: SyncServices,
  name: string,
  work: () => Promise<T>,
  detail: (result: T, seconds: number) => string = () => "",
): Promise<T> {
  const { clock, logger } = services;
  if (clock === undefined) {
    return work();
  }
  const start = clock();
  const result = await work();
  const seconds = (clock() - start) / MILLISECONDS;
  const extra = detail(result, seconds);
  logger.info(`PHASE ${name} ${seconds.toFixed(1)} s${extra === "" ? "" : ` ${extra}`}`);
  return result;
}

const STATE_FILE = "sync-state.json";
// A copy of the catalog.json last put to the bucket (scopedCatalog keeps entries from it).
const PUBLISHED_CATALOG = "catalog.published.json";

async function build(services: SyncServices): Promise<Built> {
  const { fs, runner, projectDirectory } = services;
  const config = configSchema.parse(
    JSON.parse(await fs.readText(join(projectDirectory, CONFIG_FILE))),
  );
  const nextDirectory = join(projectDirectory, "data.next");
  await fs.removeTree(nextDirectory);
  const youtube =
    config.youtubeOriginals === null
      ? []
      : ["--youtube-originals", resolve(projectDirectory, config.youtubeOriginals)];
  const raw = config.raw === null ? [] : ["--raw", resolve(projectDirectory, config.raw)];
  const result = await runner.run({
    command: "python3",
    args: [
      join(projectDirectory, "tools", "build_catalog.py"),
      "--streamer",
      config.streamer,
      "--out",
      nextDirectory,
      ...config.roots.flatMap((root) => ["--root", resolve(projectDirectory, root)]),
      ...youtube,
      ...raw,
      "--keep",
      join(projectDirectory, "data"),
    ],
    cwd: projectDirectory,
  });
  if (result.code !== 0) {
    throw new Error(`build_catalog failed: ${result.stderr.slice(-400)}`);
  }
  // Part-of-speech hints for the clip points; the analyzer lives in the repository's Python.
  const terms = await runner.run({
    command: "uv",
    args: ["run", "--no-sync", "python", "-m", "replay.clip_terms", nextDirectory],
    cwd: resolve(projectDirectory, ".."),
  });
  if (terms.code !== 0) {
    throw new Error(`clip_terms failed: ${terms.stderr.slice(-400)}`);
  }
  const catalog = catalogSchema.parse(
    JSON.parse(await fs.readText(join(nextDirectory, "catalog.json"))),
  );
  return { config, catalog, nextDirectory };
}

async function measure(args: {
  services: SyncServices;
  built: Built;
  state: SyncState;
}): Promise<Measured> {
  const { fs } = args.services;
  const { catalog, nextDirectory } = args.built;
  const perVideo = await Promise.all(
    catalog.videos.map(async (video) => ({
      id: video.id,
      sources: video.kept
        ? (args.state.sources[video.id] ?? {})
        : await collectSources({ fs, video }),
      sha: await fs.sha256(join(nextDirectory, `${video.id}.json`)),
    })),
  );
  return {
    current: Object.fromEntries(perVideo.map((item) => [item.id, item.sources])),
    dataSha: Object.fromEntries(perVideo.map((item) => [item.id, item.sha])),
    catalogSha: await fs.sha256(join(nextDirectory, "catalog.json")),
  };
}

async function localProxySizes(args: {
  services: SyncServices;
  videos: Catalog["videos"];
}): Promise<Record<string, number>> {
  const { fs, projectDirectory } = args.services;
  const proxies = args.videos.flatMap((video) =>
    video.original === null || !video.original.localPath.toLowerCase().endsWith(".mkv")
      ? []
      : [video.original.movie],
  );
  const stats = await Promise.all(
    proxies.map(async (movie) => ({
      movie,
      stat: await fs.stat(join(projectDirectory, "proxies", movie)),
    })),
  );
  return Object.fromEntries(
    stats.flatMap(
      (item): Array<[string, number]> =>
        item.stat === null ? [] : [[`media-original/${item.movie}`, item.stat.size]],
    ),
  );
}

async function uploadMedia({ services, config, media, onUploaded }: MediaRequest): Promise<void> {
  const { fs, runner, http, logger, sleep, projectDirectory } = services;
  const project = {
    directory: join(projectDirectory, "uploader"),
    projectDirectory,
    name: config.uploaderWorker,
    bucket: config.bucket,
  };
  const token = services.newToken();
  try {
    const url = await timed(services, "deploy", async () => {
      const deployed = await deployUploader({ services: { fs, runner }, project, token });
      await waitUntilReady({ http, sleep, url: deployed, token });
      return deployed;
    });
    await media.reduce(async (previous, change) => {
      await previous;
      const path = change.proxy
        ? join(projectDirectory, "proxies", basename(change.key))
        : change.path;
      if (change.proxy) {
        await fs.removeTree(path);
        await ensureProxy({
          services: { fs, runner },
          source: change.path,
          destination: path,
          cwd: projectDirectory,
        });
      }
      const size = (await fingerprintPath({ fs, path })).size;
      logger.info(`uploading ${change.key} (${size} B)`);
      const uploaded = await timed(
        services,
        "transfer",
        () =>
          uploadFile({ services: { fs, http }, sleep, url, token, key: change.key, path, size }),
        (_parts, seconds) =>
          `${size} B ${(size / MEGABYTE / Math.max(seconds, 1 / MILLISECONDS)).toFixed(1)} MB/s`,
      );
      logger.info(`uploaded ${change.key} in ${uploaded.parts} part(s)`);
      await onUploaded(change, { size, sha256: uploaded.sha256, etag: uploaded.etag });
    }, Promise.resolve());
  } finally {
    await timed(services, "delete", () => deleteUploader({ services: { fs, runner }, project }));
  }
}

// Which planned changes this run sends: --only keeps one video's movies, --originals-only the
// originals. Media left out stays unsettled in the state, so a later sync still sends it.
function selectChanges(changes: Change[], options: SyncOptions): Change[] {
  const scoped =
    options.only === undefined
      ? changes
      : changes.filter((change) => !isMediaChange(change) || change.videoId === options.only);
  return options.originalsOnly
    ? scoped.filter((change) => isMediaChange(change) && change.label === "original")
    : scoped;
}

function requireListed(catalog: Catalog, only: string | undefined): void {
  if (only !== undefined && !catalog.videos.some((video) => video.id === only)) {
    throw new Error(`--only ${only} is not in the catalog`);
  }
}

const catalogFileSchema = z
  .object({ videos: z.array(z.object({ id: z.string() }).passthrough()) })
  .passthrough();

// The built catalog in which each video whose movie is absent from the bucket keeps the entry the
// last published catalog had for it (or none); the built one itself when none is absent.
async function scopedCatalog(args: {
  services: SyncServices;
  directory: string;
  absent: MediaChange[];
}): Promise<{ path: string; sha: string }> {
  const { fs, logger, projectDirectory } = args.services;
  const full = join(args.directory, "catalog.json");
  const omitted = new Set(args.absent.map((change) => change.videoId));
  if (omitted.size === 0) {
    return { path: full, sha: await fs.sha256(full) };
  }
  const raw = catalogFileSchema.parse(JSON.parse(await fs.readText(full)));
  const publishedPath = join(projectDirectory, PUBLISHED_CATALOG);
  const published =
    (await fs.stat(publishedPath)) === null
      ? []
      : catalogFileSchema.parse(JSON.parse(await fs.readText(publishedPath))).videos;
  const previous = new Map(published.map((video) => [video.id, video]));
  const videos = raw.videos.flatMap((video) => {
    if (!omitted.has(video.id)) {
      return [video];
    }
    const kept = previous.get(video.id);
    return kept === undefined ? [] : [kept];
  });
  const path = join(args.directory, "catalog.scoped.json");
  await fs.writeText({ path, text: `${JSON.stringify({ ...raw, videos })}\n`, mode: 0o600 });
  logger.info(
    `catalog keeps the published entry of ${omitted.size} video(s) not in the bucket yet`,
  );
  return { path, sha: await fs.sha256(path) };
}

export async function runSync({ options, services }: SyncRequest): Promise<void> {
  const { fs, runner, logger, projectDirectory } = services;
  const built = await timed(services, "build", () => build(services));
  requireListed(built.catalog, options.only);
  const statePath = join(projectDirectory, STATE_FILE);
  const state = await loadState({ fs, path: statePath });
  const measured = await timed(services, "measure", () => measure({ services, built, state }));
  const proxySizes = await localProxySizes({ services, videos: built.catalog.videos });
  const remoteSizes = await timed(services, "list", () =>
    listRemoteSizes({ runner, cwd: projectDirectory, bucket: built.config.bucket }),
  );
  const plan = planChanges({
    videos: built.catalog.videos,
    current: measured.current,
    dataSha: measured.dataSha,
    catalogSha: measured.catalogSha,
    dataDirectory: built.nextDirectory,
    state,
    remoteSizes,
    proxySizes,
  });
  const media = plan.changes.filter(isMediaChange);
  // A recording's pipeline sends its original early (--originals-only); the later full sync then
  // finds it in the bucket and publishes the data and catalog that name it.
  const planned = selectChanges(plan.changes, options);
  for (const change of planned) {
    logger.info(`changed [${change.kind}] ${change.key} - ${change.reason}`);
  }
  logger.info(`changed: ${planned.length}`);
  if (options.dryRun) {
    await fs.removeTree(built.nextDirectory);
    return;
  }
  const settled = new Set(plan.settledMedia);
  const uploads = { ...state.uploads };
  const stateAfter = (published: boolean, catalogSha = measured.catalogSha) => ({
    ...nextState({
      previous: state,
      current: measured.current,
      dataSha: measured.dataSha,
      catalogSha,
      settledMedia: settled,
      dataPublished: published,
      catalogPublished: published,
    }),
    uploads,
  });
  const sent = options.dataOnly ? [] : planned.filter(isMediaChange);
  if (sent.length > 0) {
    await uploadMedia({
      services,
      config: built.config,
      media: sent,
      onUploaded: async (change, uploaded) => {
        settled.add(settledKey(change.videoId, change.label));
        uploads[change.key] = uploaded;
        await saveState({ fs, path: statePath, state: stateAfter(false) });
      },
    });
  }
  if (options.originalsOnly) {
    await fs.removeTree(built.nextDirectory);
    logger.info("SYNC DONE");
    return;
  }
  const pending = sent.length < media.length;
  // A recording-scoped sync (--only) still publishes the catalog, without only the videos whose
  // movie is neither sent now nor in the bucket, so its own recording is listed at once.
  const scoped = pending && options.only !== undefined;
  const mediaPending = pending && !scoped;
  if (mediaPending && plan.changes.some((change) => change.kind === "catalog")) {
    logger.info(`catalog deferred: ${media.length - sent.length} movie(s) not uploaded yet`);
  }
  const catalog = scoped
    ? await scopedCatalog({
        services,
        directory: built.nextDirectory,
        absent: media.filter(
          (change) =>
            !settled.has(settledKey(change.videoId, change.label)) &&
            remoteSizes[change.key] === undefined,
        ),
      })
    : { path: join(built.nextDirectory, "catalog.json"), sha: measured.catalogSha };
  const small = plan.changes
    .filter((change) => change.kind === "data" || (change.kind === "catalog" && !mediaPending))
    .map((change) => (change.kind === "catalog" ? { ...change, path: catalog.path } : change));
  await timed(
    services,
    "put",
    () =>
      small.reduce(async (previous, change) => {
        await previous;
        await putJson({
          runner,
          cwd: projectDirectory,
          bucket: built.config.bucket,
          key: change.key,
          path: change.path,
        });
        logger.info(`uploaded ${change.key}`);
      }, Promise.resolve()),
    () => `${small.length} object(s)`,
  );
  if (small.some((change) => change.kind === "catalog")) {
    const text = await fs.readText(catalog.path);
    await fs.writeText({ path: join(projectDirectory, PUBLISHED_CATALOG), text, mode: 0o600 });
  }
  await saveState({ fs, path: statePath, state: stateAfter(!mediaPending, catalog.sha) });
  await fs.removeTree(join(projectDirectory, "data"));
  await fs.rename({ from: built.nextDirectory, to: join(projectDirectory, "data") });
  logger.info("SYNC DONE");
}
