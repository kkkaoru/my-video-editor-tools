// Runs with bun (vitest). Clip generation with a fake Workers AI and an in-memory R2 bucket.
import { createHash } from "node:crypto";
import { expect, it, vi } from "vitest";
import {
  type Ai,
  adjusted,
  type Block,
  type Bucket,
  bandOf,
  blockOf,
  boundariesOf,
  type ClipEnv,
  type ClipRecord,
  cheapOf,
  chronological,
  clairicRequest,
  clefRequests,
  climaxSpan,
  clipRecordSchema,
  closed,
  cohesion,
  type Draft,
  densityOf,
  draftOf,
  editorRequest,
  editsOf,
  generate,
  goodOf,
  inputHash,
  judged,
  judgeRequests,
  lookup,
  metered,
  movieKey,
  openings,
  PLACES,
  passing,
  peaksOf,
  pick,
  type Range,
  RECORD_SCHEMA,
  rangesOf,
  refresh,
  regionsOf,
  remembered,
  requestOf,
  resolved,
  rubricRequest,
  type Scored,
  selectorOf,
  slateOf,
  store,
  storiesOf,
  tiles,
  type Unit,
  utterances,
  type VideoData,
} from "./clips.ts";

interface FakeBucket extends Bucket {
  objects: Map<string, string | Uint8Array>;
}

function bucket(entries: Record<string, string | Uint8Array>): FakeBucket {
  const objects = new Map(Object.entries(entries));
  const body = (value: string | Uint8Array) => ({
    json: async () => JSON.parse(String(value)),
    arrayBuffer: async () =>
      typeof value === "string" ? new TextEncoder().encode(value).buffer : value.slice().buffer,
  });
  return {
    objects,
    head: async (key) => {
      const value = objects.get(key);
      return value === undefined ? null : { size: value.length };
    },
    get: async (key, options) => {
      const value = objects.get(key);
      if (value === undefined) return null;
      const range = options?.range;
      return range && typeof value !== "string"
        ? body(value.slice(range.offset, range.offset + range.length))
        : body(value);
    },
    put: async (key, value) => objects.set(key, value),
    delete: async (key) => objects.delete(key),
  };
}

const cue = (start: number, end: number, text: string) => ({ start, end, text });
const said = (start: number, end: number, terms: string[], opens: boolean) => ({
  start,
  end,
  text: "x",
  terms,
  opens,
  closes: true,
});

// 10 s bins, the peak at 100-110 s: a weather aside, then a cake story rising to its climax.
const DATA: VideoData = {
  id: "v1",
  cues: [
    { start: 60, end: 64, text: "今日の天気は", terms: ["天気"], opens: true },
    { start: 66, end: 70, text: "晴れだね", terms: ["天気"], opens: true },
    {
      start: 72,
      end: 77,
      text: "なんでケーキが爆発したと思う？",
      terms: ["ケーキ", "爆発"],
      opens: true,
    },
    { start: 78, end: 83, text: "オーブンに入れたら", terms: ["オーブン"], opens: true },
    { start: 84, end: 86, text: "って", terms: [], opens: false },
    { start: 88, end: 94, text: "ケーキが膨らんで", terms: ["ケーキ"], opens: true },
    { start: 96, end: 104, text: "爆発した！ケーキ爆発！", terms: ["ケーキ", "爆発"], opens: true },
  ],
  excitement: {
    binSeconds: 10,
    speech: { score: [0, 0, 0, 0, 0, 0, 0, 0.2, 0.3, 0.5, 0.9, 0] },
    combined: null,
  },
};

// The topic changes before the question at 72 s; the weather is off topic; question and climax hook.
const ANSWERS = {
  answers: {
    "boundary.2": { noul: 0.9 },
    "clip.0": { score: 3 },
    "clip.1": { score: 3 },
    "clip.2": { score: 3 },
    "clip.3": { score: 3 },
    "clip.4": { score: 4 },
    "question.1": { noul: 0.9 },
    "topic.0": { noul: 0.1 },
    "topic.1": { noul: 0.9 },
    "topic.2": { noul: 0.9 },
    "topic.3": { noul: 0.9 },
    "hook.1.0": { score: 4 },
    "hook.4.0": { score: 4 },
    coherent: { noul: 0.9 },
    unmixed: { noul: 0.7 },
    standalone: { noul: 0.8 },
    ending: { noul: 0.6 },
    topic: { score: 3 },
    hook: { score: 3 },
    flow: { score: 3 },
    good: { noul: 0.9 },
    premise: { noul: 0.5 },
    startsMid: { noul: 0.1 },
    endsMid: { noul: 0.1 },
    stray: { noul: 0.1 },
  },
};

const estimate = (mean: number): unknown => ({ type: "estimate", mean, median: 3, p10: 2, p90: 4 });
const clairicReading = (ending: number, pNull: number): unknown => ({
  answers: {
    topic: estimate(4),
    standalone: estimate(3.5),
    hook: estimate(3),
    ending: estimate(ending),
    flow: estimate(3.5),
    punchline: { type: "text", p_null: pNull, span: [0, 4], value: "爆発", confidence: 0.3 },
  },
});

// Without a clairic reply clairic is down, so the clips are chosen on clef's reading alone.
function fakeAi(clef: unknown, clairic?: unknown): Ai & { calls: string[]; answers: unknown } {
  const calls: string[] = [];
  return {
    calls,
    answers: clef,
    run: async (model, input) => {
      calls.push(model);
      if (model === "@cf/cloudflare/clairic") {
        if (clairic === undefined) throw new Error("clairic unavailable");
        return clairic;
      }
      const count = (JSON.stringify(input).match(/## 候補/g) ?? []).length;
      const content = JSON.stringify({
        clips: Array.from({ length: count }, (_, i) => ({
          id: i + 1,
          title: "題",
          summary: "要約",
        })),
      });
      return model.startsWith("@cf/cloudflare/clef")
        ? clef
        : { choices: [{ message: { content } }] };
    },
  };
}

const scoredUnit = (start: number, end: number, topic: number): Scored => ({
  start,
  end,
  parts: [],
  excitement: 0.5,
  quality: 0.5,
  question: 0,
  topic,
  hook: 1,
  hookStart: start,
  ends: 0,
});

const draft = (
  kind: "15-30" | "35-60",
  score: number,
  ranges: Range[],
  coherence: number,
): Draft => ({
  kind,
  pattern: "hook-to-climax",
  duration: 20,
  score,
  ranges,
  parts: [],
  coherence,
  standalone: 0,
  ending: 1,
  strays: [],
  closes: true,
});

const block = (): Block => {
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  const result = region ? blockOf(region, [ANSWERS]) : null;
  if (!result) throw new Error("no block");
  return result;
};

it("merges cumulative captions into one utterance with the last cue's nouns", () => {
  expect(
    utterances([
      { start: 0, end: 1, text: "あ", terms: [], opens: false },
      { start: 1, end: 2, text: "あい", terms: ["愛"], closes: false },
      cue(5, 6, "う"),
      cue(7, 8, " "),
    ]),
  ).toStrictEqual([
    { start: 0, end: 2, text: "あい", terms: ["愛"], opens: false, closes: false },
    { start: 5, end: 6, text: "う", terms: [], opens: true, closes: true },
  ]);
});

it("puts every clip length into one of the four bands, or none between them", () => {
  expect([
    bandOf(4.9),
    bandOf(5),
    bandOf(10),
    bandOf(12),
    bandOf(15),
    bandOf(30),
    bandOf(33),
    bandOf(35),
    bandOf(60),
    bandOf(75),
    bandOf(90),
    bandOf(120),
    bandOf(121),
  ]).toStrictEqual([
    null,
    "5-10",
    "5-10",
    null,
    "15-30",
    "15-30",
    null,
    "35-60",
    "35-60",
    null,
    "90-120",
    "90-120",
    null,
  ]);
});

it("takes the seeds, then the strongest bins at least the given distance apart", () => {
  expect([
    peaksOf({ score: [0.9, 0.8, 0, 0, 0, 0.7, 0], apart: 3, count: 18, seeds: [] }),
    peaksOf({ score: [0.9, 0.8, 0, 0, 0, 0.7, 0], apart: 3, count: 1, seeds: [] }),
    peaksOf({ score: [0.9, 0.8, 0, 0, 0, 0.7, 0], apart: 3, count: 18, seeds: [2] }),
  ]).toStrictEqual([[0, 5], [0], [2, 5]]);
});

it("counts transcript characters per second in each bin with the bins beside it", () => {
  expect(
    densityOf(utterances(DATA.cues), 10, 12).map((d) => Math.round(d * 10) / 10),
  ).toStrictEqual([0, 0, 0, 0, 0, 1, 3.4, 4.4, 4.5, 2.1, 1.1, 0]);
});

it("mixes loudness and density, each over its maximum, by the density's share", () => {
  expect([
    selectorOf([0, 0.5, 1], [2, 0, 1], 0.5),
    selectorOf([0, 0.5, 1], [2, 0, 1], 0),
    selectorOf([0, 0], [0, 0], 0.5),
  ]).toStrictEqual([
    [0.5, 0.25, 0.75],
    [0, 0.5, 1],
    [0, 0],
  ]);
});

it("keeps loudness's peaks and adds the mix's where the talk is dense", () => {
  // Loud only at 190 s, where nobody speaks; the talk is at 60-104 s.
  const loudAway = {
    ...DATA,
    excitement: {
      binSeconds: 10,
      speech: { score: [...Array.from({ length: 19 }, () => 0), 0.9] },
      combined: null,
    },
  };
  expect([
    regionsOf(DATA, { density: 1, peaks: 18 }).map((r) => r.peak),
    regionsOf(loudAway, { density: 0, peaks: 1 }).map((r) => r.peak),
    regionsOf(loudAway, { density: 0.5, peaks: 2 }).map((r) => r.peak),
  ]).toStrictEqual([[10], [], [8]]);
});

it("cuts units at pauses, at 10 s and at a clean start after 5 s", () => {
  expect(
    tiles([
      said(0, 2, [], true),
      said(3, 4, [], false),
      said(8, 9, [], true),
      said(9, 14, [], false),
      said(14, 15, [], true),
      said(15, 19, [], false),
      said(19, 25, [], false),
    ]).map((group) => group.map((u) => u.start)),
  ).toStrictEqual([[0, 3], [8, 9], [14, 15], [19]]);
});

it("measures how many nouns the two sides of an utterance share", () => {
  const list = [
    said(0, 5, ["猫", "犬"], true),
    said(10, 15, ["猫"], true),
    said(20, 25, ["空"], true),
  ];
  expect([cohesion(list, 10), cohesion(list, 20), cohesion([], 0)]).toStrictEqual([
    0.4999999999999999, 0, 0,
  ]);
});

it("proposes the least cohesive clean starts outside the climax, 8 s apart", () => {
  const list = [
    said(0, 2, ["猫"], true),
    said(3, 4, ["猫"], true),
    said(5, 6, ["犬"], true),
    said(30, 31, ["空"], false),
    said(40, 41, ["空"], true),
  ];
  expect(boundariesOf(list, { start: 39, end: 50 })).toStrictEqual([2]);
  expect(boundariesOf(list, { start: 0, end: 1 })).toStrictEqual([2, 4]);
});

it("cuts each peak's surroundings into units and finds the climax in the peak bin", () => {
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  expect([
    region?.peak,
    region?.bin,
    region?.units.map((u) => [u.start, u.end, u.excitement]),
    region?.climax,
    region?.boundaries,
  ]).toStrictEqual([
    10,
    { start: 100, end: 110 },
    [
      [60, 70, 0.2],
      [72, 77, 0.2],
      [78, 86, 0.3],
      [88, 94, 0.5],
      [96, 104, 0.9],
    ],
    4,
    [2, 5, 6],
  ]);
});

it("has no regions without excitement or without speech in the peak bin", () => {
  const silent = {
    ...DATA,
    excitement: { binSeconds: 10, speech: { score: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.9] } },
  };
  expect([
    regionsOf({ id: "x", cues: DATA.cues }, { density: 0, peaks: 18 }),
    regionsOf(silent, { density: 0, peaks: 18 }),
  ]).toStrictEqual([[], []]);
});

it("prefers the combined score when comments exist", () => {
  const data = {
    ...DATA,
    excitement: {
      binSeconds: 10,
      speech: { score: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.9, 0] },
      combined: { score: [0, 0, 0, 0, 0, 0, 0, 0, 0.5, 0, 0, 0] },
    },
  };
  expect(regionsOf(data, { density: 0, peaks: 18 }).map((r) => [r.peak, r.climax])).toStrictEqual([
    [8, 2],
  ]);
});

it("offers the first clean starts as openings with what their first 3 s say", () => {
  const unit: Unit = {
    start: 0,
    end: 10,
    excitement: 0,
    parts: [
      { ...said(0, 1, [], true), text: "a" },
      { ...said(1, 2, [], false), text: "b" },
      { ...said(2.5, 3, [], true), text: "c" },
      { ...said(9, 10, [], true), text: "d" },
    ],
  };
  expect([openings(unit), openings({ ...unit, end: 3.5 })]).toStrictEqual([
    [
      { start: 0, text: "a b c" },
      { start: 2.5, text: "c" },
    ],
    [{ start: 0, text: "a b c" }],
  ]);
});

it("asks about topic changes, then every unit, in requests of at most 64", () => {
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  const many = region
    ? clefRequests({ ...region, boundaries: Array.from({ length: 50 }, (_, j) => j) })
    : [];
  const one = region ? clefRequests(region) : [];
  expect([
    Object.keys(Object(one[0]).questions),
    Object(one[0]).state.utterances[4],
    many.map((r) => Object.keys(Object(r).questions).length),
  ]).toStrictEqual([
    [
      "boundary.2",
      "boundary.5",
      "boundary.6",
      "clip.0",
      "question.0",
      "ends.0",
      "topic.0",
      "hook.0.0",
      "hook.0.1",
      "clip.1",
      "question.1",
      "ends.1",
      "topic.1",
      "hook.1.0",
      "clip.2",
      "question.2",
      "ends.2",
      "topic.2",
      "hook.2.0",
      "clip.3",
      "question.3",
      "ends.3",
      "topic.3",
      "hook.3.0",
      "clip.4",
      "question.4",
      "ends.4",
      "hook.4.0",
    ],
    { id: 4, unit: 2, time: "0:01:24", text: "って" },
    [64, 11],
  ]);
});

it("asks each region question spelling out its kind, the climax quoted", () => {
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  const request = region ? clefRequests(region)[0] : null;
  expect([
    Object(request).questions["topic.0"],
    Object(request).questions["hook.0.1"],
    Object(request).questions["boundary.2"],
    Object(request)
      .state.description.split("\n")
      .map((line: string) => line.slice(0, 14)),
  ]).toStrictEqual([
    {
      type: "noul",
      instructions:
        "区間0は、山場の区間4「爆発した！ケーキ爆発！」と同じ話題・出来事について話していて、続けて見ると話がつながるか。コメントの読み上げ・あいさつ・お礼・配信の進行・別の話題なら低い",
    },
    {
      type: "score",
      instructions:
        "区間0をここから見せたとき、最初の3秒「晴れだね」は、視聴者が思わず続きを見たくなる強い引き（疑問・驚き・意外な一言・感情の爆発）か。前置き・あいさつ・相づちだけなら弱い",
      criteria: ["引きがない", "弱い", "普通", "強い", "非常に強い"],
    },
    {
      type: "noul",
      instructions:
        "発話1から発話2に移るところで、話題・出来事が切り替わっているか（コメント読み・あいさつ・別の雑談への移行も切り替わり）",
    },
    ["ライブ配信の文字起こしの一部"],
  ]);
});
it("keeps the climax's topic block and scores its units, nouns lifting the topic", () => {
  const result = block();
  expect([
    result.units.map((u) => [u.start, u.end, u.quality, u.question, u.topic, u.hook, u.hookStart]),
    result.climax.start,
    result.span.length,
  ]).toStrictEqual([
    [
      [72, 77, 0.585, 0.9, 1, 1, 72],
      [78, 86, 0.615, 0, 0.9, 0, 78],
      [88, 94, 0.675, 0, 0.9, 0, 88],
      [96, 104, 0.97, 0, 1, 1, 96],
    ],
    96,
    1,
  ]);
});

it("treats a long pause as a topic change and has no block for a missing climax", () => {
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  const paused = region
    ? blockOf(
        {
          ...region,
          said: region.said.map((u, j) => (j === 5 ? { ...u, end: 80 } : u)),
        },
        [{ answers: {} }],
      )
    : null;
  expect([
    paused?.units.map((u) => u.start),
    region ? blockOf({ ...region, climax: 9 }, [ANSWERS]) : "none",
  ]).toStrictEqual([[96], null]);
});

it("merges the climax with the on-topic units around it that stay as exciting", () => {
  const data = {
    ...DATA,
    excitement: {
      binSeconds: 10,
      speech: { score: [0, 0, 0, 0, 0, 0, 0, 0.2, 0.3, 0.9, 0.9, 0] },
      combined: null,
    },
  };
  const region = regionsOf(data, { density: 0, peaks: 18 })[0];
  const result = region ? blockOf(region, [ANSWERS]) : null;
  expect([
    result?.span.map((u) => u.start),
    [result?.climax.start, result?.climax.end, result?.climax.topic, result?.climax.hook],
    result?.units.map((u) => u.start),
  ]).toStrictEqual([
    [88, 96],
    [88, 104, 1, 0],
    [72, 78, 88],
  ]);
});

it("extends the climax over adjacent, on-topic, as exciting units up to 25 s", () => {
  const peak = { ...scoredUnit(20, 30, 1), excitement: 1 };
  const units = [
    { ...scoredUnit(0, 9, 1), excitement: 1 },
    { ...scoredUnit(10, 19, 0.2), excitement: 1 },
    peak,
    { ...scoredUnit(30, 38, 1), excitement: 0.9 },
    { ...scoredUnit(39, 50, 1), excitement: 0.9 },
  ];
  expect([
    climaxSpan(units, peak).map((u) => u.start),
    climaxSpan(
      [
        { ...scoredUnit(10, 19, 1), excitement: 1 },
        peak,
        { ...scoredUnit(34, 40, 1), excitement: 1 },
      ],
      peak,
    ).map((u) => u.start),
    climaxSpan([peak, { ...scoredUnit(30, 38, 1), excitement: 0.5 }], peak).map((u) => u.start),
  ]).toStrictEqual([[20, 30], [10, 20], [20]]);
});

it("carries an unsettled climax on to the on-topic unit that settles its story", () => {
  const peak = scoredUnit(0, 10, 1);
  const next = scoredUnit(11, 18, 0.9);
  const settled = { ...scoredUnit(19, 26, 0.8), ends: 0.7 };
  const after = { ...scoredUnit(27, 30, 0.9), ends: 0.9 };
  expect(resolved([peak], [peak, next, settled, after])).toStrictEqual([peak, next, settled]);
});

it("keeps a climax that settles itself or that nothing on its topic settles soon", () => {
  const peak = scoredUnit(0, 10, 1);
  const done = { ...peak, ends: 0.6 };
  const offTopic = { ...scoredUnit(11, 18, 0.2), ends: 0.9 };
  const far = { ...scoredUnit(25, 35, 1), ends: 0.9 };
  const unsettled = scoredUnit(11, 18, 1);
  expect([
    resolved([done], [done, scoredUnit(11, 18, 1)]),
    resolved([peak], [peak, offTopic]),
    resolved([peak], [peak, far]),
    resolved([peak], [peak, unsettled]),
    resolved([], [peak]),
  ]).toStrictEqual([[done], [peak], [peak], [peak], []]);
});

it("tells the block as climax-first and hook-to-climax where each fits a band", () => {
  expect(
    storiesOf(block()).map((d) => [d.pattern, d.kind, d.duration, d.score, d.ranges]),
  ).toStrictEqual([
    [
      "climax-first",
      "15-30",
      30,
      0.542,
      [
        { start: 96, end: 104 },
        { start: 72, end: 94 },
      ],
    ],
    // The climax span alone fits 5-10 s; the 32 s hook-to-climax cut falls between bands.
    ["hook-to-climax", "5-10", 8, 0.581, [{ start: 96, end: 104 }]],
  ]);
});

it("tells a climax span that opens on a hook as a story of its own", () => {
  const span = [
    { ...scoredUnit(0, 6, 1), hook: 0.9 },
    { ...scoredUnit(6, 20, 1), hook: 0 },
  ];
  const climax = { ...scoredUnit(0, 20, 1), hook: 0.9 };
  expect(
    storiesOf({ peak: 0, units: [climax], climax, span }).map((d) => [d.pattern, d.ranges]),
  ).toStrictEqual([["hook-to-climax", [{ start: 0, end: 20 }]]]);
});

it("keeps no clip whose climax and openers do not hook", () => {
  const weak = {
    answers: { ...ANSWERS.answers, "hook.1.0": { score: 1 }, "hook.4.0": { score: 1 } },
  };
  const region = regionsOf(DATA, { density: 0, peaks: 18 })[0];
  const result = region ? blockOf(region, [weak]) : null;
  expect(result ? storiesOf(result) : null).toStrictEqual([]);
});

it("joins a unit that follows the previous one and starts the first at its opening", () => {
  expect(
    rangesOf([
      { ...scoredUnit(10, 20, 1), hookStart: 12 },
      scoredUnit(22, 30, 1),
      scoredUnit(0, 8, 1),
      scoredUnit(40, 50, 1),
    ]),
  ).toStrictEqual([
    { start: 12, end: 30 },
    { start: 0, end: 8 },
    { start: 40, end: 50 },
  ]);
});

it("joins a unit that starts inside the previous range, so nothing plays twice", () => {
  expect(
    rangesOf([
      scoredUnit(0, 10, 1),
      scoredUnit(9.5, 20, 1),
      scoredUnit(12, 18, 1),
      scoredUnit(40, 50, 1),
      scoredUnit(30, 38, 1),
    ]),
  ).toStrictEqual([
    { start: 0, end: 20 },
    { start: 40, end: 50 },
    { start: 30, end: 38 },
  ]);
});

it("lists ranges in time order with touching ones joined", () => {
  expect(
    chronological([
      { start: 96, end: 104 },
      { start: 72, end: 94 },
      { start: 200, end: 210 },
    ]),
  ).toStrictEqual([
    { start: 72, end: 104 },
    { start: 200, end: 210 },
  ]);
});

it("asks clef about each assembled clip alone, in time order, and about each utterance", () => {
  const stories = storiesOf(block());
  const requests = judgeRequests(stories, utterances(DATA.cues));
  const first = Object(requests[0]);
  expect([requests.length, first.state.utterances, Object.keys(first.questions)]).toStrictEqual([
    2,
    [
      { id: 0, range: 0, time: "0:01:12", text: "なんでケーキが爆発したと思う？" },
      { id: 1, range: 0, time: "0:01:18", text: "オーブンに入れたら" },
      { id: 2, range: 0, time: "0:01:24", text: "って" },
      { id: 3, range: 0, time: "0:01:28", text: "ケーキが膨らんで" },
      { id: 4, range: 0, time: "0:01:36", text: "爆発した！ケーキ爆発！" },
    ],
    [
      "coherent",
      "unmixed",
      "standalone",
      "ending",
      "stray.0",
      "stray.1",
      "stray.2",
      "stray.3",
      "stray.4",
    ],
  ]);
  expect(first.questions["stray.2"]).toStrictEqual({
    type: "noul",
    instructions:
      "発話2は、この切り抜きの主題と関係のない発話（コメントの読み上げ・あいさつ・お礼・配信の進行・別の話題）か",
  });
});

it("asks about at most 60 utterances so a request keeps 64 questions", () => {
  const cues = Array.from({ length: 70 }, (_, i) => cue(i, i + 0.5, `発話${i}`));
  const long: Draft = draft("35-60", 1, [{ start: 0, end: 70 }], 0);
  expect(Object.keys(Object(judgeRequests([long], utterances(cues))[0]).questions).length).toBe(64);
});

it("scores each clip with its coherence, standalone reading, ending and strays", () => {
  const stories = storiesOf(block());
  expect(
    judged(
      stories,
      [
        {
          answers: {
            coherent: { noul: 0.9 },
            unmixed: { noul: 0.7 },
            standalone: { noul: 0.8 },
            ending: { noul: 0.6 },
            "stray.2": { noul: 0.9 },
            "stray.3": { noul: 0.4 },
          },
        },
        { answers: {} },
      ],
      utterances(DATA.cues),
    ).map((d) => [d.coherence, d.standalone, d.ending, d.strays, d.score]),
  ).toStrictEqual([
    [0.8, 0.8, 0.6, [{ start: 84, end: 86 }], 0.842],
    [0, 0, 0, [], 0.581],
  ]);
});

it("drops the earliest lead-up of climax-first and the least on-topic middle otherwise", () => {
  const climaxFirst = draftOf("climax-first", [
    scoredUnit(40, 60, 1),
    scoredUnit(20, 30, 0.6),
    scoredUnit(30, 40, 0.9),
  ])[0];
  const hooked = draftOf("hook-to-climax", [
    scoredUnit(0, 10, 0.8),
    scoredUnit(10, 20, 0.5),
    scoredUnit(20, 30, 0.9),
    scoredUnit(30, 45, 1),
  ])[0];
  const short = draftOf("hook-to-climax", [scoredUnit(0, 10, 1), scoredUnit(10, 20, 1)])[0];
  expect([
    climaxFirst ? adjusted(climaxFirst).map((d) => d.ranges) : null,
    hooked ? adjusted(hooked).map((d) => d.ranges) : null,
    short ? adjusted(short) : null,
  ]).toStrictEqual([
    [
      [
        { start: 40, end: 60 },
        { start: 30, end: 40 },
      ],
    ],
    [
      [
        { start: 0, end: 10 },
        { start: 20, end: 45 },
      ],
    ],
    [],
  ]);
});

const spoken = (start: number, end: number, opens: boolean) => ({
  start,
  end,
  text: "x",
  terms: [],
  opens,
  closes: true,
});
const unitOf = (parts: ReturnType<typeof spoken>[]): Scored => ({
  ...scoredUnit(parts[0]?.start ?? 0, parts.at(-1)?.end ?? 0, 1),
  parts,
});

it("repairs a clip by dropping the unit its strays fill and the strays at its range ends", () => {
  const story = draftOf("hook-to-climax", [
    unitOf([spoken(0, 4, true), spoken(5, 9, true)]),
    unitOf([spoken(10, 14, true), spoken(15, 19, true)]),
    unitOf([spoken(20, 24, true), spoken(25, 29, true), spoken(29.5, 30, true)]),
    unitOf([spoken(30.5, 36, true), spoken(37, 48, true)]),
  ])[0];
  const strays = [
    { start: 10, end: 14 },
    { start: 15, end: 19 },
    { start: 29.5, end: 30 },
  ];
  expect(
    story ? adjusted({ ...story, strays }).map((d) => [d.kind, d.ranges]) : null,
  ).toStrictEqual([
    [
      "35-60",
      [
        { start: 0, end: 9 },
        { start: 20, end: 48 },
      ],
    ],
  ]);
});

it("trims a later range's leading strays up to its next clean start, never the opening's", () => {
  const story = draftOf("climax-first", [
    unitOf([spoken(50, 56, true), spoken(56.5, 62, true)]),
    unitOf([
      spoken(20, 24, true),
      spoken(24.5, 28, false),
      spoken(28.5, 34, true),
      spoken(34.5, 46, true),
    ]),
  ])[0];
  const strays = [
    { start: 20, end: 24 },
    { start: 50, end: 56 },
  ];
  expect(story ? adjusted({ ...story, strays }).map((d) => d.ranges) : null).toStrictEqual([
    [
      { start: 50, end: 62 },
      { start: 28.5, end: 46 },
    ],
  ]);
});

it("reads a clip whose strays change nothing no more", () => {
  const story = draftOf("hook-to-climax", [
    unitOf([spoken(0, 6, true), spoken(6.5, 12, true)]),
    unitOf([spoken(12.5, 20, true)]),
  ])[0];
  expect(story ? adjusted({ ...story, strays: [{ start: 6.5, end: 9 }] }) : null).toStrictEqual([]);
});

it("drops a range left without any utterance on the topic", () => {
  const story = draftOf("climax-first", [
    unitOf([spoken(50, 62, true)]),
    unitOf([spoken(20, 30, true)]),
  ])[0];
  expect(story ? adjusted({ ...story, strays: [{ start: 20, end: 30 }] }) : null).toStrictEqual([]);
});

const SENTENCES = [
  spoken(10, 14, true),
  spoken(14.5, 18, false),
  spoken(30, 36, true),
  spoken(36.5, 40, false),
  spoken(40.5, 44, false),
  spoken(50, 56, true),
];

it("moves range edges onto sentence boundaries while the band and the gaps hold", () => {
  const story: Draft = {
    ...draft(
      "15-30",
      1,
      [
        { start: 10, end: 14 },
        { start: 36.5, end: 40 },
      ],
      0,
    ),
    duration: 7.5,
  };
  expect(closed(story, SENTENCES)).toStrictEqual({
    ...story,
    duration: 22,
    ranges: [
      { start: 10, end: 18 },
      { start: 30, end: 44 },
    ],
  });
  expect(closed({ ...story, kind: "35-60" }, SENTENCES)).toStrictEqual({ ...story, kind: "35-60" });
});

it("lets an overlapping caption that runs past a range end play to its end", () => {
  const overlapping = [
    spoken(0, 6, true),
    spoken(5, 12, true),
    spoken(12.5, 30, true),
    spoken(40, 44, true),
  ];
  const story = draft(
    "15-30",
    1,
    [
      { start: 0, end: 6 },
      { start: 40, end: 44 },
    ],
    0,
  );
  expect(closed(story, overlapping).ranges).toStrictEqual([
    { start: 0, end: 12 },
    { start: 40, end: 44 },
  ]);
  const long = draft(
    "15-30",
    1,
    [
      { start: 0, end: 12 },
      { start: 40, end: 44 },
    ],
    0,
  );
  expect(closed(long, overlapping).ranges).toStrictEqual([
    { start: 0, end: 12 },
    { start: 40, end: 44 },
  ]);
});

it("keeps edges that would run into the clip's other range or past a pause", () => {
  const touching = draft(
    "15-30",
    1,
    [
      { start: 10, end: 14 },
      { start: 14.5, end: 18 },
    ],
    0,
  );
  expect(closed(touching, SENTENCES).ranges).toStrictEqual([
    { start: 10, end: 14 },
    { start: 14.5, end: 18 },
  ]);
  const paused = draft(
    "15-30",
    1,
    [
      { start: 0, end: 4 },
      { start: 30, end: 40 },
    ],
    0,
  );
  expect(
    closed(paused, [spoken(0, 4, true), spoken(20, 22, false), ...SENTENCES.slice(2)]).ranges,
  ).toStrictEqual([
    { start: 0, end: 4 },
    { start: 30, end: 44 },
  ]);
});

it("spends no reading on a stray inside a unit, which only a jump cut could remove", async () => {
  const ai = fakeAi({ answers: { ...ANSWERS.answers, "stray.2": { noul: 0.9 } } });
  const record = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  // The 8 s climax alone now ranks above the 30 s cut it shares the climax with.
  expect([record.clips.map((c) => c.ranges), ai.calls.length]).toStrictEqual([
    [[{ start: 96, end: 104 }]],
    10,
  ]);
});

const answer = (content: string | null): unknown => ({ choices: [{ message: { content } }] });
const sure = (): number => 0.9;

it("ships only clips the judge likely calls good, most likely first", () => {
  const picked = pick(
    [
      draft("15-30", 0.9, [{ start: 0, end: 20 }], 0.9),
      draft("15-30", 0.8, [{ start: 10, end: 30 }], 0.9),
      draft("15-30", 0.7, [{ start: 50, end: 70 }], 0.9),
    ],
    (d) => (d.score === 0.9 ? 0.75 : d.score === 0.8 ? 0.8 : 0.69),
    0.7,
  );
  expect(picked.map((d) => d.ranges[0])).toStrictEqual([{ start: 10, end: 30 }]);
});

it("keeps the best clips per band, never two sharing a moment", () => {
  const picked = pick(
    [
      draft("15-30", 0.9, [{ start: 0, end: 20 }], 0.9),
      draft("15-30", 0.8, [{ start: 20, end: 40 }], 0.9),
      draft("35-60", 0.6, [{ start: 41, end: 80 }], 0.9),
      draft("15-30", 0.5, [{ start: 10, end: 15 }], 0.9),
    ],
    sure,
    0.7,
  );
  const many = pick(
    Array.from({ length: 8 }, (_, i) =>
      draft("15-30", 1, [{ start: i * 100, end: i * 100 + 20 }], 1),
    ),
    sure,
    0.7,
  );
  expect([picked.map((d) => d.ranges[0]), many.length]).toStrictEqual([
    [
      { start: 0, end: 20 },
      { start: 41, end: 80 },
    ],
    6,
  ]);
});

it("reads the sixteen best coherent, closing candidates of a video with the rubric", () => {
  const slate = slateOf(
    [
      { ...draft("15-30", 2, [{ start: 0, end: 20 }], 0.9), closes: false },
      draft("15-30", 3, [{ start: 0, end: 20 }], 0.19),
      ...Array.from({ length: 18 }, (_, i) =>
        draft("15-30", 1 - i / 100, [{ start: i * 100, end: i * 100 + 20 }], 0.9),
      ),
    ],
    16,
  );
  expect([slate.length, slate[0]?.score, slate.at(-1)?.score]).toStrictEqual([16, 1, 0.85]);
});

it("asks the editor about every candidate in one request, each as played under its story", () => {
  const request = editorRequest(
    [
      {
        ...draft(
          "15-30",
          1,
          [
            { start: 96, end: 104 },
            { start: 72, end: 94 },
          ],
          1,
        ),
        pattern: "climax-first",
        duration: 30,
      },
      draft("15-30", 1, [{ start: 60, end: 70 }], 1),
    ],
    utterances(DATA.cues),
    2,
  );
  const lines = Object(request).messages[1].content.split("\n");
  expect([Object(request).temperature, Object(request).seed]).toStrictEqual([0, 2]);
  expect([lines.slice(0, 3), lines.filter((l: string) => l.startsWith("## "))]).toStrictEqual([
    [
      "## 候補1（30秒） 構成: 最初の区間がこの話の山場（一番の盛り上がり）で、続く区間はそこに至るまでの経緯を時系列で見せる構成",
      "[区間1]",
      "爆発した！ケーキ爆発！",
    ],
    [
      "## 候補1（30秒） 構成: 最初の区間がこの話の山場（一番の盛り上がり）で、続く区間はそこに至るまでの経緯を時系列で見せる構成",
      "## 候補2（20秒） 構成: 最初の区間で疑問・気になる起点を見せ、区間が進むにつれて盛り上がり、最後の区間が山場になる構成",
    ],
  ]);
});

it("reads the editor's titles, leaving out malformed and unknown entries", () => {
  const edits = editsOf(
    answer(
      'ok {"clips":[{"id":1,"title":"t1","summary":"s1"},{"id":2,"title":""},{"id":9,"title":"t9","summary":"s9"}]}',
    ),
    5,
  );
  expect([...edits.entries()]).toStrictEqual([[0, { title: "t1", summary: "s1" }]]);
});

it("asks clairic the same rubric as one line per utterance under the clip's story", () => {
  const request = clairicRequest(
    { ...draft("15-30", 1, [{ start: 96, end: 104 }], 1), pattern: "hook-to-climax" },
    utterances(DATA.cues),
  );
  expect([
    Object(request).model,
    Object(request).state,
    Object.keys(Object(request).fields),
    Object(request).fields.ending.type,
    Object(request).fields.punchline.instructions.slice(0, 12),
  ]).toStrictEqual([
    "clairic",
    "[区間1] 爆発した！ケーキ爆発！",
    ["topic", "standalone", "hook", "ending", "flow", "punchline", "premise"],
    "estimate",
    "構成: 疑問や気になる起",
  ]);
});

it("adds clairic's reading to clef's on Cloudflare, keeps clef's chance without a usable one, and reads clef alone on the Mac", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  expect([
    goodOf(ANSWERS, null, "cloudflare"),
    goodOf(ANSWERS, clairicReading(4, 0.01), "cloudflare"),
    goodOf(ANSWERS, clairicReading(1.5, 0.08), "cloudflare"),
    goodOf(ANSWERS, { answers: {} }, "cloudflare"),
    goodOf(ANSWERS, null, "mac"),
    goodOf(ANSWERS, clairicReading(4, 0.01), "mac"),
    goodOf(ANSWERS, ANSWERS, "mac"),
  ]).toStrictEqual([0.845, 0.943, 0.02, 0.845, 0.684, 0.684, 0.684]);
  expect(warn.mock.calls.map((c) => String(c[0]).slice(0, 55))).toStrictEqual([
    "clairic reply outside the form, using the clef-only fit",
  ]);
  warn.mockRestore();
});

it("asks clef the judge's rubric about a clip as played and turns the answers into a chance", () => {
  const request = rubricRequest(
    { ...draft("15-30", 1, [{ start: 96, end: 104 }], 1), pattern: "climax-first" },
    utterances(DATA.cues),
    "clef",
  );
  expect([
    Object(request).state.utterances,
    Object.keys(Object(request).questions),
    goodOf(ANSWERS, null, "cloudflare"),
    goodOf({ answers: { ...ANSWERS.answers, good: { noul: 0.1 } } }, null, "cloudflare"),
    goodOf({ answers: {} }, null, "cloudflare"),
  ]).toStrictEqual([
    [{ range: 1, text: "爆発した！ケーキ爆発！" }],
    [
      "topic",
      "standalone",
      "hook",
      "ending",
      "flow",
      "good",
      "premise",
      "startsMid",
      "endsMid",
      "stray",
    ],
    0.845,
    0.48,
    0.048,
  ]);
});

it("throws on an editor reply outside the form, so the queue retries", () => {
  expect(
    [
      () => editsOf(answer("not json {"), 1),
      () => editsOf(answer('{"title":"t"}'), 1),
      () => editsOf({ error: "x" }, 1),
    ].map((f) => {
      try {
        f();
        return "ok";
      } catch {
        return "threw";
      }
    }),
  ).toStrictEqual(["threw", "threw", "threw"]);
});

it("titles new clips in one editor call and keeps unchanged titles without one", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const ai = fakeAi(ANSWERS);
  const now = new Date("2026-10-21T00:00:00Z");
  const first = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  });
  const second = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: first,
    now,
  });
  expect([first.clips, ai.calls.length, second.clips]).toStrictEqual([
    [
      {
        id: "c1",
        kind: "5-10",
        pattern: "hook-to-climax",
        duration: 8,
        score: 0.881,
        ranges: [{ start: 96, end: 104 }],
        title: "題",
        summary: "要約",
      },
    ],
    19,
    first.clips,
  ]);
  // clairic is down in fakeAi, and each of the 2 + 2 slate readings says so.
  expect(warn.mock.calls.map((c) => String(c[0]))).toStrictEqual([
    "clairic failed, using the clef-only fit: Error: clairic unavailable",
    "clairic failed, using the clef-only fit: Error: clairic unavailable",
    "clairic failed, using the clef-only fit: Error: clairic unavailable",
    "clairic failed, using the clef-only fit: Error: clairic unavailable",
  ]);
  warn.mockRestore();
});

it("ships what the rubric passes whatever the old readings, and drops a cut ending mid-sentence", async () => {
  const unsure = fakeAi({ answers: { ...ANSWERS.answers, standalone: { noul: 0.1 } } });
  const now = new Date("2026-10-21T00:00:00Z");
  const asked = await generate({
    place: "cloudflare",
    ai: unsure,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  });
  const hanging = {
    ...DATA,
    cues: DATA.cues.map((c) => (c.start === 88 ? { ...c, closes: false } : c)),
  };
  const sure = fakeAi(ANSWERS);
  const dropped = await generate({
    place: "cloudflare",
    ai: sure,
    data: hanging,
    movieKey: "k",
    previous: null,
    now,
  });
  expect([
    asked.clips.map((c) => c.ranges),
    unsure.calls.length,
    dropped.clips.map((c) => [c.pattern, c.ranges]),
  ]).toStrictEqual([
    [[{ start: 96, end: 104 }]],
    10,
    [["hook-to-climax", [{ start: 96, end: 104 }]]],
  ]);
});

it("asks the editor again after a reply outside the form, and fails the video after three", async () => {
  const replies = (bad: number): Ai & { calls: string[]; answers: unknown } => {
    const ai = fakeAi(ANSWERS);
    const left = { bad };
    return {
      calls: ai.calls,
      answers: ai.answers,
      run: async (model, input) => {
        if (model !== "@cf/zai-org/glm-5.3-flash" || left.bad === 0) return ai.run(model, input);
        left.bad -= 1;
        ai.calls.push(model);
        return { choices: [{ message: { content: "busy" } }] };
      },
    };
  };
  const now = new Date("2026-10-21T00:00:00Z");
  const once = replies(1);
  const record = await generate({
    place: "cloudflare",
    ai: once,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  });
  const always = replies(3);
  const failed = await generate({
    place: "cloudflare",
    ai: always,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  })
    .then(() => "written")
    .catch(() => "failed");
  expect([record.clips.length, once.calls.length, failed, always.calls.length]).toStrictEqual([
    1,
    11,
    "failed",
    12,
  ]);
});

it("asks the editor again when it titles only some clips, and warns about any still untitled", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const untitled = (empty: number): Ai & { calls: string[]; answers: unknown } => {
    const ai = fakeAi(ANSWERS);
    const left = { empty };
    return {
      calls: ai.calls,
      answers: ai.answers,
      run: async (model, input) => {
        if (model !== "@cf/zai-org/glm-5.3-flash" || left.empty === 0) return ai.run(model, input);
        left.empty -= 1;
        ai.calls.push(model);
        return { choices: [{ message: { content: '{"clips":[]}' } }] };
      },
    };
  };
  const now = new Date("2026-10-21T00:00:00Z");
  const once = untitled(1);
  const titled = await generate({
    place: "cloudflare",
    ai: once,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  });
  const always = untitled(3);
  const dropped = await generate({
    place: "cloudflare",
    ai: always,
    data: DATA,
    movieKey: "k",
    previous: null,
    now,
  });
  const warnings = warn.mock.calls.map((call) => String(call[0]));
  warn.mockRestore();
  expect([
    titled.clips.map((c) => c.title),
    once.calls.length,
    dropped.clips.length,
    always.calls.length,
    warnings.filter((w) => w.startsWith("clip editor")),
  ]).toStrictEqual([["題"], 11, 0, 12, ["clip editor titled 0 of 1 clips; the rest are dropped"]]);
});

it("ships nothing and calls no editor when the rubric doubts every clip", async () => {
  const ai = fakeAi({ answers: { ...ANSWERS.answers, good: { noul: 0.1 } } });
  const record = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  expect([record.clips, ai.calls.filter((m) => m === "@cf/zai-org/glm-5.3-flash")]).toStrictEqual([
    [],
    [],
  ]);
});

it("reads an incoherent clip again without its weakest unit", async () => {
  const ai = fakeAi({
    answers: { ...ANSWERS.answers, coherent: { noul: 0.1 }, unmixed: { noul: 0.1 } },
  });
  const record = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  // One region request, then both stories read as built and the 30 s cut after its repairs; the
  // 8 s climax alone has no weaker unit to drop, and a repeated draft is not read again.
  expect([record.clips, ai.calls.length]).toStrictEqual([[], 4]);
});

it("generates nothing and calls no model without candidates", async () => {
  const ai = fakeAi(ANSWERS);
  const record = await generate({
    place: "cloudflare",
    ai,
    data: { id: "x", cues: [] },
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  expect([record.clips, record.excitement, ai.calls]).toStrictEqual([[], false, []]);
});

it("hashes inputs so a changed cue or noun changes the hash", async () => {
  const changed = { ...DATA, cues: [cue(0, 1, "x")] };
  const renamed = {
    ...DATA,
    cues: DATA.cues.map((c, i) => (i === 0 ? { ...c, terms: ["雨"] } : c)),
  };
  expect([
    (await inputHash(DATA, "cloudflare")) === (await inputHash(changed, "cloudflare")),
    (await inputHash(DATA, "cloudflare")) === (await inputHash(renamed, "cloudflare")),
  ]).toStrictEqual([false, false]);
});

it("computes the same movie key as replay.excitement.movie_key", async () => {
  const movie = Uint8Array.from({ length: 1572864 }, (_, i) => i % 251);
  const store = bucket({ "media/a.mp4": movie, "media/abc.mp4": "abc", "media/empty.mp4": "" });
  expect([
    await movieKey(store, "media/a.mp4"),
    await movieKey(store, "media/abc.mp4"),
    await movieKey(store, "media/empty.mp4"),
    await movieKey(store, "media/none.mp4"),
  ]).toStrictEqual([
    "3d890c891d3c9bf5344b66383deee2a9",
    "2e8e504c5e6b886af70f8c03cbfc31ac",
    "5feceb66ffc86f38d952786c6d696c79",
    null,
  ]);
});

function clipEnv(entries: Record<string, string | Uint8Array>) {
  const env: ClipEnv & { MEDIA: FakeBucket } = { MEDIA: bucket(entries) };
  return { env };
}

const ABC_KEY = "2e8e504c5e6b886af70f8c03cbfc31ac";

it("only points a video at its movie key, computing nothing, whatever the message asks", async () => {
  const { env } = clipEnv({
    "data/v1.json": JSON.stringify(DATA),
    "media/v1.mp4": "abc",
    "clips/pending/v1.json": "{}",
  });
  expect([
    await refresh(env, { videoId: "v1", force: false }),
    await refresh(env, { videoId: "v1", force: true }),
    env.MEDIA.objects.get("clips/videos/v1.json"),
    env.MEDIA.objects.has(`clips/${ABC_KEY}.json`),
    env.MEDIA.objects.has("clips/pending/v1.json"),
  ]).toStrictEqual([
    "skipped",
    "skipped",
    '{"movieKey":"2e8e504c5e6b886af70f8c03cbfc31ac"}',
    false,
    false,
  ]);
});

it("uses the original movie of a video that is not edited yet", async () => {
  const { env } = clipEnv({
    "data/v1.json": JSON.stringify(DATA),
    "catalog.json": JSON.stringify({ videos: [{ id: "v1", original: { movie: "raw.mp4" } }] }),
    "media-original/raw.mp4": "abc",
  });
  await refresh(env, { videoId: "v1", force: false });
  expect(env.MEDIA.objects.get("clips/videos/v1.json")).toBe(
    '{"movieKey":"2e8e504c5e6b886af70f8c03cbfc31ac"}',
  );
});

it("stores a Mac record under the derived movie key and refuses others", async () => {
  const record = {
    version: 9,
    videoId: "v1",
    movieKey: "local",
    inputHash: "h",
    models: { score: "s", summary: "m" },
    createdAt: "t",
    clips: [],
  };
  const { env } = clipEnv({
    "data/v1.json": JSON.stringify(DATA),
    "media/v1.mp4": "abc",
    "clips/pending/v1.json": "{}",
  });
  const { env: bare } = clipEnv({ "data/v1.json": JSON.stringify(DATA), "catalog.json": "{}" });
  expect([
    await store(env, "v1", record),
    JSON.parse(String(env.MEDIA.objects.get(`clips/${ABC_KEY}.json`))).movieKey,
    env.MEDIA.objects.has("clips/pending/v1.json"),
    await store(env, "v2", record),
    await store(env, "v1", { videoId: "v1" }),
    await store(env, "v2", { ...record, videoId: "v2" }),
    await store(bare, "v1", record),
  ]).toStrictEqual([
    "written",
    "2e8e504c5e6b886af70f8c03cbfc31ac",
    false,
    "invalid",
    "invalid",
    "no-data",
    "no-movie",
  ]);
});

it("writes a record again after a transient R2 failure and gives up after three", async () => {
  const record: ClipRecord = {
    version: 10,
    videoId: "v1",
    movieKey: "local",
    inputHash: "h",
    models: { score: "s", summary: "m" },
    createdAt: "t",
    clips: [],
  };
  const { env } = clipEnv({ "data/v1.json": JSON.stringify(DATA), "media/v1.mp4": "abc" });
  const put = env.MEDIA.put.bind(env.MEDIA);
  const failures = { left: 1 };
  env.MEDIA.put = async (key: string, value: string, options: unknown) => {
    if (failures.left > 0) {
      failures.left -= 1;
      throw new Error("put: We encountered an internal error. Please try again. (10001)");
    }
    return put(key, value, options);
  };
  const written = await store(env, "v1", record);
  failures.left = 6;
  const lost = await store(env, "v1", record).catch((error: unknown) => String(error));
  expect([written, lost]).toStrictEqual([
    "written",
    "Error: put: We encountered an internal error. Please try again. (10001)",
  ]);
});

it("skips videos without data or movie", async () => {
  const { env } = clipEnv({ "data/v1.json": JSON.stringify(DATA), "catalog.json": "{}" });
  expect([
    await refresh(env, { videoId: "../x", force: false }),
    await refresh(env, { videoId: "v2", force: false }),
    await refresh(env, { videoId: "v1", force: false }),
  ]).toStrictEqual(["no-data", "no-data", "no-movie"]);
});

it("reads own requests and R2 upload events from the queue", () => {
  expect([
    requestOf({ videoId: "a", force: true }),
    requestOf({ videoId: "a" }),
    requestOf({ action: "PutObject", object: { key: "data/b.json" } }),
    requestOf({ object: { key: "media/b.mp4" } }),
    requestOf("x"),
  ]).toStrictEqual([
    { videoId: "a", force: true },
    { videoId: "a", force: false },
    { videoId: "b", force: false },
    null,
    null,
  ]);
});

const RECORD: ClipRecord = {
  version: 1,
  videoId: "v1",
  movieKey: ABC_KEY,
  inputHash: "h",
  models: { score: "s", summary: "m" },
  createdAt: "t",
  clips: [
    {
      id: "c1",
      kind: "15-30",
      duration: 12,
      score: 1,
      ranges: [{ start: 0, end: 12 }],
      title: "t",
      summary: "s",
    },
  ],
};

it("looks clips up by the video's pointer or a viewer's own key", async () => {
  const { env } = clipEnv({
    "clips/videos/v1.json": JSON.stringify({ movieKey: ABC_KEY }),
    [`clips/${ABC_KEY}.json`]: JSON.stringify(RECORD),
    "clips/pending/v2.json": JSON.stringify({ at: 0, force: false }),
    "clips/pending/v3.json": JSON.stringify({ at: 0, force: true }),
    "clips/videos/v5.json": JSON.stringify({ movieKey: "e".repeat(32) }),
  });
  const now = new Date(60_000);
  expect([
    await lookup(env, { videoId: "v1", key: null, now }),
    await lookup(env, { videoId: "v9", key: ABC_KEY, now }),
    await lookup(env, { videoId: "v1", key: "f".repeat(32), now }),
    await lookup(env, { videoId: "v2", key: null, now }),
    await lookup(env, { videoId: "v3", key: null, now }),
    await lookup(env, { videoId: "v3", key: null, now: new Date(700_000) }),
    await lookup(env, { videoId: "v4", key: null, now }),
    await lookup(env, { videoId: "../x", key: null, now }),
    // The published movie whose clips are not made yet: missing, not another movie's.
    await lookup(env, { videoId: "v5", key: "e".repeat(32), now }),
  ]).toStrictEqual([
    { status: "ready", record: RECORD },
    { status: "ready", record: RECORD },
    { status: "mismatch" },
    { status: "pending", force: false },
    { status: "pending", force: true },
    { status: "missing" },
    { status: "missing" },
    { status: "missing" },
    { status: "missing" },
  ]);
});

it("matches the Python key of a 1.5 MiB pattern through node's hash too", () => {
  const movie = Uint8Array.from({ length: 1572864 }, (_, i) => i % 251);
  const digest = createHash("sha256")
    .update("1572864")
    .update(movie.subarray(0, 1 << 20))
    .update(movie.subarray(524288));
  expect(digest.digest("hex").slice(0, 32)).toBe("3d890c891d3c9bf5344b66383deee2a9");
});

it("counts each model's calls and the input tokens its replies report", async () => {
  const replies: Record<string, unknown> = {
    "@cf/cloudflare/clef": { answers: {}, usage: { input_tokens: 120 } },
    "@cf/zai-org/glm-5.3-flash": { choices: [], usage: { prompt_tokens: 30 } },
    other: { data: [] },
  };
  const ai = metered({ run: async (model) => replies[model] });
  await ai.run("@cf/cloudflare/clef", {});
  await ai.run("@cf/cloudflare/clef", {});
  await ai.run("@cf/zai-org/glm-5.3-flash", {});
  await ai.run("other", {});
  expect(ai.usage).toStrictEqual({
    "@cf/cloudflare/clef": { calls: 2, tokens: 240 },
    "@cf/zai-org/glm-5.3-flash": { calls: 1, tokens: 30 },
    other: { calls: 1, tokens: 0 },
  });
});

it("asks clef each region whole, reads each story, screens and gates both", async () => {
  const fake = fakeAi(ANSWERS);
  const sent: unknown[] = [];
  const ai = {
    run: (model: string, input: unknown) => {
      sent.push(Object.keys(Object(input).questions ?? {}).map((id) => id.split(".")[0]));
      return fake.run(model, input);
    },
  };
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  warn.mockRestore();
  expect([fake.calls, sent.map((kinds) => [...new Set(Object(kinds))])]).toStrictEqual([
    [
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clairic",
      "@cf/cloudflare/clairic",
      "@cf/cloudflare/clef-flash",
      "@cf/cloudflare/clef-flash",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/zai-org/glm-5.3-flash",
    ],
    [
      ["boundary", "clip", "question", "ends", "topic", "hook"],
      ["coherent", "unmixed", "standalone", "ending", "stray"],
      ["coherent", "unmixed", "standalone", "ending", "stray"],
      [],
      [],
      [
        "topic",
        "standalone",
        "hook",
        "ending",
        "flow",
        "good",
        "premise",
        "startsMid",
        "endsMid",
        "stray",
      ],
      [
        "topic",
        "standalone",
        "hook",
        "ending",
        "flow",
        "good",
        "premise",
        "startsMid",
        "endsMid",
        "stray",
      ],
      [
        "topic",
        "standalone",
        "hook",
        "ending",
        "flow",
        "good",
        "premise",
        "startsMid",
        "endsMid",
        "stray",
      ],
      [
        "topic",
        "standalone",
        "hook",
        "ending",
        "flow",
        "good",
        "premise",
        "startsMid",
        "endsMid",
        "stray",
      ],
      [],
    ],
  ]);
});

it("sends clef an identical request once per generation and other models every time", async () => {
  const run = vi.fn(async (_model: string, _input: unknown) => ({ answers: {} }));
  const ai = remembered({ run });
  await ai.run("@cf/cloudflare/clef", { a: 1 });
  await ai.run("@cf/cloudflare/clef", { a: 1 });
  await ai.run("@cf/cloudflare/clef", { a: 2 });
  await ai.run("@cf/cloudflare/clairic", { a: 1 });
  await ai.run("@cf/cloudflare/clairic", { a: 1 });
  expect(run.mock.calls.map(([model, input]) => `${model} ${JSON.stringify(input)}`)).toStrictEqual(
    [
      '@cf/cloudflare/clef {"a":1}',
      '@cf/cloudflare/clef {"a":2}',
      '@cf/cloudflare/clairic {"a":1}',
      '@cf/cloudflare/clairic {"a":1}',
    ],
  );
});

it("asks no more of a region whose climax request holds no strong opening", async () => {
  const answers = Object.fromEntries(
    Object.entries(ANSWERS.answers).map(([id, a]) => [
      id,
      id.startsWith("hook.") ? { score: 1 } : a,
    ]),
  );
  const ai = fakeAi({ answers });
  const record = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  expect([record.clips, ai.calls]).toStrictEqual([[], ["@cf/cloudflare/clef"]]);
});

it("asks only the local clef and clef-flash screen on the Mac, no Cloudflare model, and names the local models", async () => {
  const ai = fakeAi(ANSWERS);
  const record = await generate({
    place: "mac",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  expect([
    ai.calls,
    record.models,
    record.inputHash === (await inputHash(DATA, "mac")),
    (await inputHash(DATA, "mac")) === (await inputHash(DATA, "cloudflare")),
  ]).toStrictEqual([
    [
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef-flash",
      "@cf/cloudflare/clef-flash",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      // The shipped clip's title, which the Mac's model server routes to LFM3.
      "@cf/zai-org/glm-5.3-flash",
    ],
    { score: "mlx-community/clef-OptiQ-4bit", summary: "LiquidAI/LFM3-1.7B-MLX-8bit" },
    true,
    false,
  ]);
});

it("screens a doubtful clip with clef-flash and clairic so clef never reads it", async () => {
  const ai = fakeAi(ANSWERS, clairicReading(1.5, 0.08));
  const record = await generate({
    place: "cloudflare",
    ai,
    data: DATA,
    movieKey: "k",
    previous: null,
    now: new Date(0),
  });
  expect([
    cheapOf(ANSWERS, clairicReading(4, 0.01), "cloudflare"),
    cheapOf(ANSWERS, clairicReading(1.5, 0.08), "cloudflare"),
    cheapOf(ANSWERS, null, "cloudflare"),
    cheapOf(ANSWERS, clairicReading(4, 0.01), "mac"),
    cheapOf(ANSWERS, ANSWERS, "mac"),
    record.clips,
    ai.calls,
  ]).toStrictEqual([
    0.979,
    0.042,
    1,
    // The Mac screens with clef-flash alone, whatever the second reading.
    0.924,
    0.924,
    [],
    [
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clef",
      "@cf/cloudflare/clairic",
      "@cf/cloudflare/clairic",
      "@cf/cloudflare/clef-flash",
      "@cf/cloudflare/clef-flash",
    ],
  ]);
});

it("ships a broadcast's best two at the relative floor on the Mac, and only the gate's least on Cloudflare", () => {
  const ranked = [0.62, 0.58, 0.56, 0.5];
  expect([
    passing(0.7, ranked, PLACES.mac),
    passing(0.62, ranked, PLACES.mac),
    passing(0.58, ranked, PLACES.mac),
    passing(0.56, ranked, PLACES.mac),
    passing(0.5, [0.5], PLACES.mac),
    passing(0.62, ranked, PLACES.cloudflare),
    passing(0.7, ranked, PLACES.cloudflare),
  ]).toStrictEqual([true, true, true, false, false, false, true]);
});

// The record the Mac stores, shared with the Python clips step's test (tests/test_recording_viewer_steps.py):
// a change to either side's record or to RECORD_SCHEMA without the other fails one of the suites.
it("makes the contract record the Worker's schema accepts", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
  const record = await generate({
    place: "mac",
    ai: fakeAi(ANSWERS),
    data: DATA,
    movieKey: "mac",
    previous: null,
    now: new Date("2026-10-29T00:00:00Z"),
  });
  warn.mockRestore();
  const logged = JSON.parse(String(log.mock.calls[0]?.[0]));
  log.mockRestore();
  expect(logged.funnel).toStrictEqual({
    regions: 1,
    stories: 2,
    slate: 2,
    screened: 2,
    gated: 2,
    best: [0.684, 0.684],
    shipped: 1,
    titled: 1,
    clips: 1,
  });
  const contract = { schema: RECORD_SCHEMA, record };
  expect(clipRecordSchema.safeParse(record).success).toBe(true);
  await expect(`${JSON.stringify(contract, null, 1)}\n`).toMatchFileSnapshot(
    "./fixtures/clip-record-contract.json",
  );
});
