// Runs with bun (vitest). The clip evaluation harness with a fake judge and an in-memory cache.
import { expect, it } from "vitest";
import type { Ai, ClipRecord, Said } from "./clips.ts";
import {
  cachedAi,
  calibrate,
  calibration,
  cleanEnd,
  cleanStart,
  cut,
  evaluate,
  type Grade,
  invariants,
  JUDGE_MODEL,
  judgeRequest,
  type Meter,
  markdown,
  neighboursOf,
  openCodeAi,
  overlap,
  type Pair,
  parseGrade,
  played,
  type Store,
  shapeOf,
  shippedRate,
  spliced,
  strictJudge,
  summarize,
  surrogateOf,
  type VideoEval,
} from "./clips-eval.ts";

const said = (start: number, end: number, text: string, opens: boolean): Said => ({
  start,
  end,
  text,
  terms: [],
  opens,
  closes: true,
});
// 0-2 a sentence, 2-4 its continuation, 4-6 and 6-8 new sentences, 20-22 after a pause.
const SAID: Said[] = [
  said(0, 2, "ケーキ焼いた", true),
  said(2, 4, "けど焦げた", false),
  said(4, 6, "なんで\n焦げたの", true),
  said(6, 8, "温度だよ", true),
  said(20, 22, "天気いいね", false),
  said(700, 702, "全然別の話", true),
  said(702, 712, "続き", true),
];
const GRADE: Grade = {
  topic: 5,
  standalone: 4,
  hook: 4,
  ending: 4,
  flow: 5,
  offTopic: [],
  reason: "一貫",
};
const chat = (content: string | null): unknown => ({ choices: [{ message: { content } }] });
const record = (clips: ClipRecord["clips"]): ClipRecord => ({
  version: 5,
  videoId: "v",
  movieKey: "0".repeat(32),
  inputHash: "h",
  models: { score: "s", summary: "m" },
  createdAt: "2026-10-22T00:00:00.000Z",
  clips,
});
const clip = (
  id: string,
  ranges: { start: number; end: number }[],
): ClipRecord["clips"][number] => ({
  id,
  kind: "short",
  pattern: "climax-first",
  duration: 12,
  score: 1,
  ranges,
  title: "t",
  summary: "s",
});
const memory = (): Store & { keys: () => string[] } => {
  const map = new Map<string, string>();
  return {
    read: async (key) => map.get(key) ?? null,
    write: async (key, text) => {
      map.set(key, text);
    },
    keys: () => [...map.keys()],
  };
};

it("finds clips outside the bands, without a story and touching another clip", () => {
  expect(
    invariants(
      record([
        clip("c1", [
          { start: 0, end: 12 },
          { start: 12, end: 14 },
        ]),
        clip("c2", [{ start: 14, end: 18 }]),
        { ...clip("c3", [{ start: 100, end: 140 }]), pattern: undefined },
      ]),
    ),
  ).toStrictEqual([
    { clip: "c2", rule: "band", detail: "4 s" },
    { clip: "c3", rule: "band", detail: "40 s" },
    { clip: "c3", rule: "pattern", detail: "no story pattern" },
    { clip: "c1", rule: "overlap", detail: "c1 0:12-0:14 touches c2 0:14-0:18" },
  ]);
});

it("checks each clip against its own band, the new ones and the earlier 10-30 and 30-90 s", () => {
  expect(
    invariants(
      record([
        { ...clip("c1", [{ start: 0, end: 10 }]), kind: "5-10" },
        { ...clip("c2", [{ start: 30, end: 75 }]), kind: "long" },
        { ...clip("c3", [{ start: 100, end: 145 }]), kind: "35-60" },
        { ...clip("c4", [{ start: 200, end: 330 }]), kind: "90-120" },
      ]),
    ),
  ).toStrictEqual([{ clip: "c4", rule: "band", detail: "130 s" }]);
});

it("starts cleanly on an opening utterance, after a pause, or with nothing to hear", () => {
  expect(cleanStart({ start: 0, end: 4 }, SAID)).toBe(true);
  expect(cleanStart({ start: 2, end: 4 }, SAID)).toBe(false);
  expect(cleanStart({ start: 1, end: 4 }, SAID)).toBe(false);
  expect(cleanStart({ start: 20, end: 22 }, SAID)).toBe(true);
  expect(cleanStart({ start: 900, end: 910 }, SAID)).toBe(true);
});

it("ends cleanly before an opening utterance, a pause or the stream's end", () => {
  expect(cleanEnd({ start: 0, end: 4 }, SAID)).toBe(true);
  expect(cleanEnd({ start: 0, end: 2 }, SAID)).toBe(false);
  expect(cleanEnd({ start: 0, end: 5 }, SAID)).toBe(false);
  expect(cleanEnd({ start: 4, end: 8 }, SAID)).toBe(true);
  expect(cleanEnd({ start: 700, end: 712 }, SAID)).toBe(true);
  expect(cleanEnd({ start: 30, end: 40 }, SAID)).toBe(true);
});

it("counts the clean edges of a clip's ranges", () => {
  expect(
    shapeOf(
      [
        { start: 2, end: 4 },
        { start: 4, end: 8 },
      ],
      SAID,
    ),
  ).toStrictEqual({ ranges: 2, cleanStarts: 1, cleanEnds: 2, firstClean: false });
  expect(shapeOf([], SAID)).toStrictEqual({
    ranges: 0,
    cleanStarts: 0,
    cleanEnds: 0,
    firstClean: false,
  });
});

it("numbers the utterances in playing order and shows each range to the judge", () => {
  const ranges = [
    { start: 4, end: 8 },
    { start: 0, end: 2 },
  ];
  expect(played(ranges, SAID)).toStrictEqual([
    { number: 1, range: 1, text: "なんで焦げたの" },
    { number: 2, range: 1, text: "温度だよ" },
    { number: 3, range: 2, text: "ケーキ焼いた" },
  ]);
  expect(judgeRequest(ranges, SAID, undefined)).toMatchObject({
    messages: [
      { role: "system" },
      {
        role: "user",
        content:
          "[区間1 0:04〜0:08]\n1: なんで焦げたの\n2: 温度だよ\n[区間2 0:00〜0:02]\n3: ケーキ焼いた",
      },
    ],
    reasoning_effort: "low",
    response_format: { type: "json_object" },
  });
});

it("tells the judge the clip's story so its deliberate order is not marked down", () => {
  expect(
    Object(judgeRequest([{ start: 4, end: 8 }], SAID, "climax-first")).messages[1],
  ).toStrictEqual({
    role: "user",
    content: "構成: 山場→経緯\n[区間1 0:04〜0:08]\n1: なんで焦げたの\n2: 温度だよ",
  });
  expect(
    Object(judgeRequest([{ start: 4, end: 6 }], SAID, "hook-to-climax")).messages[1],
  ).toStrictEqual({
    role: "user",
    content: "構成: 引き→山場\n[区間1 0:04〜0:06]\n1: なんで焦げたの",
  });
});

it("finds a clip whose own ranges overlap, which plays a moment twice", () => {
  expect(
    invariants(
      record([
        clip("c1", [
          { start: 0, end: 10 },
          { start: 9, end: 20 },
          { start: 20, end: 25 },
        ]),
      ]),
    ),
  ).toStrictEqual([{ clip: "c1", rule: "replay", detail: "c1 0:00-0:10 touches c1 0:09-0:20" }]);
});

it("reads the rubric's JSON from the judge's answer, or nothing when it is malformed", () => {
  expect(parseGrade(chat(`前置き ${JSON.stringify(GRADE)} 以上`))).toStrictEqual(GRADE);
  expect(parseGrade(chat('{"topic": 6}'))).toBe(null);
  expect(parseGrade(chat("{not json}"))).toBe(null);
  expect(parseGrade(chat(null))).toBe(null);
  expect(parseGrade({ error: "x" })).toBe(null);
});

it("answers a repeated request from the cache and counts every call", async () => {
  const store = memory();
  const meter: Meter = { calls: 0, fresh: 0 };
  const seen: unknown[] = [];
  const ai = cachedAi(
    {
      run: async (model, input) => {
        seen.push([model, input]);
        return { answer: seen.length };
      },
    },
    store,
    meter,
  );
  expect(await ai.run("m", { a: 1 })).toStrictEqual({ answer: 1 });
  expect(await ai.run("m", { a: 1 })).toStrictEqual({ answer: 1 });
  expect(await ai.run("n", { a: 1 })).toStrictEqual({ answer: 2 });
  expect(meter).toStrictEqual({ calls: 3, fresh: 2 });
  expect(store.keys().length).toBe(2);
});

const judge = (grades: (Grade | null)[]): Ai & { models: string[] } => {
  const models: string[] = [];
  return {
    models,
    run: async (model) => {
      models.push(model);
      const grade = grades[models.length - 1] ?? null;
      return chat(grade ? JSON.stringify(grade) : "わかりません");
    },
  };
};

it("grades every clip of a record with the judge model and checks the rules", async () => {
  const ai = judge([GRADE, null]);
  const result = await evaluate({
    ai,
    record: record([clip("c1", [{ start: 0, end: 4 }]), clip("c2", [{ start: 2, end: 8 }])]),
    said: SAID,
    subset: "dev",
    calls: 7,
  });
  expect(ai.models).toStrictEqual([JUDGE_MODEL, JUDGE_MODEL]);
  expect(result).toStrictEqual({
    id: "v",
    subset: "dev",
    calls: 7,
    problems: [
      { clip: "c1", rule: "band", detail: "4 s" },
      { clip: "c2", rule: "band", detail: "6 s" },
      { clip: "c1", rule: "overlap", detail: "c1 0:00-0:04 touches c2 0:02-0:08" },
    ],
    clips: [
      {
        id: "c1",
        kind: "short",
        duration: 12,
        shape: { ranges: 1, cleanStarts: 1, cleanEnds: 1, firstClean: true },
        grade: GRADE,
      },
      {
        id: "c2",
        kind: "short",
        duration: 12,
        shape: { ranges: 1, cleanStarts: 0, cleanEnds: 1, firstClean: false },
        grade: null,
      },
    ],
  });
});

const video = (clips: VideoEval["clips"], calls: number): VideoEval => ({
  id: "v",
  subset: "heldOut",
  clips,
  problems: [],
  calls,
});

it("summarizes grades, clean edges, good clips and cost per video", () => {
  const shape = { ranges: 2, cleanStarts: 1, cleanEnds: 2, firstClean: true };
  const weak: Grade = { ...GRADE, topic: 3, hook: 2 };
  expect(
    summarize([
      video(
        [
          { id: "c1", kind: "short", duration: 12, shape, grade: GRADE },
          { id: "c2", kind: "long", duration: 40, shape, grade: weak },
        ],
        50,
      ),
      video(
        [
          {
            id: "c1",
            kind: "short",
            duration: 12,
            shape: { ...shape, firstClean: false },
            grade: null,
          },
        ],
        30,
      ),
    ]),
  ).toStrictEqual({
    videos: 2,
    clips: 3,
    clipsPerVideo: 1.5,
    problems: 0,
    cleanStartRate: 0.5,
    cleanEndRate: 1,
    firstCleanRate: 0.667,
    means: { topic: 4, standalone: 4, hook: 3, ending: 4, flow: 5 },
    goodRate: 0.5,
    goodPerVideo: 0.5,
    unjudged: 1,
    callsPerVideo: 40,
  });
});

it("summarizes an empty subset as zeros", () => {
  expect(summarize([])).toStrictEqual({
    videos: 0,
    clips: 0,
    clipsPerVideo: 0,
    problems: 0,
    cleanStartRate: 0,
    cleanEndRate: 0,
    firstCleanRate: 0,
    means: { topic: 0, standalone: 0, hook: 0, ending: 0, flow: 0 },
    goodRate: 0,
    goodPerVideo: 0,
    unjudged: 0,
    callsPerVideo: 0,
  });
});

it("splices a far, unrelated 10 s stretch after the first range", () => {
  expect(
    spliced(
      [
        { start: 0, end: 4 },
        { start: 4, end: 8 },
      ],
      SAID,
    ),
  ).toStrictEqual([
    { start: 0, end: 4 },
    { start: 700, end: 710 },
    { start: 4, end: 8 },
  ]);
  expect(spliced([{ start: 0, end: 8 }], SAID.slice(0, 4))).toStrictEqual([]);
  expect(spliced([], SAID)).toStrictEqual([]);
});

it("cuts each range's first and last utterance, or nothing when a range is too short", () => {
  expect(cut([{ start: 0, end: 8 }], SAID)).toStrictEqual([{ start: 2, end: 6 }]);
  expect(cut([{ start: 0, end: 4 }], SAID)).toStrictEqual([]);
  expect(cut([{ start: 0, end: 6 }], SAID)).toStrictEqual([{ start: 2, end: 4 }]);
  expect(
    cut(
      [
        { start: 0, end: 8 },
        { start: 20, end: 22 },
      ],
      SAID,
    ),
  ).toStrictEqual([]);
});

it("grades each clip as is and with each negative that applies", async () => {
  const altered: Grade = { ...GRADE, topic: 2, hook: 1 };
  const ai = judge([GRADE, altered, altered]);
  const pairs = await calibrate({ ai, clips: [clip("c1", [{ start: 0, end: 8 }])], said: SAID });
  expect(pairs).toStrictEqual([
    { clip: "c1", negative: "spliced", original: GRADE, altered },
    { clip: "c1", negative: "cut", original: GRADE, altered },
  ]);
});

it("counts the pairs the judge ranked correctly, skipping ungraded ones", () => {
  const pair = (negative: Pair["negative"], altered: Grade | null): Pair => ({
    clip: "c",
    negative,
    original: GRADE,
    altered,
  });
  expect(
    calibration([
      pair("spliced", { ...GRADE, topic: 2 }),
      pair("spliced", GRADE),
      pair("cut", { ...GRADE, ending: 1 }),
      pair("cut", null),
    ]),
  ).toStrictEqual({ pairs: 4, correct: { spliced: 1, cut: 1 }, total: { spliced: 2, cut: 1 } });
});

it("tabulates labelled runs side by side for both subsets", () => {
  const empty = summarize([]);
  const table = markdown([
    { label: "v4", subsets: { dev: empty, heldOut: empty } },
    { label: "v5", subsets: { dev: { ...empty, goodRate: 0.8 }, heldOut: empty } },
  ]);
  expect(table.split("\n").slice(0, 6)).toStrictEqual([
    "| measure | v4 dev | v5 dev | v4 heldOut | v5 heldOut |",
    "| --- | ---: | ---: | ---: | ---: |",
    "| videos | 0 | 0 | 0 | 0 |",
    "| clips / video | 0 | 0 | 0 | 0 |",
    "| good clips / video | 0 | 0 | 0 | 0 |",
    "| good rate | 0 | 0.8 | 0 | 0 |",
  ]);
});

it("asks OpenCode's free DeepSeek first and the paid one when the free tier is refused", async () => {
  const sent: { url: string; auth: string | undefined; body: string }[] = [];
  const replies = [
    { ok: false, status: 403, body: null },
    { ok: true, status: 200, body: { choices: [{ message: { content: "{}" } }] } },
    { ok: true, status: 200, body: { choices: [] } },
  ];
  const ai = openCodeAi(async (url, init) => {
    sent.push({ url, auth: init.headers.Authorization, body: init.body });
    const reply = replies.shift() ?? { ok: false, status: 599, body: null };
    return { ok: reply.ok, status: reply.status, json: async () => reply.body };
  }, "k3y");
  const paid = await ai.run(JUDGE_MODEL, { messages: [] });
  const free = await ai.run(JUDGE_MODEL, { messages: [] });
  const none = await openCodeAi(
    async () => ({ ok: false, status: 500, json: async () => null }),
    "k3y",
  )
    .run(JUDGE_MODEL, {})
    .catch((error: unknown) => String(error));
  expect([paid, free, none, sent]).toStrictEqual([
    { choices: [{ message: { content: "{}" } }] },
    { choices: [] },
    "Error: OpenCode answered none of the judge models",
    [
      {
        url: "https://opencode.ai/inference/openai/v1/chat/completions",
        auth: "Bearer k3y",
        body: '{"messages":[],"model":"deepseek-v4.1-flash-free"}',
      },
      {
        url: "https://opencode.ai/inference/openai/v1/chat/completions",
        auth: "Bearer k3y",
        body: '{"messages":[],"model":"deepseek-v4.1-flash"}',
      },
      {
        url: "https://opencode.ai/inference/openai/v1/chat/completions",
        auth: "Bearer k3y",
        body: '{"messages":[],"model":"deepseek-v4.1-flash-free"}',
      },
    ],
  ]);
});

it("adds the strict instructions to the judge's system message only", () => {
  expect([
    strictJudge({
      messages: [
        { role: "system", content: "S" },
        { role: "user", content: "U" },
      ],
      x: 1,
    }),
    strictJudge({}),
  ]).toStrictEqual([
    {
      messages: [
        {
          role: "system",
          content:
            "S\n採点は厳しく行う。各項目の平均は3になるように付け、5は欠点がまったくない場合だけ、迷ったら低い方を選ぶ。\n次のどれかに当てはまる項目は3以下にする: standaloneは前提（誰が・何の話か）が文字起こしにない、endingは最後の発話がオチ・結論・反応になっていない、flowは途中から始まる・話が飛ぶ、topicは主題と無関係な発話がある。",
        },
        { role: "user", content: "U" },
      ],
      x: 1,
    },
    { messages: [] },
  ]);
});

const READING = {
  answers: {
    topic: { mean: 4 },
    standalone: { mean: 3.5 },
    hook: { mean: 3 },
    ending: { mean: 3.5 },
    flow: { mean: 3.5 },
    punchline: { p_null: 0.01 },
  },
};
const RUBRIC_ANSWERS = {
  answers: {
    topic: { score: 3 },
    standalone: { score: 2 },
    hook: { score: 2 },
    ending: { score: 3 },
    flow: { score: 3 },
    good: { noul: 0.6 },
    premise: { noul: 0.5 },
    startsMid: { noul: 0.2 },
    endsMid: { noul: 0.1 },
    stray: { noul: 0.2 },
  },
};

it("measures how much two clips play the same moments", () => {
  expect([
    overlap([{ start: 0, end: 10 }], [{ start: 0, end: 10 }]),
    overlap([{ start: 0, end: 10 }], [{ start: 5, end: 15 }]),
    overlap([{ start: 0, end: 10 }], [{ start: 20, end: 30 }]),
  ]).toStrictEqual([1, 1 / 3, 0]);
});

it("weighs the judged clips of the same moment by IoU squared", () => {
  const judged = [
    { video: "v", ranges: [{ start: 0, end: 10 }], good: true },
    { video: "v", ranges: [{ start: 0, end: 8 }], good: false },
    { video: "w", ranges: [{ start: 0, end: 10 }], good: true },
    { video: "v", ranges: [{ start: 6, end: 30 }], good: true },
  ];
  expect([
    neighboursOf("v", [{ start: 0, end: 10 }], judged),
    neighboursOf("v", [{ start: 40, end: 50 }], judged),
  ]).toStrictEqual([
    { signed: 0.36, count: 2 },
    { signed: 0, count: 0 },
  ]);
});

it("judges with clef, Laya and the judged neighbours, and abstains in between or without Laya", () => {
  const none = { signed: 0, count: 0 };
  const laya = (good: number) => ({ answers: { ...RUBRIC_ANSWERS.answers, good: { noul: good } } });
  const verdicts = [
    surrogateOf(RUBRIC_ANSWERS, laya(0.95), none, 20),
    surrogateOf(RUBRIC_ANSWERS, laya(0.6), none, 20),
    surrogateOf(RUBRIC_ANSWERS, laya(0.6), { signed: -1, count: 1 }, 20),
    surrogateOf(RUBRIC_ANSWERS, laya(0.05), { signed: -1.5, count: 2 }, 20),
    surrogateOf(
      { answers: { ...RUBRIC_ANSWERS.answers, ending: { score: 0.5 }, endsMid: { noul: 0.9 } } },
      laya(0.05),
      none,
      20,
    ),
    surrogateOf(RUBRIC_ANSWERS, READING, none, 20),
    surrogateOf(RUBRIC_ANSWERS, laya(0.95), none, 8),
    surrogateOf(RUBRIC_ANSWERS, laya(0.95), none, 100),
  ];
  expect(verdicts.map((v) => [v.chance, v.verdict, v.domain])).toStrictEqual([
    [0.98, "good", "verified"],
    [0.947, "good", "verified"],
    // A bad judged neighbour of the same moment pulls the chance down.
    [0.89, "good", "verified"],
    [0.523, "unjudged", "verified"],
    [0.106, "bad", "verified"],
    // clairic's reply is not Laya's rubric answers, so it gives no verdict.
    [0, "unjudged", "verified"],
    [0.98, "good", "extrapolated"],
    // Past 60 s the old labels showed the surrogate no better than chance, so it gives no verdict.
    [0.98, "unjudged", "unverified"],
  ]);
});

it("estimates the good rate of clips called good with the old residual and its interval", () => {
  expect([shippedRate([0.9, 0.8]), shippedRate([])]).toStrictEqual([
    { rate: 0.778, low: 0.61, high: 0.946 },
    null,
  ]);
});
