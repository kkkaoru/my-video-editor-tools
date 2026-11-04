// Runs with bun (vitest). Tests the HLS ladder publisher (src/hls.ts) with in-memory fakes.
import { expect, it } from "vitest";
import { hlsArgs, indexOf, mismatched, publishHls, stemOf } from "./hls.ts";
import {
  createFakeFs,
  createFakeHttp,
  createFakeRunner,
  fakeFile,
  type RunnerCall,
  zerosEtag,
} from "./sync/fakes.fake.ts";

const DIRECTORY = "/scratch/web-hls/a-review";
const FILES = ["master.m3u8", "stream_0.m3u8", "stream_0.m4s", "stream_3.m4s"];

function setup(options: {
  listed?: Record<string, number>;
  ffmpegCode?: number;
  master?: boolean;
}) {
  const fsHandle = createFakeFs({});
  const logs: string[] = [];
  const { runner, calls } = createFakeRunner((call: RunnerCall) => {
    if (call.command === "ffmpeg") {
      const names = options.master === false ? FILES.slice(1) : FILES;
      for (const name of names) {
        fsHandle.files.set(`${DIRECTORY}/${name}`, fakeFile({ size: 7 }));
      }
      return { code: options.ffmpegCode ?? 0, stderr: "ffmpeg broke" };
    }
    if (call.command === "cf" && call.args[0] === "deploy") {
      return { stdout: "https://up.example.workers.dev" };
    }
    if (call.command === "cf" && call.args[2] === "list") {
      const listed = options.listed ?? {
        "hls/a-review/master.m3u8": 7,
        "hls/a-review/stream_0.m3u8": 7,
        "hls/a-review/stream_0.m4s": 7,
        "hls/a-review/stream_3.m4s": 7,
        "hls/b-review/master.m3u8": 9,
      };
      return {
        stdout: JSON.stringify(Object.entries(listed).map(([key, size]) => ({ key, size }))),
      };
    }
    return {};
  });
  const { http } = createFakeHttp((call) => {
    if (call.url.endsWith("/ready")) {
      return { status: 200, body: { ready: true } };
    }
    if (call.url.endsWith("/create")) {
      return { status: 200, body: { uploadId: "u" } };
    }
    if (call.url.includes("/part")) {
      return { status: 200, body: { partNumber: 1, etag: zerosEtag(call.bodyBytes) } };
    }
    return { status: 200, body: {} };
  });
  const request = {
    services: {
      fs: fsHandle.fs,
      runner,
      http,
      logger: { info: (message: string) => logs.push(message) },
      sleep: async () => {},
      newToken: () => "token",
      projectDirectory: "/repo/web-viewer",
      now: () => 0,
      pid: 1,
      alive: () => true,
    },
    bucket: "bucket",
    uploaderWorker: "uploader",
    source: "/deliveries/a-review.mp4",
    movie: "a-review.mp4",
    directory: DIRECTORY,
    lockWaitMinutes: 1,
  };
  return { request, fsHandle, calls, logs };
}

it("names a movie's HLS directory after its file without the extension", () => {
  expect([stemOf("a-review.mp4"), stemOf("b.MP4"), stemOf("c")]).toStrictEqual([
    "a-review",
    "b",
    "c",
  ]);
});

it("encodes three capped H.264 renditions and one AAC track as byte-range fMP4 HLS", () => {
  expect(hlsArgs({ source: "/in.mp4", directory: "/out" })).toStrictEqual([
    "-hide_banner",
    "-nostdin",
    "-v",
    "error",
    "-i",
    "/in.mp4",
    "-filter_complex",
    "[0:v]split=3[s0][s1][s2];[s0]scale=540:960[v0];[s1]scale=720:1280[v1];[s2]scale=1080:1920[v2]",
    "-map",
    "[v0]",
    "-map",
    "[v1]",
    "-map",
    "[v2]",
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
    "expr:gte(t,n_forced*2)",
    "-sc_threshold",
    "0",
    "-b:v:0",
    "800k",
    "-maxrate:v:0",
    "1200k",
    "-bufsize:v:0",
    "1600k",
    "-b:v:1",
    "1800k",
    "-maxrate:v:1",
    "2700k",
    "-bufsize:v:1",
    "3600k",
    "-b:v:2",
    "3500k",
    "-maxrate:v:2",
    "5250k",
    "-bufsize:v:2",
    "7000k",
    "-c:a",
    "aac",
    "-b:a",
    "160k",
    "-ac",
    "2",
    "-f",
    "hls",
    "-hls_time",
    "6",
    "-hls_playlist_type",
    "vod",
    "-hls_segment_type",
    "fmp4",
    "-hls_flags",
    "single_file+independent_segments",
    "-master_pl_name",
    "master.m3u8",
    "-var_stream_map",
    "v:0,agroup:aud v:1,agroup:aud v:2,agroup:aud a:0,agroup:aud,default:yes",
    "/out/stream_%v.m3u8",
  ]);
});

it("indexes the movies whose master playlist is in the bucket, sorted", () => {
  expect(
    indexOf({
      "hls/b/master.m3u8": 1,
      "hls/b/stream_0.m4s": 2,
      "hls/a/master.m3u8": 1,
      "hls/c/stream_0.m3u8": 1,
    }),
  ).toStrictEqual({ version: 1, movies: ["a", "b"] });
});

it("reports uploaded files whose bucket size differs or is missing", () => {
  expect(mismatched({ a: 1, b: 2, c: 3 }, { a: 1, b: 5 })).toStrictEqual(["b", "c"]);
});

it("encodes, uploads, verifies, publishes the index and removes the local encode", async () => {
  const { request, fsHandle, calls, logs } = setup({});
  const index = await publishHls(request);
  const put = calls.find((call) => call.args[2] === "put");
  expect([
    index,
    put?.args.slice(0, 4),
    fsHandle.calls.includes(`remove ${DIRECTORY}`),
    [...fsHandle.files.keys()].some((path) => path.startsWith(DIRECTORY)),
    logs,
  ]).toStrictEqual([
    { version: 1, movies: ["a-review", "b-review"] },
    ["r2", "objects", "put", "hls/index.json"],
    true,
    false,
    [
      "encoded 4 HLS files for a-review.mp4",
      "uploaded hls/a-review/master.m3u8 (7 B)",
      "uploaded hls/a-review/stream_0.m3u8 (7 B)",
      "uploaded hls/a-review/stream_0.m4s (7 B)",
      "uploaded hls/a-review/stream_3.m4s (7 B)",
    ],
  ]);
});

it("publishes no index when the bucket lacks an uploaded file, and still removes the encode", async () => {
  const { request, fsHandle, calls } = setup({ listed: { "hls/a-review/master.m3u8": 7 } });
  await expect(publishHls(request)).rejects.toThrow(
    "HLS upload incomplete: hls/a-review/stream_0.m3u8, hls/a-review/stream_0.m4s, hls/a-review/stream_3.m4s",
  );
  expect([
    calls.some((call) => call.args[2] === "put"),
    fsHandle.calls.includes(`remove ${DIRECTORY}`),
  ]).toStrictEqual([false, true]);
});

it("stops before uploading when ffmpeg fails or writes no master playlist", async () => {
  const failed = setup({ ffmpegCode: 1 });
  await expect(publishHls(failed.request)).rejects.toThrow("ffmpeg hls failed: ffmpeg broke");
  const headless = setup({ master: false });
  await expect(publishHls(headless.request)).rejects.toThrow(
    "ffmpeg wrote no master.m3u8 in /scratch/web-hls/a-review",
  );
  expect([
    failed.calls.some((call) => call.args[0] === "deploy"),
    headless.calls.some((call) => call.args[0] === "deploy"),
    failed.fsHandle.calls.includes(`remove ${DIRECTORY}`),
  ]).toStrictEqual([false, false, true]);
});
