// Runs with bun. Offline evaluation harness for clip points (切り抜き). It runs the shipping
// `generate()` (or reads saved records), checks every record against the hard rules, measures
// whether ranges start and end on sentence boundaries, and asks a judge model from another
// family than the pipeline's to grade each clip with a fixed rubric. Model responses are cached
// by model + input, so re-running an unchanged algorithm costs nothing. Calibration grades
// synthetic negatives of real clips (an unrelated range spliced in, edges cut mid-sentence)
// that the judge must score lower before its grades are trusted.
import { createHash } from "node:crypto";
import { z } from "zod";
import {
  type Ai,
  BANDS,
  type ClipRecord,
  type Range,
  type Reader,
  readingLogit,
  type Said,
  safeJson,
} from "./clips.ts";

export type Rule = "band" | "overlap" | "replay" | "pattern";
export type Negative = "spliced" | "cut";
export type Subset = "dev" | "heldOut";
type Rubric = "topic" | "standalone" | "hook" | "ending" | "flow";
type EvalClip = ClipRecord["clips"][number];

export interface Problem {
  clip: string;
  rule: Rule;
  detail: string;
}
/** How a clip's ranges meet the stream's sentences. */
export interface Shape {
  ranges: number;
  cleanStarts: number;
  cleanEnds: number;
  /** Whether the first range, the one the viewer hears first, starts cleanly. */
  firstClean: boolean;
}
export interface Grade {
  topic: number;
  standalone: number;
  hook: number;
  ending: number;
  flow: number;
  /** Numbers of the played utterances the judge found off the clip's topic. */
  offTopic: number[];
  reason: string;
}
export interface ClipEval {
  id: string;
  kind: EvalClip["kind"];
  duration: number;
  shape: Shape;
  grade: Grade | null;
}
export interface VideoEval {
  id: string;
  subset: Subset;
  clips: ClipEval[];
  problems: Problem[];
  /** Model calls the record cost, cached or not. */
  calls: number;
}
export interface SubsetSummary {
  videos: number;
  clips: number;
  clipsPerVideo: number;
  problems: number;
  cleanStartRate: number;
  cleanEndRate: number;
  firstCleanRate: number;
  means: Record<Rubric, number>;
  /** Share of graded clips that hold one topic, stand alone, end and flow acceptably. */
  goodRate: number;
  goodPerVideo: number;
  unjudged: number;
  callsPerVideo: number;
}
export interface Store {
  read(key: string): Promise<string | null>;
  write(key: string, text: string): Promise<void>;
}
export interface Meter {
  calls: number;
  fresh: number;
}
/** A clip the old DeepSeek judge graded: its video, ranges and whether it was good. */
export interface Judged {
  video: string;
  ranges: Range[];
  good: boolean;
}
/**
 * The judged clips of the same video playing the same moment (IoU 0.5 or more): the sum of their
 * verdicts (+1 good, -1 bad) weighted by IoU squared, and how many there are.
 */
export interface Neighbours {
  signed: number;
  count: number;
}
export interface Surrogate {
  chance: number;
  verdict: "good" | "bad" | "unjudged";
  neighbours: Neighbours;
  /** How far the old labels back a verdict at this clip's length (see domainOf). */
  domain: Domain;
}
export type Domain = "verified" | "extrapolated" | "unverified";
export type Post = (
  url: string,
  init: { method: string; headers: Record<string, string>; body: string },
) => Promise<{ ok: boolean; status: number; json(): Promise<unknown> }>;
export interface Pair {
  clip: string;
  negative: Negative;
  original: Grade | null;
  altered: Grade | null;
}
export interface Calibration {
  pairs: number;
  /** Pairs the judge ranked correctly, per negative kind. */
  correct: Record<Negative, number>;
  total: Record<Negative, number>;
}
export interface Played {
  number: number;
  range: number;
  text: string;
}

/**
 * The judge: DeepSeek V4.1 Flash through OpenCode (the user allows no Cloudflare model but clairic),
 * the free tier first and the paid one when OpenCode refuses it. The earlier judge was the same model
 * on Workers AI; Ternary-Bonsai-2-9B, tried on the Mac, agreed with it only by chance (kappa 0.04).
 */
export const JUDGE_MODEL: string = "opencode/deepseek-v4.1-flash";
export const OPENCODE_CHAT: string = "https://opencode.ai/inference/openai/v1/chat/completions";
export const OPENCODE_MODELS: string[] = ["deepseek-v4.1-flash-free", "deepseek-v4.1-flash"];
/** The judge on the Mac: Ternary-Bonsai-2-9B under STRICT_JUDGE, asked under its own model name. */
export const LOCAL_JUDGE: string = "prism-ml/Ternary-Bonsai-2-9B-mlx-2bit+strict";
/**
 * Appended to the judge's instructions for Bonsai, which called 93% of 191 judged clips good
 * without it. With it Bonsai calls 52% good (DeepSeek 47%), but agrees with DeepSeek's verdicts
 * only a little beyond chance (kappa 0.10, AUC 0.61 for the axis sum): it is strict, not sharp.
 */
export const STRICT_JUDGE: string = [
  "採点は厳しく行う。各項目の平均は3になるように付け、5は欠点がまったくない場合だけ、迷ったら低い方を選ぶ。",
  "次のどれかに当てはまる項目は3以下にする: standaloneは前提（誰が・何の話か）が文字起こしにない、" +
    "endingは最後の発話がオチ・結論・反応になっていない、flowは途中から始まる・話が飛ぶ、topicは主題と無関係な発話がある。",
].join("\n");
const JUDGE_TOKENS: number = 6000;
/** Within this many seconds an edge and an utterance boundary are the same moment. */
const EDGE_SLACK: number = 0.05;
/** A silence this long ends a sentence even when the next utterance is not a clean start. */
const SENTENCE_PAUSE: number = 3;
const GOOD_TOPIC: number = 4;
const GOOD_REST: number = 3;
const SPLICE_SECONDS: number = 10;
const SPLICE_DISTANCE: number = 600;
const NEGATIVES: Negative[] = ["spliced", "cut"];
const STORIES: Record<NonNullable<EvalClip["pattern"]>, string> = {
  "climax-first": "山場→経緯",
  "hook-to-climax": "引き→山場",
};
const JUDGE_SYSTEM: string = [
  "あなたはライブ配信の切り抜き動画を審査する編集長です。切り抜きは配信のいくつかの区間を、示した順に続けて再生したものです（区間の間は編集でつながります）。",
  "文字起こしだけを根拠に、次の5項目を1〜5の整数で採点します。音声認識の誤字は減点しません。",
  "- topic: 全体が一つの話題・出来事だけを扱っているか。5=最初から最後まで同じ話題、3=主題はあるが無関係な発話が少し混じる、1=無関係な二つ以上の話題が混在する。",
  "- standalone: 配信を見ていない人がこの切り抜きだけで何の話か理解できるか。5=前提がすべて含まれる、3=少し分かりにくい、1=何の話か分からない。",
  "- hook: 最初の1〜2発話（再生開始3秒）が続きを見たくさせるか。5=強い疑問・驚き・感情の爆発、3=普通、1=あいさつ・相づち・前置きだけ。",
  "- ending: 最後が話のオチ・結論・反応で区切れているか。5=きれいにオチる、3=一応区切れる、1=文や話の途中で切れる。",
  "- flow: 発話のつながりが自然で話が飛ばないか。5=自然、3=少し飛ぶ、1=支離滅裂。区間が一つでも、文の途中から始まる・途中で終わるなら下げる。",
  "構成が示されたら、その構成どおりの区間の順番は減点しない。山場→経緯は、最初の区間が山場で、続く区間がそこに至る経緯を時系列で見せ、ループ再生で山場に戻る構成なので、endingは経緯が山場の直前まで自然につながっているかで判断する。引き→山場は、疑問や気になる起点から始まり最後の区間が山場になる構成。",
  "offTopicには主題と無関係な発話の番号を入れる（なければ空配列）。reasonは40字以内の日本語。",
  'JSONだけを返す: {"topic": 1-5, "standalone": 1-5, "hook": 1-5, "ending": 1-5, "flow": 1-5, "offTopic": [番号], "reason": "..."}',
].join("\n");

const score = z.number().int().min(1).max(5);
const gradeSchema = z.object({
  topic: score,
  standalone: score,
  hook: score,
  ending: score,
  flow: score,
  offTopic: z.array(z.number().int()),
  reason: z.string(),
});
const chatSchema = z.object({
  choices: z.array(z.object({ message: z.object({ content: z.string().nullable() }) })).min(1),
});

const round = (value: number): number => Math.round(value * 1000) / 1000;
const share = (part: number, whole: number): number => (whole === 0 ? 0 : round(part / whole));
const mean = (values: readonly number[]): number =>
  share(
    values.reduce((t, v) => t + v, 0),
    values.length,
  );
const clock = (seconds: number): string =>
  `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
const touches = (a: Range, b: Range): boolean => !(a.end < b.start || b.end < a.start);
const replays = (a: Range, b: Range): boolean =>
  a.end > b.start + EDGE_SLACK && b.end > a.start + EDGE_SLACK;
const problem = (clip: string, rule: Rule, detail: string): Problem => ({ clip, rule, detail });
const isGood = (g: Grade): boolean =>
  g.topic >= GOOD_TOPIC &&
  g.standalone >= GOOD_REST &&
  g.ending >= GOOD_REST &&
  g.flow >= GOOD_REST;
const heard = (r: Range, said: readonly Said[]): Said[] =>
  said.filter((u) => u.start >= r.start - EDGE_SLACK && u.start < r.end - EDGE_SLACK);

/** Rules every record must keep: the bands, the two stories, no shared moment between clips. */
export function invariants(record: ClipRecord): Problem[] {
  const ranges = record.clips.flatMap((c) => c.ranges.map((r) => ({ clip: c.id, ...r })));
  // A clip's own ranges may meet (a climax-first lead-up ends where the climax starts) but
  // never overlap, or that moment plays twice.
  const overlaps = ranges.flatMap((a, i) =>
    ranges
      .slice(i + 1)
      .filter((b) => (b.clip === a.clip ? replays(a, b) : touches(a, b)))
      .map((b) =>
        problem(
          a.clip,
          b.clip === a.clip ? "replay" : "overlap",
          `${a.clip} ${clock(a.start)}-${clock(a.end)} touches ${b.clip} ${clock(b.start)}-${clock(b.end)}`,
        ),
      ),
  );
  const shaped = record.clips.flatMap((c) => {
    const total = c.ranges.reduce((t, r) => t + r.end - r.start, 0);
    const [least, most] = BANDS[c.kind];
    const fits = total >= least && total <= most;
    return [
      ...(fits ? [] : [problem(c.id, "band", `${round(total)} s`)]),
      ...(c.pattern ? [] : [problem(c.id, "pattern", "no story pattern")]),
    ];
  });
  return [...shaped, ...overlaps];
}

/** A range starts cleanly on an utterance that opens a sentence or follows a silence. */
export function cleanStart(r: Range, said: readonly Said[]): boolean {
  const first = said.find((u) => u.end > r.start + EDGE_SLACK);
  const before = said.filter((u) => u.end <= r.start + EDGE_SLACK).at(-1);
  return (
    first === undefined ||
    (first.start >= r.start - EDGE_SLACK &&
      (first.opens || before === undefined || first.start - before.end >= SENTENCE_PAUSE))
  );
}

/** A range ends cleanly when its last utterance is whole and the next one starts a sentence. */
export function cleanEnd(r: Range, said: readonly Said[]): boolean {
  const last = heard(r, said).at(-1);
  const next = said.find((u) => u.start >= r.end - EDGE_SLACK);
  const whole = last === undefined || last.end <= r.end + EDGE_SLACK;
  return (
    whole &&
    (next === undefined || next.opens || next.start - (last?.end ?? r.end) >= SENTENCE_PAUSE)
  );
}

export function shapeOf(ranges: readonly Range[], said: readonly Said[]): Shape {
  const first = ranges[0];
  return {
    ranges: ranges.length,
    cleanStarts: ranges.filter((r) => cleanStart(r, said)).length,
    cleanEnds: ranges.filter((r) => cleanEnd(r, said)).length,
    firstClean: first !== undefined && cleanStart(first, said),
  };
}

/** The utterances a viewer hears, in playing order, numbered from 1. */
export function played(ranges: readonly Range[], said: readonly Said[]): Played[] {
  return ranges
    .flatMap((r, k) =>
      heard(r, said).map((u) => ({ range: k + 1, text: u.text.replace(/\n/g, "") })),
    )
    .map((p, i) => ({ number: i + 1, ...p }));
}

export function judgeRequest(
  ranges: readonly Range[],
  said: readonly Said[],
  pattern: EvalClip["pattern"],
): unknown {
  const lines = played(ranges, said);
  const story = pattern ? [`構成: ${STORIES[pattern]}`] : [];
  const transcript = ranges.map((r, k) =>
    [
      `[区間${k + 1} ${clock(r.start)}〜${clock(r.end)}]`,
      ...lines.filter((p) => p.range === k + 1).map((p) => `${p.number}: ${p.text}`),
    ].join("\n"),
  );
  return {
    messages: [
      { role: "system", content: JUDGE_SYSTEM },
      { role: "user", content: [...story, ...transcript].join("\n") },
    ],
    reasoning_effort: "low",
    max_completion_tokens: JUDGE_TOKENS,
    response_format: { type: "json_object" },
  };
}

/** The judge's grade, or null when it did not answer in the rubric's form. */
export function parseGrade(response: unknown): Grade | null {
  const chat = chatSchema.safeParse(response);
  const content = chat.success ? (chat.data.choices[0]?.message.content ?? "") : "";
  const json = content.slice(content.indexOf("{"), content.lastIndexOf("}") + 1);
  const parsed = gradeSchema.safeParse(json === "" ? null : safeJson(json));
  return parsed.success ? parsed.data : null;
}

/** The judge request with STRICT_JUDGE added to its instructions, for the local judge. */
export function strictJudge(request: unknown): unknown {
  const body = Object(request);
  const messages: unknown[] = Array.isArray(body.messages) ? body.messages : [];
  return {
    ...body,
    messages: messages.map((m, i) =>
      i === 0 ? { ...Object(m), content: `${Object(m).content}\n${STRICT_JUDGE}` } : m,
    ),
  };
}

/**
 * The judge on OpenCode: each of OPENCODE_MODELS in turn until one answers; the key travels only
 * in the Authorization header.
 */
export function openCodeAi(post: Post, key: string): Ai {
  const ask = async (input: unknown, models: readonly string[]): Promise<unknown> => {
    const [model, ...rest] = models;
    if (model === undefined) throw new Error("OpenCode answered none of the judge models");
    const response = await post(OPENCODE_CHAT, {
      method: "POST",
      headers: { Authorization: `Bearer ${key}`, "Content-Type": "application/json" },
      body: JSON.stringify({ ...Object(input), model }),
    });
    return response.ok ? response.json() : ask(input, rest);
  };
  return { run: async (_model, input) => ask(input, OPENCODE_MODELS) };
}

/**
 * The surrogate judge, all on the Mac: the local clef's rubric answers, Laya's answers to the same
 * rubric (in clairic's place, the user's choice) and the IoU-weighted verdicts of the other judged
 * clips of the same moment, fitted on the old DeepSeek grades of 191 clips (LOVO by video AUC 0.743;
 * with Cloudflare's clairic it was 0.783). Only the neighbours' verdicts count, not their number,
 * which had lifted clips with bad neighbours. Near-identical judged clips agreed only 60-75% with each
 * other, so they are evidence, not the answer. Good at 0.75 or above, bad below 0.25, no verdict
 * between: on the old clips that judged 39% of them at 79% accuracy.
 */
export const SURROGATE = {
  bias: -5.6411,
  clef: {
    topic: 0.3993,
    standalone: 0.0957,
    hook: 0.1602,
    ending: 0.88,
    flow: 0.4512,
    good: 0.8346,
    premise: -0.1867,
    startsMid: -0.0518,
    endsMid: -1.5062,
    stray: -0.1559,
  },
  reader: {
    kind: "laya",
    weights: {
      topic: 0.3829,
      standalone: 0.0464,
      hook: -0.4079,
      ending: -0.1305,
      flow: 0.2876,
      good: 2.9262,
      premise: 0.2761,
      startsMid: -0.2383,
      endsMid: -0.2399,
      stray: -0.5769,
    },
  },
  neighbours: 0.7854,
} satisfies {
  bias: number;
  clef: Record<string, number>;
  reader: Reader;
  neighbours: number;
};
export const SURROGATE_GOOD: number = 0.75;
export const SURROGATE_BAD: number = 0.25;
/**
 * Prediction-powered correction of the good rate among clips called good: on the old clips at 0.75
 * or above the judge was good 0.072 less often than the surrogate said (residual variance 0.183 over
 * 30 clips); with clairic the same correction removed the surrogate's bias in 500 video splits and its
 * 95% interval held 91% of the time. No threshold could be certified at 70-80% precision with these
 * labels (Learn Then Test, delta 0.1), so the rate is an estimate, not a guarantee.
 */
export const SHIPPED_RECTIFIER = { mean: -0.0717, variance: 0.183, count: 30 };
/**
 * How well the old labels (10-90 s clips) back the surrogate at a length, from its LOVO chances per
 * length: 15-60 s is verified (AUC 0.82 at 15-30 s over 107 clips, 0.95 at 35-60 s over 37), shorter
 * clips extrapolate from 10-15 s (AUC 0.74, where it called fewer good than the judge did, so its
 * "good" errs strict), and longer ones are unverified: at 60-90 s it ranked worse than chance (0.44
 * over 19 clips), so it gives them no verdict. Length terms did not help (AUC 0.778).
 */
export function domainOf(duration: number): Domain {
  if (duration > VERIFIED_UNTIL) return "unverified";
  return duration < VERIFIED_FROM ? "extrapolated" : "verified";
}
const VERIFIED_FROM: number = 15;
const VERIFIED_UNTIL: number = 60;
/** Below this IoU two clips are not the same moment. */
const SAME_MOMENT: number = 0.5;

const seconds = (ranges: readonly Range[]): number =>
  ranges.reduce((t, r) => t + r.end - r.start, 0);

export function overlap(a: readonly Range[], b: readonly Range[]): number {
  const shared = a.reduce(
    (t, x) =>
      t +
      b.reduce((u, y) => u + Math.max(0, Math.min(x.end, y.end) - Math.max(x.start, y.start)), 0),
    0,
  );
  return shared / (seconds(a) + seconds(b) - shared);
}

/** The judged clips of `video` playing the same moment as `ranges`, IoU-squared weighted. */
export function neighboursOf(
  video: string,
  ranges: readonly Range[],
  judged: readonly Judged[],
): Neighbours {
  const near = judged
    .filter((j) => j.video === video)
    .map((j) => ({ iou: overlap(ranges, j.ranges), sign: j.good ? 1 : -1 }))
    .filter((n) => n.iou >= SAME_MOMENT);
  return { signed: round(near.reduce((t, n) => t + n.iou ** 2 * n.sign, 0)), count: near.length };
}

/**
 * The surrogate's chance and verdict for a clip of `duration` seconds; without a usable clairic
 * reading, or at a length the old labels do not back, it gives no verdict.
 */
export function surrogateOf(
  clef: unknown,
  clairic: unknown,
  neighbours: Neighbours,
  duration: number,
): Surrogate {
  const domain = domainOf(duration);
  const logit = readingLogit(clef, clairic, SURROGATE);
  if (logit === null) return { chance: 0, verdict: "unjudged", neighbours, domain };
  const chance = round(1 / (1 + Math.exp(-(logit + SURROGATE.neighbours * neighbours.signed))));
  if (domain === "unverified") return { chance, verdict: "unjudged", neighbours, domain };
  if (chance >= SURROGATE_GOOD) return { chance, verdict: "good", neighbours, domain };
  return { chance, verdict: chance < SURROGATE_BAD ? "bad" : "unjudged", neighbours, domain };
}

/** The good rate among clips called good, PPI-corrected, with its 95% interval. */
export function shippedRate(
  chances: readonly number[],
): { rate: number; low: number; high: number } | null {
  if (chances.length === 0) return null;
  const mean = chances.reduce((t, c) => t + c, 0) / chances.length;
  const spread = chances.reduce((t, c) => t + (c - mean) ** 2, 0) / chances.length;
  const rate = mean + SHIPPED_RECTIFIER.mean;
  const half =
    1.96 *
    Math.sqrt(spread / chances.length + SHIPPED_RECTIFIER.variance / SHIPPED_RECTIFIER.count);
  return {
    rate: round(rate),
    low: round(Math.max(0, rate - half)),
    high: round(Math.min(1, rate + half)),
  };
}

/** An Ai that answers repeated requests from `store` and counts every call in `meter`. */
export function cachedAi(ai: Ai, store: Store, meter: Meter): Ai {
  return {
    async run(model: string, input: unknown): Promise<unknown> {
      const key = createHash("sha256")
        .update(`${model}\n${JSON.stringify(input)}`)
        .digest("hex");
      meter.calls += 1;
      const known = await store.read(key);
      if (known !== null) return JSON.parse(known);
      meter.fresh += 1;
      const answer = await ai.run(model, input);
      await store.write(key, JSON.stringify(answer));
      return answer;
    },
  };
}

/** Grades every clip of one record and checks it; `calls` is what the record cost. */
export async function evaluate(input: {
  ai: Ai;
  record: ClipRecord;
  said: readonly Said[];
  subset: Subset;
  calls: number;
}): Promise<VideoEval> {
  const clips = await input.record.clips.reduce<Promise<ClipEval[]>>(async (done, c) => {
    const list = await done;
    const response = await input.ai.run(JUDGE_MODEL, judgeRequest(c.ranges, input.said, c.pattern));
    list.push({
      id: c.id,
      kind: c.kind,
      duration: c.duration,
      shape: shapeOf(c.ranges, input.said),
      grade: parseGrade(response),
    });
    return list;
  }, Promise.resolve([]));
  return {
    id: input.record.videoId,
    subset: input.subset,
    clips,
    problems: invariants(input.record),
    calls: input.calls,
  };
}

export function summarize(videos: readonly VideoEval[]): SubsetSummary {
  const clips = videos.flatMap((v) => v.clips);
  const grades = clips.flatMap((c) => (c.grade ? [c.grade] : []));
  const ranges = clips.reduce((t, c) => t + c.shape.ranges, 0);
  const good = grades.filter(isGood).length;
  return {
    videos: videos.length,
    clips: clips.length,
    clipsPerVideo: share(clips.length, videos.length),
    problems: videos.reduce((t, v) => t + v.problems.length, 0),
    cleanStartRate: share(
      clips.reduce((t, c) => t + c.shape.cleanStarts, 0),
      ranges,
    ),
    cleanEndRate: share(
      clips.reduce((t, c) => t + c.shape.cleanEnds, 0),
      ranges,
    ),
    firstCleanRate: share(clips.filter((c) => c.shape.firstClean).length, clips.length),
    means: {
      topic: mean(grades.map((g) => g.topic)),
      standalone: mean(grades.map((g) => g.standalone)),
      hook: mean(grades.map((g) => g.hook)),
      ending: mean(grades.map((g) => g.ending)),
      flow: mean(grades.map((g) => g.flow)),
    },
    goodRate: share(good, grades.length),
    goodPerVideo: share(good, videos.length),
    unjudged: clips.length - grades.length,
    callsPerVideo: share(
      videos.reduce((t, v) => t + v.calls, 0),
      videos.length,
    ),
  };
}

/** The clip with a 10 s stretch from at least 10 minutes away inserted after its first range. */
export function spliced(ranges: readonly Range[], said: readonly Said[]): Range[] {
  const first = ranges[0];
  const far = said.find(
    (u) =>
      u.opens &&
      first !== undefined &&
      Math.abs(u.start - first.start) >= SPLICE_DISTANCE &&
      ranges.every((r) => u.start + SPLICE_SECONDS < r.start || u.start > r.end),
  );
  return first && far
    ? [first, { start: far.start, end: far.start + SPLICE_SECONDS }, ...ranges.slice(1)]
    : [];
}

/** The clip without each range's first and last utterance: it now starts and ends mid-story. */
export function cut(ranges: readonly Range[], said: readonly Said[]): Range[] {
  const inner = ranges.flatMap((r) => {
    const lines = heard(r, said);
    const second = lines[1];
    const penultimate = lines.at(-2);
    return second && penultimate && second.start < penultimate.end
      ? [{ start: second.start, end: penultimate.end }]
      : [];
  });
  return inner.length === ranges.length ? inner : [];
}

const ALTER: Record<Negative, (ranges: readonly Range[], said: readonly Said[]) => Range[]> = {
  spliced,
  cut,
};
/** What a correct judge ranks lower for each negative kind. */
const WORSE: Record<Negative, (original: Grade, altered: Grade) => boolean> = {
  spliced: (o, a) => a.topic < o.topic,
  cut: (o, a) => a.hook + a.ending + a.flow < o.hook + o.ending + o.flow,
};

/** Every clip graded as is and with each negative applied. */
export async function calibrate(input: {
  ai: Ai;
  clips: readonly EvalClip[];
  said: readonly Said[];
}): Promise<Pair[]> {
  const grade = async (c: EvalClip, ranges: readonly Range[]): Promise<Grade | null> =>
    parseGrade(await input.ai.run(JUDGE_MODEL, judgeRequest(ranges, input.said, c.pattern)));
  return input.clips.reduce<Promise<Pair[]>>(async (done, c) => {
    const list = await done;
    const original = await grade(c, c.ranges);
    const variants = NEGATIVES.map((negative) => ({
      negative,
      ranges: ALTER[negative](c.ranges, input.said),
    })).filter((v) => v.ranges.length > 0);
    const graded = await variants.reduce<Promise<Pair[]>>(async (made, v) => {
      const pairs = await made;
      pairs.push({
        clip: c.id,
        negative: v.negative,
        original,
        altered: await grade(c, v.ranges),
      });
      return pairs;
    }, Promise.resolve([]));
    list.push(...graded);
    return list;
  }, Promise.resolve([]));
}

export function calibration(pairs: readonly Pair[]): Calibration {
  const count = (keep: (p: Pair) => boolean): Record<Negative, number> => ({
    spliced: pairs.filter((p) => p.negative === "spliced" && keep(p)).length,
    cut: pairs.filter((p) => p.negative === "cut" && keep(p)).length,
  });
  return {
    pairs: pairs.length,
    correct: count(
      (p) => p.original !== null && p.altered !== null && WORSE[p.negative](p.original, p.altered),
    ),
    total: count((p) => p.original !== null && p.altered !== null),
  };
}

/** The comparison table of labelled runs, one row per measure and one column per run. */
export function markdown(
  runs: readonly { label: string; subsets: Record<Subset, SubsetSummary> }[],
): string {
  const rows: [string, (s: SubsetSummary) => number][] = [
    ["videos", (s) => s.videos],
    ["clips / video", (s) => s.clipsPerVideo],
    ["good clips / video", (s) => s.goodPerVideo],
    ["good rate", (s) => s.goodRate],
    ["topic", (s) => s.means.topic],
    ["standalone", (s) => s.means.standalone],
    ["hook", (s) => s.means.hook],
    ["ending", (s) => s.means.ending],
    ["flow", (s) => s.means.flow],
    ["clean starts", (s) => s.cleanStartRate],
    ["clean ends", (s) => s.cleanEndRate],
    ["first range clean", (s) => s.firstCleanRate],
    ["rule problems", (s) => s.problems],
    ["unjudged", (s) => s.unjudged],
    ["model calls / video", (s) => s.callsPerVideo],
  ];
  const subsets: Subset[] = ["dev", "heldOut"];
  const header = subsets.flatMap((k) => runs.map((r) => `${r.label} ${k}`));
  return [
    `| measure | ${header.join(" | ")} |`,
    `| --- |${header.map(() => " ---: |").join("")}`,
    ...rows.map(
      ([name, value]) =>
        `| ${name} | ${subsets.flatMap((k) => runs.map((r) => String(value(r.subsets[k])))).join(" | ")} |`,
    ),
  ].join("\n");
}
