// Runs with bun (vitest). The Mac's clip points: routing to the local server, the viewer's bearer
// API and which videos are computed.
import { expect, it } from "vitest";
import { type ClipRecord, GATE_MODEL, inputHash, SCREEN_MODEL, SUMMARY_MODEL } from "./clips.ts";
import { type AnswerStore, localAi, matches, persisted, runMac, viewerOf } from "./clips-mac.ts";

interface Sent {
  url: string;
  method: string;
  body: string | undefined;
}

function fetcher(replies: { ok: boolean; status: number; body: unknown }[]) {
  const sent: Sent[] = [];
  const queue = [...replies];
  return {
    sent,
    fetch: async (
      url: string,
      init: { method: string; headers: Record<string, string>; body?: string },
    ) => {
      sent.push({ url, method: init.method, body: init.body });
      const reply = queue.shift() ?? { ok: false, status: 599, body: null };
      return { ok: reply.ok, status: reply.status, json: async () => reply.body };
    },
  };
}

const EMPTY = { id: "v1", cues: [] };
const RECORD: ClipRecord = {
  version: 9,
  videoId: "v1",
  movieKey: "k",
  inputHash: "old",
  models: { score: "s", summary: "m" },
  createdAt: "t",
  clips: [],
};

it("sends clef, Laya and the summary model to the local server and nothing to Cloudflare", async () => {
  const local = fetcher([
    { ok: true, status: 200, body: { answers: {} } },
    { ok: true, status: 200, body: { choices: [] } },
    { ok: true, status: 200, body: { answers: { good: { noul: 0.5 } } } },
    { ok: false, status: 400, body: null },
  ]);
  const ai = localAi(local.fetch, "http://127.0.0.1:8791");
  const clef = await ai.run("@cf/cloudflare/clef", { state: 1 });
  const glm = await ai.run("@cf/zai-org/glm-5.3-flash", { messages: [] });
  const laya = await ai.run("aac6fef/laya-multilingual-mlx", { state: 2 });
  const failed = await ai.run("@cf/cloudflare/clef", {}).catch((error: unknown) => String(error));
  const clairic = await ai
    .run("@cf/cloudflare/clairic", {})
    .catch((error: unknown) => String(error));
  const flash = await ai
    .run("@cf/cloudflare/clef-omni", {})
    .catch((error: unknown) => String(error));
  expect([clef, glm, laya, failed, clairic, flash, local.sent]).toStrictEqual([
    { answers: {} },
    { choices: [] },
    { answers: { good: { noul: 0.5 } } },
    "Error: local clef answered 400",
    "Error: @cf/cloudflare/clairic is not served on the Mac",
    "Error: @cf/cloudflare/clef-omni is not served on the Mac",
    [
      { url: "http://127.0.0.1:8791/clef", method: "POST", body: '{"state":1}' },
      { url: "http://127.0.0.1:8791/lfm3", method: "POST", body: '{"messages":[]}' },
      { url: "http://127.0.0.1:8791/laya", method: "POST", body: '{"state":2}' },
      { url: "http://127.0.0.1:8791/clef", method: "POST", body: "{}" },
    ],
  ]);
});

it("reads ready records and stores new ones with the bearer", async () => {
  const remote = fetcher([
    { ok: true, status: 200, body: { status: "ready", record: RECORD, schema: 2 } },
    { ok: true, status: 200, body: { status: "missing", schema: 2 } },
    { ok: false, status: 404, body: { status: "missing", schema: 2 } },
    { ok: true, status: 200, body: { outcome: "written" } },
    { ok: false, status: 400, body: { outcome: "invalid", error: "✖ Invalid option" } },
    { ok: false, status: 500, body: null },
    { ok: false, status: 500, body: null },
    { ok: false, status: 500, body: null },
  ]);
  const pauses: number[] = [];
  const viewer = viewerOf(remote.fetch, "https://viewer.test", "t0ken", async (ms) => {
    pauses.push(ms);
  });
  const ready = await viewer.current("v1");
  const missing = await viewer.current("v1");
  const absent = await viewer.current("v2");
  await viewer.store("v1", RECORD);
  const refused = await viewer.store("v1", RECORD).catch((error: unknown) => String(error));
  const failed = await viewer.store("v1", RECORD).catch((error: unknown) => String(error));
  expect([
    ready?.inputHash,
    missing,
    absent,
    refused,
    failed,
    pauses,
    remote.sent.map((s) => `${s.method} ${s.url}`),
  ]).toStrictEqual([
    "old",
    null,
    null,
    "Error: storing clips for v1 answered 400: ✖ Invalid option",
    "Error: storing clips for v1 answered 500",
    [30000, 30000],
    [
      "GET https://viewer.test/api/clips/v1.json",
      "GET https://viewer.test/api/clips/v1.json",
      "GET https://viewer.test/api/clips/v2.json",
      "PUT https://viewer.test/api/clips/v1.json",
      "PUT https://viewer.test/api/clips/v1.json",
      "PUT https://viewer.test/api/clips/v1.json",
      "PUT https://viewer.test/api/clips/v1.json",
      "PUT https://viewer.test/api/clips/v1.json",
    ],
  ]);
});

it("stops before computing when the deployed Worker accepts another record shape", async () => {
  const remote = fetcher([
    { ok: true, status: 200, body: { status: "missing" } },
    { ok: true, status: 200, body: { status: "missing", schema: 3 } },
  ]);
  const viewer = viewerOf(remote.fetch, "https://viewer.test", "t0ken", async () => undefined);
  const old = await viewer.current("v1").catch((error: unknown) => String(error));
  const ahead = await viewer.current("v1").catch((error: unknown) => String(error));
  expect([old, ahead]).toStrictEqual([
    "Error: Worker clip schema v1 deployed, v2 required: deploy web-viewer",
    "Error: Worker clip schema v3 deployed, v2 required: deploy web-viewer",
  ]);
});

it("reports a refusal whose body is not JSON by its status alone", async () => {
  const viewer = viewerOf(
    async () => ({ ok: false, status: 502, json: async () => JSON.parse("<html>") }),
    "https://viewer.test",
    "t0ken",
    async () => undefined,
  );
  const refused = await viewer.store("v1", RECORD).catch((error: unknown) => String(error));
  expect(refused).toBe("Error: storing clips for v1 answered 502");
});

it("sends a record again after a transient server error and stops once it is stored", async () => {
  const remote = fetcher([
    { ok: false, status: 500, body: null },
    { ok: true, status: 200, body: { outcome: "written" } },
  ]);
  const pauses: number[] = [];
  const viewer = viewerOf(remote.fetch, "https://viewer.test", "t0ken", async (ms) => {
    pauses.push(ms);
  });
  await viewer.store("v1", RECORD);
  expect([pauses, remote.sent.map((s) => s.method)]).toStrictEqual([[30000], ["PUT", "PUT"]]);
});

it("keeps records made from the same data on either place", async () => {
  const mac = { ...RECORD, inputHash: await inputHash(EMPTY, "mac") };
  const cloud = { ...RECORD, inputHash: await inputHash(EMPTY, "cloudflare") };
  expect([
    await matches(mac, EMPTY),
    await matches(cloud, EMPTY),
    await matches(RECORD, EMPTY),
  ]).toStrictEqual([true, true, false]);
});

it("computes only videos without a matching record and stores them", async () => {
  const kept = {
    ...RECORD,
    videoId: "v2",
    inputHash: await inputHash({ id: "v2", cues: [] }, "cloudflare"),
  };
  const stored: ClipRecord[] = [];
  const results = await runMac({
    videos: [EMPTY, { id: "v2", cues: [] }],
    force: false,
    ai: { run: async () => Promise.reject(new Error("no model is needed without cues")) },
    viewer: {
      current: async (videoId) => (videoId === "v2" ? kept : null),
      store: async (_videoId, record) => {
        stored.push(record);
      },
    },
    now: new Date(0),
  });
  expect([results, stored.map((r) => [r.videoId, r.movieKey, r.models.summary])]).toStrictEqual([
    [
      { videoId: "v1", outcome: "written", clips: 0 },
      { videoId: "v2", outcome: "current", clips: 0 },
    ],
    [["v1", "mac", "LiquidAI/LFM3-1.7B-MLX-8bit"]],
  ]);
});

it("recomputes a matching record when a user forces it", async () => {
  const kept = { ...RECORD, inputHash: await inputHash(EMPTY, "mac") };
  const stored: string[] = [];
  const results = await runMac({
    videos: [EMPTY],
    force: true,
    ai: { run: async () => Promise.reject(new Error("no model is needed without cues")) },
    viewer: {
      current: async () => kept,
      store: async (videoId) => {
        stored.push(videoId);
      },
    },
    now: new Date(0),
  });
  expect([results, stored]).toStrictEqual([
    [{ videoId: "v1", outcome: "written", clips: 0 }],
    ["v1"],
  ]);
});

function memoryStore(): AnswerStore & { keys: () => string[] } {
  const kept = new Map<string, string>();
  return {
    keys: () => [...kept.keys()],
    read: async (key) => kept.get(key) ?? null,
    write: async (key, value) => {
      kept.set(key, value);
    },
  };
}

it("keeps the deciders' answers between runs and asks the editor again", async () => {
  const asked: string[] = [];
  const server = {
    run: async (model: string, input: unknown) => {
      asked.push(model);
      return { answers: { n: asked.length }, input };
    },
  };
  const store = memoryStore();
  const first = persisted(server, store, "v1");
  const request = { state: "s", questions: { q: {} } };
  expect(await first.run(GATE_MODEL, request)).toStrictEqual({ answers: { n: 1 }, input: request });
  await first.run(SCREEN_MODEL, request);
  await first.run(SUMMARY_MODEL, request);
  expect(first.hits()).toBe(0);
  // A later run (a re-run after a late failure) gets the same answers without asking clef again.
  const second = persisted(server, store, "v1");
  expect(await second.run(GATE_MODEL, request)).toStrictEqual({
    answers: { n: 1 },
    input: request,
  });
  expect(await second.run(SCREEN_MODEL, request)).toStrictEqual({
    answers: { n: 2 },
    input: request,
  });
  await second.run(SUMMARY_MODEL, request);
  expect(second.hits()).toBe(2);
  expect(asked).toStrictEqual([GATE_MODEL, SCREEN_MODEL, SUMMARY_MODEL, SUMMARY_MODEL]);
  // Other server code or model files: asked again.
  const changed = persisted(server, store, "v2");
  await changed.run(GATE_MODEL, request);
  expect(changed.hits()).toBe(0);
  expect(asked.length).toBe(5);
  expect(store.keys().length).toBe(3);
});
