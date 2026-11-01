// Runs with bun (tests) and in the Cloudflare Worker. Clip points (切り抜き) of one video, each
// telling one topic as one of two stories from ranges of the same stream, played back to back:
//  - climax-first: the strongest moment opens the clip, then the lead-up that led to it;
//  - hook-to-climax: a question or curious opening first, rising to the strongest moment last.
// Around each excitement peak the utterances are cut into units at clean sentence starts (the
// sync's morphological analysis marks utterances that continue a sentence) and pauses. The
// topic changes are found in two steps: noun cohesion (the same analysis's content nouns)
// proposes boundaries and @cf/cloudflare/clef confirms them, so a clip only uses the climax's
// own topic block. clef then scores every unit (interest, curiosity, same topic as the climax)
// and every opening's first 3 seconds; a clip is kept only when its opening hooks. A climax that
// stops mid-story is carried on to the unit where clef hears the story settle. Range edges
// are moved onto sentence boundaries. A second clef pass reads each assembled clip alone: one
// topic, nothing unrelated mixed in, understandable alone, a clean ending, and which of its
// utterances stray from the topic. A clip with strays loses the unit they fill or the strays at
// its range edges and is read again, up to twice. Clips of one video never overlap, and each lasts
// 10-30 s or 30-90 s in total. A clip whose last sentence hangs on a particle (the sync's closes
// hint) is dropped. clef then reads the best candidates once more with the independent judge's
// own rubric, and a logistic model fitted on judged clips turns the answers into the chance the
// judge calls the clip good; @cf/cloudflare/clairic reads the same rubric as score distributions
// and a punchline span, and both readings feed that model. Only clips at GOOD_LEAST or above
// ship. @cf/zai-org/glm-5.3-flash only titles and summarizes the shipped clips, in one request
// per video. Records are cached in R2 per movie content key (size + first/last MiB, the same key as
// replay.excitement.movie_key and the Mac viewer), so an unchanged video is never recomputed.
import { z } from "zod";
import { LANGUAGE, LEVELS as LEVEL_TEXTS, prompt } from "./prompts.ts";

/** The four lengths a clip may total (the user's bands, version 10). */
export type ClipKind = "5-10" | "15-30" | "35-60" | "90-120";
/** Kinds of records made before version 10 (10-30 s and 30-90 s), still read and shown. */
export type StoredKind = ClipKind | "short" | "long";
export type Pattern = "climax-first" | "hook-to-climax";
/** Where a record is computed: on the Mac by default, on Cloudflare only for a user's reload. */
export type Place = "mac" | "cloudflare";
export type Outcome = "written" | "skipped" | "no-movie" | "no-data" | "invalid";

export interface Range {
  start: number;
  end: number;
}
/** One utterance (cumulative captions merged) with the sync's part-of-speech hints. */
export interface Said extends Range {
  text: string;
  /** Content nouns (base forms). */
  terms: string[];
  /** False when it continues the previous utterance's sentence, so no range starts there. */
  opens: boolean;
  /** False when it ends on a particle that leaves its sentence hanging (を・て・けど・は...). */
  closes: boolean;
}
/** Consecutive utterances, cut at clean starts and pauses; the pieces a clip is built from. */
export interface Unit extends Range {
  parts: Said[];
  excitement: number;
}
/** One excitement peak's surroundings, before clef reads them. */
export interface Region {
  peak: number;
  /** The peak bin, in seconds. */
  bin: Range;
  said: Said[];
  units: Unit[];
  /** The unit of the peak bin with the most excitement. */
  climax: number;
  /** Indexes into `said` where the topic may change, least cohesive first. */
  boundaries: number[];
}
export interface Scored extends Unit {
  quality: number;
  question: number;
  /** How surely the unit is about the climax's topic (1 for the climax itself). */
  topic: number;
  /** Strength of the best opening's first 3 s (0-1), and where that opening starts. */
  hook: number;
  hookStart: number;
  /** How surely the story reaches a punchline, conclusion or reaction by the unit's end. */
  ends: number;
}
/** The climax's topic block: the units between the topic changes around it. */
export interface Block {
  peak: number;
  units: Scored[];
  /** The climax span's units merged into one. */
  climax: Scored;
  /** The climax span's own units, in time order. */
  span: Scored[];
}
export interface Draft {
  kind: ClipKind;
  pattern: Pattern;
  duration: number;
  score: number;
  ranges: Range[];
  /** Units in playing order; the first starts at its strongest opening. */
  parts: Scored[];
  /** clef's reading of the assembled clip as one coherent topic (0 before it is read). */
  coherence: number;
  /** Whether a viewer understands it alone, and whether it ends on a punchline (0 unread). */
  standalone: number;
  ending: number;
  /** The utterances clef found off the clip's topic. */
  strays: Range[];
  /** Whether the last utterance played ends its sentence (true before the clip is read). */
  closes: boolean;
}
export interface Clip {
  id: string;
  kind: ClipKind;
  pattern: Pattern;
  duration: number;
  score: number;
  ranges: Range[];
  title: string;
  summary: string;
}
export interface Ai {
  run(model: string, input: unknown): Promise<unknown>;
}
interface RangeRead {
  range: { offset: number; length: number };
}
interface StoredObject {
  json(): Promise<unknown>;
  arrayBuffer(): Promise<ArrayBuffer>;
}
export interface Bucket {
  head(key: string): Promise<{ size: number } | null>;
  get(key: string, options?: RangeRead): Promise<StoredObject | null>;
  put(key: string, value: string, options: unknown): Promise<unknown>;
  delete(key: string): Promise<unknown>;
}
export interface ClipEnv {
  MEDIA: Bucket;
}
export interface ClipRequest {
  videoId: string;
  force: boolean;
}
/** Candidates left after each stage of one generation (gated: at the gate's least; shipped: after
 * the per-kind limits and overlaps; titled: with a title, new or kept). */
interface Funnel {
  regions: number;
  stories: number;
  slate: number;
  screened: number;
  gated: number;
  /** The best gate chances, highest first. */
  best: number[];
  shipped: number;
  titled: number;
  clips: number;
}
export interface Generation {
  ai: Ai;
  place: Place;
  data: VideoData;
  movieKey: string;
  previous: ClipRecord | null;
  now: Date;
}
export interface LookupRequest {
  videoId: string;
  key: string | null;
  now: Date;
}
interface PeakAsk {
  score: readonly number[];
  apart: number;
  count: number;
  seeds: readonly number[];
}
interface Question {
  id: string;
  body: unknown;
}
interface Opening {
  start: number;
  text: string;
}
interface Lead {
  kept: Scored[];
  open: boolean;
}
interface Summary {
  title: string;
  summary: string;
}
interface RubricQuestion {
  type: "score" | "noul";
  instructions: string;
  criteria?: string[];
}
/** A clairic field: a judge score as a 1-5 distribution, or an utterance span. */
interface ClairicField {
  type: "estimate" | "text";
  min?: number;
  max?: number;
  step?: number;
  instructions: string;
}
/** An utterance after the first, with the silence before it. */
interface Step {
  i: number;
  said: Said;
  pause: number;
}
interface Candidate extends Step {
  cohesion: number;
}
/** clef's reading of an assembled clip. */
interface Reading {
  coherence: number;
  standalone: number;
  ending: number;
}
/** An utterance of a clip, with the joined range it belongs to. */
interface Line {
  range: number;
  said: Said;
}
/** A range edge being moved: where it was, and where it is now. */
interface Edge {
  from: number;
  end: number;
}
/** What one model cost a video: its calls and their input tokens. */
interface Usage {
  calls: number;
  tokens: number;
}
interface Fit {
  bias: number;
  weights: Record<string, number>;
}
/** What the gate and the search depend on per place. */
interface PlaceRules {
  /** Candidates the gate reads. */
  slate: number;
  /** Whether a region below EARLY_HOOK at its climax is left unasked. */
  early: boolean;
  /**
   * Shifts tried on clef's boundary and topic answers: each makes its own block, so a threshold a
   * slightly different clef crosses still yields the stories it would have.
   */
  shifts: number[];
  /** Transcript density's share, beside loudness, in where regions are looked for. */
  density: number;
  /** Regions looked at: the strongest bins of that mix. */
  peaks: number;
  /** The judge's chance of calling a clip good at which it ships. */
  least: number;
  /** Also ships a broadcast's `top` best candidates at `least` or above, if any (its own scale). */
  relative: { top: number; least: number } | null;
  /** Below this screen chance clef does not read the candidate. */
  floor: number;
  /** The second reading of each candidate the gate combines with clef's rubric answers, if any. */
  second: { both: Fit; reader: Reader } | null;
  clefOnly: Fit;
  /** The screen: clef-flash's rubric answers, with the second reading when there is one. */
  screen: Fit;
  screenReader: Reader | null;
}
/** clairic's reading on Cloudflare: its means per axis and its chance that no utterance lands. */
interface ClairicReader {
  kind: "clairic";
  weights: Record<string, number>;
  punchlineNull: number;
}
/** Laya's answers to the same rubric as clef (on the Mac, in clairic's place). */
interface LayaReader {
  kind: "laya";
  weights: Record<string, number>;
}
export type Reader = ClairicReader | LayaReader;

// The decision models of the three stages, each with the `model` field its requests carry. All are
// clef: on the validation videos clef-omni for the reads (1.5 good clips per video), clef-omni for
// regions and reads (3 clips in all), clef-flash for the reads (2.5) and the mean of clef-flash and
// clef-omni for the reads (2) shipped fewer good clips than clef (3).
export const REGION_MODEL: string = "@cf/cloudflare/clef";
const REGION_NAME: string = "clef";
export const READ_MODEL: string = "@cf/cloudflare/clef";
const READ_NAME: string = "clef";
export const GATE_MODEL: string = "@cf/cloudflare/clef";
const GATE_NAME: string = "clef";
/** Screens the slate before the gate; it never decides a clip ships. */
export const SCREEN_MODEL: string = "@cf/cloudflare/clef-flash";
const SCREEN_NAME: string = "clef-flash";
/** Models that answered a repeated identical request identically (128 of 128 answers). */
const STEADY: Set<string> = new Set(["@cf/cloudflare/clef"]);
export const SUMMARY_MODEL: string = "@cf/zai-org/glm-5.3-flash";
export const CLAIRIC_MODEL: string = "@cf/cloudflare/clairic";
/** clairic's stand-in on the Mac: Laya (multilingual, MLX) asked clef's rubric. */
export const LAYA_MODEL: string = "aac6fef/laya-multilingual-mlx";
/**
 * The models a Mac record names: the 4-bit MLX quant of clef answers every clef stage and
 * LFM3-1.7B writes titles and summaries, both served on 127.0.0.1 under the ids the stages ask
 * for, so no Cloudflare model runs (the user's choice).
 */
export const MAC_MODELS: { score: string; summary: string } = {
  score: "mlx-community/clef-OptiQ-4bit",
  summary: "LiquidAI/LFM3-1.7B-MLX-8bit",
};
/**
 * Bump when the algorithm or prompts change; records recompute on the next upload or reload.
 * 10: the four bands 5-10, 15-30, 35-60 and 90-120 s (the user's) replace 10-30 and 30-90 s.
 */
export const ALGORITHM: number = 10;
export const BANDS: Record<StoredKind, readonly [number, number]> = {
  "5-10": [5, 10],
  "15-30": [15, 30],
  "35-60": [35, 60],
  "90-120": [90, 120],
  short: [10, 30],
  long: [30, 90],
};
const KINDS: ClipKind[] = ["5-10", "15-30", "35-60", "90-120"];
export const MOVIE_KEY: RegExp = /^[0-9a-f]{32}$/;
const UNIT_MAX: number = 10;
/** A unit this long is cut at the next clean sentence start. */
const UNIT_TARGET: number = 5;
/** A longer silence cuts a unit, and only units this close play as one range. */
const UNIT_PAUSE: number = 3;
/** A longer silence always changes the topic. */
const PAUSE_BREAK: number = 12;
/** Loudness's own peaks, always looked at; the mix of loudness and density adds the rest. */
const PEAKS: number = 18;
/** Peaks at least this many seconds apart, so their stories differ. */
const PEAK_GAP: number = 90;
const REGION_BEFORE: number = 120;
const REGION_AFTER: number = 30;
/** Seconds of nouns compared on each side of a possible topic change. */
const COHESION_WINDOW: number = 30;
const BOUNDARIES: number = 6;
const BOUNDARY_GAP: number = 8;
const TOPIC_BREAK: number = 0.5;
const ON_TOPIC: number = 0.5;
// Below COHERENT a clip is repaired. The editor's review is the shipping gate: on 130 candidates
// read by an independent judge it passed only good clips, half of them below clef's old gates
// (coherence 0.35, ending 0.3), so clef only rules out what was never measured (below CANDIDATE).
const COHERENT: number = 0.35;
const CANDIDATE: number = 0.2;
/** Units around the peak this nearly as exciting belong to the climax, up to CLIMAX_MAX s. */
const CLIMAX_SHARE: number = 0.8;
const CLIMAX_MAX: number = 25;
/** A climax that does not settle its story takes in at most this many following seconds. */
const RESOLUTION_MAX: number = 20;
const ENDS: number = 0.5;
/** The first 3 s of a clip are what must hook the viewer. */
const HOOK_SECONDS: number = 3;
/** Openings tried per unit: its first clean utterance starts. */
const OPENINGS: number = 3;
/** The shortest first range an opening may leave. */
const OPENING_MIN: number = 3;
const HOOK_MIN: number = 0.6;
/** How much of the climax a topic question quotes. */
const QUOTE_CHARACTERS: number = 80;
/** clef accepts at most 64 questions per request. */
const QUESTIONS_PER_REQUEST: number = 64;
/** The whole-clip questions of a judge request; the rest ask about single utterances. */
const CLIP_QUESTIONS: number = 4;
const STRAY: number = 0.5;
/** Rounds of repair after the first reading of the assembled clips. */
const REPAIRS: number = 2;
const INTEREST_WEIGHT: number = 0.7;
const WEIGHTS: Record<
  "hook" | "quality" | "climax" | "coherence" | "standalone" | "ending",
  number
> = {
  hook: 0.3,
  quality: 0.15,
  climax: 0.15,
  coherence: 0.2,
  standalone: 0.1,
  ending: 0.1,
};
/** Clips per band and video at most. */
const LIMITS: Record<ClipKind, number> = { "5-10": 4, "15-30": 6, "35-60": 4, "90-120": 2 };
const KEY_SAMPLE_BYTES: number = 1 << 20;
const KEY_HEX: number = 32;
const MERGE_GAP: number = 1.5;
/** Within this many seconds a range edge and an utterance boundary are the same moment. */
const EDGE_SLACK: number = 0.05;
const PENDING_SECONDS: number = 600;
const EDITOR_TOKENS: number = 12_000;
/** Editor requests per video before a reply outside the form fails the video. */
const EDITOR_ATTEMPTS: number = 3;
/** Gate chances the funnel log line lists. */
const FUNNEL_BEST: number = 3;
/** The best candidates clef reads with the judge's rubric. */
const SLATE: number = 16;
/**
 * The judge's chance of calling a clip good at which it ships. Leave-one-video-out on 191 judged
 * clips of 30 videos, clips at 0.7 or above were 81% good (Wilson lower bound 0.66).
 */
const GOOD_LEAST: number = 0.7;
/**
 * A region whose climax request holds no opening scored this high (0-4) is not asked the rest: on
 * 288 regions of 16 videos none below it yielded a draft (the lowest region a clip shipped from
 * scored 2.17).
 */
const EARLY_HOOK: number = 1.8;
// The gate's screen: clef-flash's answers to the same rubric plus clairic's reading, fitted on the
// 191 judged clips (LOVO AUC 0.70 against clef's 0.76). Below CHEAP_FLOOR clef does not read the
// candidate; at 0.2 clef read 88% of the candidates and lost none of the 44 clips it would ship.
const CHEAP_FLOOR: number = 0.2;
const CHEAP_BIAS: number = -5.3993;
const CHEAP_WEIGHTS: Record<string, number> = {
  topic: 0.1236,
  standalone: -0.2088,
  hook: 0.0694,
  ending: 0.6864,
  flow: -0.2727,
  good: 1.248,
  premise: 0.7542,
  startsMid: -0.2422,
  endsMid: -1.6297,
  stray: -0.4449,
};
const CHEAP_CLAIRIC_WEIGHTS: Record<string, number> = {
  topic: 0.4409,
  standalone: 0.1369,
  ending: 1.2031,
  flow: -0.3174,
  hook: 0.8705,
};
const CHEAP_PUNCHLINE_NULL_WEIGHT: number = -56.9031;
// With clairic's means and its chance that no utterance lands the story (punchline), the same
// fit on the same clips: at 0.7, 84% good (Wilson 0.71) over 44 clips instead of 81% over 37.
const BOTH_BIAS: number = -6.6308;
const BOTH_WEIGHTS: Record<string, number> = {
  topic: 0.2761,
  standalone: 0.2127,
  hook: -0.0782,
  ending: 0.7665,
  flow: 0.0011,
  good: 1.5168,
  premise: -0.4705,
  startsMid: -0.2159,
  endsMid: -1.644,
  stray: -0.4369,
};
const CLAIRIC_WEIGHTS: Record<string, number> = {
  topic: 0.5426,
  standalone: 0.2029,
  ending: 0.9207,
  flow: -0.2159,
  hook: 0.7112,
};
const PUNCHLINE_NULL_WEIGHT: number = -62.6916;
// Logistic weights on the rubric answers (score levels 0-4, noul 0-1), fitted on those clips
// with L2 10 on standardized answers; ending, endsMid and good carry most of the signal.
const GOOD_BIAS: number = -2.9804;
const GOOD_WEIGHTS: Record<string, number> = {
  topic: 0.406,
  standalone: 0.2677,
  hook: 0.1763,
  ending: 0.8764,
  flow: 0.214,
  good: 2.2251,
  premise: -0.5414,
  startsMid: -0.1486,
  endsMid: -1.4432,
  stray: -0.2197,
};
// The Mac's local clef answers a little apart from Cloudflare's, so its gate is the same fit made
// on its own answers to the 191 judged clips (LOVO AUC 0.756 with clairic, 0.719 alone). The
// user lowered its floor to 0.65 (with clairic 51 clips 78% good, clef alone 47 clips 74%), and its
// calls cost nothing, so it asks every region and reads twice the candidates.
export const PLACES: Record<Place, PlaceRules> = {
  cloudflare: {
    slate: SLATE,
    early: true,
    shifts: [0],
    density: 0,
    peaks: 18,
    least: GOOD_LEAST,
    relative: null,
    floor: CHEAP_FLOOR,
    second: {
      both: { bias: BOTH_BIAS, weights: BOTH_WEIGHTS },
      reader: { kind: "clairic", weights: CLAIRIC_WEIGHTS, punchlineNull: PUNCHLINE_NULL_WEIGHT },
    },
    clefOnly: { bias: GOOD_BIAS, weights: GOOD_WEIGHTS },
    screen: { bias: CHEAP_BIAS, weights: CHEAP_WEIGHTS },
    screenReader: {
      kind: "clairic",
      weights: CHEAP_CLAIRIC_WEIGHTS,
      punchlineNull: CHEAP_PUNCHLINE_NULL_WEIGHT,
    },
  },
  mac: {
    slate: 3 * SLATE,
    early: false,
    // The 4-bit clef lands a boundary or topic answer on the other side of 0.5 often enough to lose
    // whole stories (validation video 1: v9's clip at 2649 s); these bring them back as drafts.
    shifts: [0, -0.15, -0.075, 0.075, 0.15],
    // Regions are looked for where loudness and transcript density are high together (the user's
    // 10-28 choice: neither alone). Loudness alone ranks the judged clips below chance (AUC 0.446).
    // Picked alone, the mix's peaks move regions off known good clips (at 18 it covers 95 of 109
    // judged good, loudness 104), so loudness's 18 stay and the mix adds 12 where talk is dense,
    // with a slate half again as long for the gate to read their candidates.
    density: 0.5,
    peaks: 30,
    // No second reading (the user's 10-28 redesign drops Laya, which added nothing): clef alone,
    // fit on its answers to the 191 judged clips, ranks them at LOVO AUC 0.719. Precision holds at
    // 0.74-0.81 from 0.65 to 0.8, so the middle of that plateau ships the most good clips: 0.675 keeps
    // 44 at 75% (33 good), where 0.75 kept 26 at 77% (20 good). Adding Laya, clef-flash or LFM3's
    // rubric or entertainment questions lowered the AUC (Laya alone 0.495, LFM3 questions <= 0.60).
    least: 0.675,
    // The gate's scale shifts by broadcast: at 0.675 alone 13 of the 30 judged videos ship nothing
    // (10-30: 10/24 none, 10/28 and a YouTube replay one each). A broadcast's best two at 0.55 or above
    // also ship: on the 191 judged clips 52 kept at 79% (41 good) over 20 videos, against 44 at 77%
    // (34 good) over 17. In-sample (the gate was fit on those clips), so the absolute precision is
    // optimistic; the comparison between the rules is what chose it (edits/work/finisher-20261027/
    // gate-analysis).
    relative: { top: 2, least: 0.55 },
    // At 0.25 the clef-flash screen (AUC 0.653) leaves 91% for clef and loses no shipped clip.
    floor: 0.25,
    second: null,
    clefOnly: {
      bias: -3.3042,
      weights: {
        topic: 0.3721,
        standalone: 0.1347,
        hook: 0.1529,
        ending: 0.8793,
        flow: 0.4847,
        good: 0.6623,
        premise: -0.0735,
        startsMid: 0.0239,
        endsMid: -1.5276,
        stray: 0.041,
      },
    },
    screen: {
      bias: -1.3897,
      weights: {
        topic: 0.0058,
        standalone: 0.1877,
        hook: 0.4169,
        ending: 0.8162,
        flow: 0.0041,
        good: 1.9156,
        premise: 0.8031,
        startsMid: 0.2662,
        endsMid: -1.6828,
        stray: -0.1779,
      },
    },
    screenReader: null,
  },
};
const LEVELS: string[] = LEVEL_TEXTS.interest[LANGUAGE];
const HOOK_LEVELS: string[] = LEVEL_TEXTS.hook[LANGUAGE];
/** Region question kinds answered as score levels; the rest are noul. */
const VIDEO_ID: RegExp = /^[A-Za-z0-9._-]+$/;
const DATA_KEY: RegExp = /^data\/([A-Za-z0-9._-]+)\.json$/;
const JSON_PUT: unknown = { httpMetadata: { contentType: "application/json" } };
/**
 * Writes of one stored JSON: R2 now and then fails a put with a transient internal error (10001)
 * that its documentation says to retry (10-29: 10/26's record answered 500 for one).
 */
const R2_PUT_ATTEMPTS: number = 3;
const R2_RETRY_MS: number = 200;
const STORY: Record<Pattern, string> = {
  "climax-first": prompt("editor.story.climax-first"),
  "hook-to-climax": prompt("editor.story.hook-to-climax"),
};
/** The units a repair keeps: the climax that opens climax-first; hook-to-climax's both ends. */
const KEEPS: Record<Pattern, (index: number, count: number) => boolean> = {
  "climax-first": (index) => index === 0,
  "hook-to-climax": (index, count) => index === 0 || index === count - 1,
};
const UNREAD: Reading = { coherence: 0, standalone: 0, ending: 0 };
const TRIM: Record<Pattern, (parts: readonly Scored[]) => Scored[]> = {
  "climax-first": trimLead,
  "hook-to-climax": trimMiddle,
};
// The editor's one request per video: GLM titles and summarizes the clips that ship, so a video
// costs at most one GLM call whatever its clip count, and none when nothing ships.
const EDITOR_SYSTEM: string = prompt("editor.system");
// The judge's rubric as clef questions (src/prompts.ts), word for word what the weights were fitted on.
const RUBRIC_STATE: string = prompt("rubric.state");
const RUBRIC_STORY: Record<Pattern, string> = {
  "climax-first": prompt("rubric.story.climax-first"),
  "hook-to-climax": prompt("rubric.story.hook-to-climax"),
};
/** A rubric score with its five levels: low, closer to low, mid, closer to high, high. */
const rubricScore = (id: "topic" | "standalone" | "hook" | "ending" | "flow"): RubricQuestion => {
  const low = prompt(`rubric.${id}.low`);
  const high = prompt(`rubric.${id}.high`);
  return {
    type: "score",
    instructions: prompt(`rubric.${id}`),
    criteria: [
      low,
      prompt("rubric.lean", { level: low }),
      prompt(`rubric.${id}.mid`),
      prompt("rubric.lean", { level: high }),
      high,
    ],
  };
};
const RUBRIC: Record<string, RubricQuestion> = {
  topic: rubricScore("topic"),
  standalone: rubricScore("standalone"),
  hook: rubricScore("hook"),
  ending: rubricScore("ending"),
  flow: rubricScore("flow"),
  good: { type: "noul", instructions: prompt("rubric.good") },
  premise: { type: "noul", instructions: prompt("rubric.premise") },
  startsMid: { type: "noul", instructions: prompt("rubric.startsMid") },
  endsMid: { type: "noul", instructions: prompt("rubric.endsMid") },
  stray: { type: "noul", instructions: prompt("rubric.stray") },
};
// The same rubric as clairic fields. clairic gives each score as a distribution, so its means add a
// second, differently made reading to clef's.
const CLAIRIC_STORY: Record<Pattern, string> = {
  "climax-first": prompt("clairic.story.climax-first"),
  "hook-to-climax": prompt("clairic.story.hook-to-climax"),
};
const clairicScore = (instructions: string): ClairicField => ({
  type: "estimate",
  min: 1,
  max: 5,
  step: 1,
  instructions: `${prompt("clairic.context")}${instructions}`,
});
const CLAIRIC_FIELDS: Record<string, ClairicField> = {
  topic: clairicScore(prompt("clairic.topic")),
  standalone: clairicScore(prompt("clairic.standalone")),
  hook: clairicScore(prompt("clairic.hook")),
  ending: clairicScore(prompt("clairic.ending")),
  flow: clairicScore(prompt("clairic.flow")),
  punchline: {
    type: "text",
    instructions: `${prompt("clairic.context")}${prompt("clairic.punchline")}`,
  },
  // Asked but unweighted: the fit is calibrated on answers read with this field present.
  premise: {
    type: "text",
    instructions: `${prompt("clairic.context")}${prompt("clairic.premise")}`,
  },
};
const STRAY_KINDS: string = prompt("topic.strays");
const REGION_STATE: string = prompt("region.state", { rule: prompt("topic.rule") });
const DRAFT_STATE: string = prompt("draft.state", { rule: prompt("topic.rule") });
const CLIP_ASKS: Record<"coherent" | "unmixed" | "standalone" | "ending", string> = {
  coherent: prompt("draft.coherent"),
  unmixed: prompt("draft.unmixed", { strays: STRAY_KINDS }),
  standalone: prompt("draft.standalone"),
  ending: prompt("draft.ending"),
};

const cueSchema = z.object({
  start: z.number(),
  end: z.number(),
  text: z.string(),
  speaker: z.string().optional(),
  terms: z.array(z.string()).optional(),
  opens: z.boolean().optional(),
  closes: z.boolean().optional(),
});
const seriesSchema = z.object({ score: z.array(z.number()) });
export const videoDataSchema = z.object({
  id: z.string(),
  cues: z.array(cueSchema),
  excitement: z
    .object({
      binSeconds: z.number().positive(),
      speech: seriesSchema,
      combined: seriesSchema.nullable().optional(),
    })
    .nullable()
    .optional(),
});
const rangeSchema = z.object({ start: z.number(), end: z.number() });
/**
 * The stored record's shape, which the Worker reports on every clips GET: bump it with any change to
 * clipRecordSchema so a Mac ahead of the deployed Worker stops before computing. 2: the four bands
 * (1, implied when a Worker reports none: short and long only).
 */
export const RECORD_SCHEMA: number = 2;
export const clipRecordSchema = z.object({
  version: z.number(),
  videoId: z.string(),
  movieKey: z.string(),
  inputHash: z.string(),
  models: z.object({ score: z.string(), summary: z.string() }),
  createdAt: z.string(),
  /** False when the video had no excitement scores yet, so no candidates could be chosen. */
  excitement: z.boolean().optional(),
  clips: z.array(
    z.object({
      id: z.string(),
      kind: z.enum(["5-10", "15-30", "35-60", "90-120", "short", "long"]),
      pattern: z.enum(["climax-first", "hook-to-climax"]).optional(),
      duration: z.number(),
      score: z.number(),
      ranges: z.array(rangeSchema).min(1),
      title: z.string(),
      summary: z.string(),
    }),
  ),
});
const clefSchema = z.object({
  answers: z.record(
    z.string(),
    z.union([z.object({ score: z.number() }), z.object({ noul: z.number() })]),
  ),
});
const clairicSchema = z.object({
  answers: z.object({
    topic: z.object({ mean: z.number() }),
    standalone: z.object({ mean: z.number() }),
    hook: z.object({ mean: z.number() }),
    ending: z.object({ mean: z.number() }),
    flow: z.object({ mean: z.number() }),
    punchline: z.object({ p_null: z.number() }),
  }),
});
const chatSchema = z.object({
  choices: z.array(z.object({ message: z.object({ content: z.string().nullable() }) })).min(1),
});
const editSchema = z.object({
  id: z.number(),
  title: z.string().min(1),
  summary: z.string().min(1),
});
const editorSchema = z.object({ clips: z.array(z.unknown()) });
const ownRequestSchema = z.object({ videoId: z.string(), force: z.boolean().optional() });
const eventSchema = z.object({ object: z.object({ key: z.string() }) });
const pendingSchema = z.object({ at: z.number(), force: z.boolean() });
// clef and clairic report input_tokens, chat models prompt_tokens.
const usageSchema = z.object({
  usage: z.object({ input_tokens: z.number().optional(), prompt_tokens: z.number().optional() }),
});
const pointerSchema = z.object({ movieKey: z.string().regex(MOVIE_KEY) });
const catalogSchema = z.object({
  videos: z.array(
    z.object({ id: z.string(), original: z.object({ movie: z.string() }).nullable().optional() }),
  ),
});

export type VideoData = z.infer<typeof videoDataSchema>;
export type ClipRecord = z.infer<typeof clipRecordSchema>;
type Cue = z.infer<typeof cueSchema>;
type Answers = z.infer<typeof clefSchema>["answers"];
type ClairicAnswers = z.infer<typeof clairicSchema>["answers"];
export type Lookup =
  | { status: "ready"; record: ClipRecord }
  | { status: "pending"; force: boolean }
  | { status: "mismatch" }
  | { status: "missing" };

const round = (value: number): number => Math.round(value * 1000) / 1000;
const length = (ranges: readonly Range[]): number =>
  round(ranges.reduce((total, r) => total + r.end - r.start, 0));
const pad = (value: number): string => String(value).padStart(2, "0");
const clock = (seconds: number): string =>
  `${Math.floor(seconds / 3600)}:${pad(Math.floor((seconds % 3600) / 60))}:${pad(Math.floor(seconds % 60))}`;
const byStart = (a: Range, b: Range): number => a.start - b.start;
const byScore = (a: Draft, b: Draft): number => b.score - a.score;
const descending = (a: number, b: number): number => b - a;
const earliest = (d: Draft): number => Math.min(...d.ranges.map((r) => r.start));
const byEarliest = (a: Draft, b: Draft): number => earliest(a) - earliest(b);
const byCuriosity = (a: Scored, b: Scored): number => b.question + b.hook - (a.question + a.hook);
const byWeakest = (a: Candidate, b: Candidate): number =>
  a.cohesion - b.cohesion || b.pause - a.pause;
const byNumber = (a: number, b: number): number => a - b;
const recordKey = (movieKey: string): string => `clips/${movieKey}.json`;
const pointerKey = (videoId: string): string => `clips/videos/${videoId}.json`;
const pendingKey = (videoId: string): string => `clips/pending/${videoId}.json`;
const hex = (bytes: ArrayBuffer): string =>
  [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
const scoreOf = (answers: Answers, id: string, levels: readonly string[]): number => {
  const answer = answers[id];
  return answer && "score" in answer ? answer.score / (levels.length - 1) : 0;
};
const noulOf = (answers: Answers, id: string): number => {
  const answer = answers[id];
  return answer && "noul" in answer ? answer.noul : 0;
};
const answersOf = (responses: readonly unknown[]): Answers =>
  Object.assign({}, ...responses.map((r) => clefSchema.parse(r).answers));
const needsRepair = (d: Draft): boolean => d.coherence < COHERENT || d.strays.length > 0;
const disjoint = (a: Draft, b: Draft): boolean =>
  a.ranges.every((r) => b.ranges.every((s) => r.end < s.start || s.end < r.start));

/** Same merge as the viewers: cumulative captions collapse to their final text. */
export function utterances(cues: readonly Cue[]): Said[] {
  const merged = cues.reduce<Cue[]>((out, cue) => {
    const last = out.at(-1);
    const grows =
      last !== undefined &&
      cue.start <= last.end + MERGE_GAP &&
      last.text !== "" &&
      cue.text.startsWith(last.text) &&
      cue.speaker === last.speaker;
    if (grows) {
      out.splice(-1, 1, {
        ...last,
        end: Math.max(last.end, cue.end),
        text: cue.text,
        terms: cue.terms,
        closes: cue.closes,
      });
    } else out.push({ ...cue });
    return out;
  }, []);
  return merged
    .filter((u) => u.text.trim() !== "" && u.end > u.start)
    .map((u) => ({
      start: u.start,
      end: u.end,
      text: u.text,
      terms: u.terms ?? [],
      opens: u.opens !== false,
      closes: u.closes !== false,
    }));
}

export function bandOf(duration: number): ClipKind | null {
  return KINDS.find((k) => duration >= BANDS[k][0] && duration <= BANDS[k][1]) ?? null;
}

/** `seeds`, then the strongest other bins up to `count`, all at least `apart` bins from each other. */
export function peaksOf({ score, apart, count, seeds }: PeakAsk): number[] {
  const order = [...score.entries()].sort(([a, x], [b, y]) => y - x || a - b);
  return order.reduce<number[]>(
    (chosen, [bin, value]) => {
      const far = chosen.every((other) => Math.abs(other - bin) >= apart);
      if (chosen.length < count && far && value > 0) chosen.push(bin);
      return chosen;
    },
    [...seeds],
  );
}

function steps(said: readonly Said[]): Step[] {
  return said
    .slice(1)
    .map((u, k) => ({ i: k + 1, said: u, pause: u.start - (said[k]?.end ?? u.start) }));
}

/** Utterances grouped into units: a pause, UNIT_MAX or a clean start after UNIT_TARGET cuts. */
export function tiles(said: readonly Said[]): Said[][] {
  return said.reduce<Said[][]>((groups, u) => {
    const group = groups.at(-1);
    const first = group?.[0];
    const last = group?.at(-1);
    const joins =
      first !== undefined &&
      last !== undefined &&
      u.start - last.end <= UNIT_PAUSE &&
      u.end - first.start <= UNIT_MAX &&
      !(u.opens && last.end - first.start >= UNIT_TARGET);
    if (group && joins) group.push(u);
    else groups.push([u]);
    return groups;
  }, []);
}

/** Cosine similarity of two bags of nouns. */
function cosine(a: readonly string[], b: readonly string[]): number {
  const count = (terms: readonly string[]): Map<string, number> =>
    terms.reduce((m, t) => m.set(t, (m.get(t) ?? 0) + 1), new Map<string, number>());
  const norm = (m: Map<string, number>): number =>
    Math.sqrt([...m.values()].reduce((s, n) => s + n * n, 0));
  const x = count(a);
  const y = count(b);
  const dot = [...x].reduce((s, [t, n]) => s + n * (y.get(t) ?? 0), 0);
  const scale = norm(x) * norm(y);
  return scale === 0 ? 0 : dot / scale;
}

/** How much the nouns of the window before `at` share with the window from it on. */
export function cohesion(said: readonly Said[], at: number): number {
  const terms = (from: number, to: number): string[] =>
    said.filter((u) => u.start >= from && u.start < to).flatMap((u) => u.terms);
  return cosine(terms(at - COHESION_WINDOW, at), terms(at, at + COHESION_WINDOW));
}

/** The least cohesive clean starts outside the climax, BOUNDARY_GAP apart, in time order. */
export function boundariesOf(said: readonly Said[], climax: Range): number[] {
  const ranked = steps(said)
    .filter((step) => step.said.opens && !inside(step.said, climax))
    .map((step) => ({ ...step, cohesion: cohesion(said, step.said.start) }))
    .sort(byWeakest);
  return ranked
    .reduce<Candidate[]>((chosen, c) => {
      const far = chosen.every((o) => Math.abs(o.said.start - c.said.start) >= BOUNDARY_GAP);
      if (chosen.length < BOUNDARIES && far) chosen.push(c);
      return chosen;
    }, [])
    .map((c) => c.i)
    .sort(byNumber);
}

function inside(u: Said, range: Range): boolean {
  return u.start > range.start && u.start < range.end;
}

/** The peak bin's unit with the most excitement, or -1 when nobody speaks in the bin. */
function climaxOf(units: readonly Unit[], bin: Range): number {
  return units.reduce(
    (best, u, i) =>
      u.end > bin.start && u.start < bin.end && u.excitement > (units[best]?.excitement ?? -1)
        ? i
        : best,
    -1,
  );
}

/**
 * Each bin's transcript characters per second, summed with the bins beside it so one long
 * utterance does not make a peak alone.
 */
export function densityOf(said: readonly Said[], width: number, bins: number): number[] {
  const own = said.reduce(
    (chars, u) => {
      const bin = Math.min(bins - 1, Math.floor(u.start / width));
      chars[bin] = (chars[bin] ?? 0) + u.text.replace(/\n/g, "").length / width;
      return chars;
    },
    Array.from({ length: bins }, () => 0),
  );
  return own.map((c, bin) => (own[bin - 1] ?? 0) + c + (own[bin + 1] ?? 0));
}

/**
 * Where to look for clips: loudness and transcript density, each over its stream's maximum, mixed
 * with `share` the share of density (0 keeps loudness's own peaks).
 */
export function selectorOf(
  score: readonly number[],
  density: readonly number[],
  share: number,
): number[] {
  const loudest = Math.max(0, ...score);
  const densest = Math.max(0, ...density);
  return score.map(
    (s, bin) =>
      (1 - share) * (loudest > 0 ? s / loudest : 0) +
      share * (densest > 0 ? (density[bin] ?? 0) / densest : 0),
  );
}

/** Every peak's surroundings, cut into units, with the possible topic changes around it. */
export function regionsOf(data: VideoData, rules: Pick<PlaceRules, "density" | "peaks">): Region[] {
  const excitement = data.excitement;
  if (!excitement) return [];
  const score = excitement.combined?.score ?? excitement.speech.score;
  const width = excitement.binSeconds;
  const level = (range: Range): number =>
    Math.max(0, ...score.slice(Math.floor(range.start / width), Math.floor(range.end / width) + 1));
  const said = utterances(data.cues);
  const apart = Math.ceil(PEAK_GAP / width);
  const loudest = peaksOf({ score, apart, count: PEAKS, seeds: [] });
  const selector = selectorOf(score, densityOf(said, width, score.length), rules.density);
  return peaksOf({ score: selector, apart, count: rules.peaks, seeds: loudest }).flatMap((peak) => {
    const bin = { start: peak * width, end: (peak + 1) * width };
    const local = said.filter(
      (u) => u.start >= bin.start - REGION_BEFORE && u.end <= bin.end + REGION_AFTER,
    );
    const units = tiles(local).map((parts) => {
      const span = {
        start: Math.min(...parts.map((u) => u.start)),
        end: Math.max(...parts.map((u) => u.end)),
      };
      return { ...span, parts, excitement: level(span) };
    });
    const climax = climaxOf(units, bin);
    const unit = units[climax];
    return unit && units.length > 1
      ? [{ peak, bin, said: local, units, climax, boundaries: boundariesOf(local, unit) }]
      : [];
  });
}

/** The openings a unit may start at: its first clean starts, each leaving OPENING_MIN+ s. */
export function openings(unit: Unit): Opening[] {
  return unit.parts
    .slice(0, OPENINGS)
    .filter((p) => p.opens && unit.end - p.start >= OPENING_MIN)
    .map((p) => ({
      start: p.start,
      text: unit.parts
        .filter((q) => q.start >= p.start && q.start < p.start + HOOK_SECONDS)
        .map((q) => q.text)
        .join(" "),
    }));
}

/** Topic changes to confirm, then every unit's interest, curiosity, topic and openings. */
export function questions(region: Region): Question[] {
  const quote = (region.units[region.climax]?.parts ?? [])
    .map((u) => u.text.replace(/\n/g, ""))
    .join(" ")
    .slice(0, QUOTE_CHARACTERS);
  const changes = region.boundaries.map((j) => ({
    id: `boundary.${j}`,
    body: {
      type: "noul",
      instructions: prompt("region.boundary", { previous: j - 1, utterance: j }),
    },
  }));
  const units = region.units.flatMap((u, i) => [
    {
      id: `clip.${i}`,
      body: {
        type: "score",
        instructions: prompt("region.interest", { unit: i }),
        criteria: LEVELS,
      },
    },
    {
      id: `question.${i}`,
      body: { type: "noul", instructions: prompt("region.opener", { unit: i }) },
    },
    {
      id: `ends.${i}`,
      body: { type: "noul", instructions: prompt("region.settles", { unit: i }) },
    },
    ...(i === region.climax
      ? []
      : [
          {
            id: `topic.${i}`,
            body: {
              type: "noul",
              instructions: prompt("region.topic", {
                unit: i,
                climax: region.climax,
                quote,
                strays: STRAY_KINDS,
              }),
            },
          },
        ]),
    ...openings(u).map((opening, k) => ({
      id: `hook.${i}.${k}`,
      body: {
        type: "score",
        instructions: prompt("region.hook", {
          unit: i,
          seconds: HOOK_SECONDS,
          opening: opening.text,
        }),
        criteria: HOOK_LEVELS,
      },
    })),
  ]);
  return [...changes, ...units];
}

/** Requests of at most QUESTIONS_PER_REQUEST questions; each carries the same state. */
function chunked(state: unknown, all: readonly Question[]): unknown[] {
  return Array.from({ length: Math.ceil(all.length / QUESTIONS_PER_REQUEST) }, (_, n) => ({
    model: REGION_NAME,
    state,
    questions: Object.fromEntries(
      all
        .slice(n * QUESTIONS_PER_REQUEST, (n + 1) * QUESTIONS_PER_REQUEST)
        .map((q) => [q.id, q.body]),
    ),
  }));
}

export function clefRequests(region: Region): unknown[] {
  const unitOf = (u: Said): number => region.units.findIndex((v) => v.parts.includes(u));
  return chunked(
    {
      description: REGION_STATE,
      climax: region.climax,
      utterances: region.said.map((u, j) => ({
        id: j,
        unit: unitOf(u),
        time: clock(u.start),
        text: u.text,
      })),
      units: region.units.map((u, i) => ({
        id: i,
        time: `${clock(u.start)}〜${clock(u.end)}`,
        excitement: Math.round(u.excitement * 100) / 100,
      })),
    },
    questions(region),
  );
}

/** The climax's topic block, its units scored; null when the climax itself is cut apart. */
export function blockOf(region: Region, responses: readonly unknown[]): Block | null {
  const answers = answersOf(responses);
  const confirmed = region.boundaries.filter(
    (j) => noulOf(answers, `boundary.${j}`) >= TOPIC_BREAK,
  );
  const breaks = [
    ...region.said.filter((_, j) => confirmed.includes(j)).map((u) => u.start),
    ...steps(region.said)
      .filter((step) => step.pause > PAUSE_BREAK)
      .map((step) => step.said.start),
  ];
  const nouns = (u: Unit | undefined): string[] => (u?.parts ?? []).flatMap((p) => p.terms);
  const climaxNouns = nouns(region.units[region.climax]);
  const scored = region.units.map((u, i) => {
    const hooks = openings(u).map((opening, k) => ({
      start: opening.start,
      hook: scoreOf(answers, `hook.${i}.${k}`, HOOK_LEVELS),
    }));
    const best = hooks.reduce((b, h) => (h.hook > b.hook ? h : b), { start: u.start, hook: 0 });
    return {
      ...u,
      quality: round(
        INTEREST_WEIGHT * scoreOf(answers, `clip.${i}`, LEVELS) +
          (1 - INTEREST_WEIGHT) * u.excitement,
      ),
      question: round(noulOf(answers, `question.${i}`)),
      // Shared nouns are evidence of one topic that clef can miss in a long transcript.
      topic:
        i === region.climax
          ? 1
          : round(Math.max(noulOf(answers, `topic.${i}`), cosine(nouns(u), climaxNouns))),
      hook: round(best.hook),
      hookStart: best.start,
      ends: round(noulOf(answers, `ends.${i}`)),
    };
  });
  const peak = scored[region.climax];
  if (!peak) return null;
  const from = Math.max(-Infinity, ...breaks.filter((t) => t <= peak.start));
  const to = Math.min(Infinity, ...breaks.filter((t) => t >= peak.end));
  const units = scored.filter((u) => u.start >= from && u.end <= to);
  // The story's climax: the most interesting on-topic unit from the peak on (often the punchline).
  const top = units
    .filter((u) => u.topic >= ON_TOPIC && u.end > region.bin.start)
    .reduce((best, u) => (u.quality > best.quality ? u : best), peak);
  const span = resolved(climaxSpan(units, top), units);
  const climax = merged(top, span);
  return {
    peak: region.peak,
    climax,
    span,
    units: [...units.filter((u) => !span.includes(u)), climax].sort(byStart),
  };
}

/** The units, in order, that `edge` accepts one after another until it first refuses. */
function adjoining(
  sequence: readonly Scored[],
  edge: (u: Scored, kept: Scored[]) => boolean,
): Scored[] {
  return sequence.reduce<Lead>(
    (lead, u) => {
      const fits = lead.open && edge(u, lead.kept);
      if (fits) lead.kept.push(u);
      return { kept: lead.kept, open: fits };
    },
    { kept: [], open: true },
  ).kept;
}

/** The peak unit with the on-topic units around it that stay nearly as exciting. */
export function climaxSpan(units: readonly Scored[], peak: Scored): Scored[] {
  const near = (u: Scored): boolean =>
    u.topic >= ON_TOPIC && u.excitement >= CLIMAX_SHARE * peak.excitement;
  const after = adjoining(
    units.filter((u) => u.start >= peak.end),
    (u, kept) =>
      near(u) &&
      u.start - (kept.at(-1) ?? peak).end <= UNIT_PAUSE &&
      u.end - peak.start <= CLIMAX_MAX,
  );
  const end = (after.at(-1) ?? peak).end;
  const before = adjoining(
    units.filter((u) => u.end <= peak.start).reverse(),
    (u, kept) =>
      near(u) && (kept.at(-1) ?? peak).start - u.end <= UNIT_PAUSE && end - u.start <= CLIMAX_MAX,
  );
  return [...before.reverse(), peak, ...after];
}

/** Units played as one: the first one's opening, the strongest excitement. */
function merged(single: Scored, span: readonly Scored[]): Scored {
  const first = span[0];
  const last = span.at(-1);
  return first && last && span.length > 1
    ? {
        start: first.start,
        end: last.end,
        parts: span.flatMap((u) => u.parts),
        excitement: Math.max(...span.map((u) => u.excitement)),
        quality: round(span.reduce((t, u) => t + u.quality, 0) / span.length),
        question: first.question,
        topic: 1,
        hook: first.hook,
        hookStart: first.hookStart,
        ends: last.ends,
      }
    : single;
}

/**
 * The climax span with the on-topic units that follow it up to the one that settles its
 * story, when the span itself stops before the punchline; unchanged when nothing settles it.
 */
export function resolved(span: readonly Scored[], units: readonly Scored[]): Scored[] {
  const last = span.at(-1);
  const settles = (u: Scored | undefined): boolean => (u?.ends ?? 0) >= ENDS;
  const rest = last
    ? adjoining(
        units.filter((u) => u.start >= last.end),
        (u, kept) =>
          !settles(kept.at(-1)) &&
          u.topic >= ON_TOPIC &&
          u.start - (kept.at(-1) ?? last).end <= UNIT_PAUSE &&
          u.end - last.end <= RESOLUTION_MAX,
      )
    : [];
  return settles(last) || !settles(rest.at(-1)) ? [...span] : [...span, ...rest];
}

/**
 * Units in playing order become ranges; a unit right after the previous one, or starting inside
 * it (overlapping captions), extends it, so no moment of a clip plays twice.
 */
export function rangesOf(parts: readonly Scored[]): Range[] {
  return parts.reduce<Range[]>((ranges, p, i) => {
    const last = ranges.at(-1);
    const start = i === 0 ? p.hookStart : p.start;
    if (last && start > last.start && start - last.end <= UNIT_PAUSE) {
      last.end = Math.max(last.end, p.end);
    } else ranges.push({ start, end: p.end });
    return ranges;
  }, []);
}

function scoreOfDraft(parts: readonly Scored[], reading: Reading): number {
  const climax = parts.reduce((c, p) => (p.excitement > c.excitement ? p : c));
  const quality = parts.reduce((t, p) => t + p.quality, 0) / parts.length;
  return round(
    WEIGHTS.hook * parts.slice(0, 1).reduce((t, p) => t + p.hook, 0) +
      WEIGHTS.quality * quality +
      WEIGHTS.climax * climax.excitement +
      WEIGHTS.coherence * reading.coherence +
      WEIGHTS.standalone * reading.standalone +
      WEIGHTS.ending * reading.ending,
  );
}

export function draftOf(pattern: Pattern, parts: readonly Scored[]): Draft[] {
  return shaped(pattern, parts, rangesOf(parts));
}

/** The draft of `parts` played as `ranges`, unread, when it is a story that fits a band. */
function shaped(pattern: Pattern, parts: readonly Scored[], ranges: Range[]): Draft[] {
  const duration = round(length(ranges));
  const kind = bandOf(duration);
  // A 5-10 s clip may be the climax alone; longer ones tell a story of two or more units.
  return kind && (parts.length > 1 || kind === "5-10")
    ? [
        {
          kind,
          pattern,
          duration,
          ranges,
          parts: [...parts],
          ...UNREAD,
          strays: [],
          closes: true,
          score: scoreOfDraft(parts, UNREAD),
        },
      ]
    : [];
}

/**
 * Range edges on sentence boundaries: a range that starts on an utterance continuing the
 * previous sentence starts with that sentence, and one followed closely by the rest of its
 * last sentence takes it in, as long as the clip stays in its band and its ranges apart.
 */
export function closed(draft: Draft, said: readonly Said[]): Draft {
  const ranges = draft.ranges.map((r, i) => {
    const others = draft.ranges.filter((_, k) => k !== i);
    const from = i === 0 ? r.start : sentenceStart(r.start, said);
    const to = sentenceEnd(r.end, said);
    const apart = others.every((o) => o.end < from || to < o.start);
    return apart ? { start: from, end: to } : r;
  });
  const duration = round(length(ranges));
  return bandOf(duration) === draft.kind ? { ...draft, ranges, duration } : draft;
}

function sentenceStart(start: number, said: readonly Said[]): number {
  const at = said.findIndex((u) => u.start >= start - EDGE_SLACK);
  return said[at]?.opens === false ? (opening(said, at, start) ?? start) : start;
}

/** The utterance that opens the sentence `said[at]` continues, unless a pause comes first. */
function opening(said: readonly Said[], at: number, from: number): number | null {
  const u = said[at];
  const previous = said[at - 1];
  const reachable =
    u !== undefined &&
    previous !== undefined &&
    u.start - previous.end <= UNIT_PAUSE &&
    from - previous.start <= UNIT_MAX;
  if (!reachable) return null;
  return previous.opens ? previous.start : opening(said, at - 1, from);
}

function sentenceEnd(end: number, said: readonly Said[]): number {
  // Overlapping captions (several speakers, auto captions) can leave the last heard utterance
  // running past the range; it plays to its end.
  const running = said.filter(
    (u) => u.start < end - EDGE_SLACK && u.end > end && u.end - end <= UNIT_MAX,
  );
  const whole = Math.max(end, ...running.map((u) => u.end));
  return continued(
    said,
    said.findIndex((u) => u.start >= whole - EDGE_SLACK),
    { from: whole, end: whole },
  );
}

/** The end of the utterances from `said[at]` on that continue the sentence ending at `end`. */
function continued(said: readonly Said[], at: number, edge: Edge): number {
  const u = said[at];
  const continues =
    u !== undefined &&
    !u.opens &&
    u.start - edge.end <= UNIT_PAUSE &&
    u.end - edge.from <= UNIT_MAX;
  return continues ? continued(said, at + 1, { from: edge.from, end: u.end }) : edge.end;
}

/** The on-topic units right before the climax, without a gap, while the clip fits `cap`. */
function leadOf(block: Block, cap: number): Scored[] {
  const head = block.climax.end - block.climax.hookStart;
  return adjoining(
    block.units.filter((u) => u.end <= block.climax.start).reverse(),
    (u, kept) =>
      u.topic >= ON_TOPIC &&
      (kept.at(-1) ?? block.climax).start - u.end <= UNIT_PAUSE &&
      head + length(kept) + u.end - u.start <= cap,
  ).reverse();
}

/** Climax-first: the climax opens (it must hook), then its unbroken lead-up in time order. */
function climaxFirst(block: Block, cap: number): Draft[] {
  return block.climax.hook >= HOOK_MIN
    ? draftOf("climax-first", [block.climax, ...leadOf(block, cap)])
    : [];
}

/** Hook-to-climax: the most curious hooking unit opens, the on-topic units on to the climax. */
function hookToClimax(block: Block, cap: number): Draft[] {
  const climax = block.climax;
  const openers = block.units.filter(
    (u) => u.end <= climax.start && u.hook >= HOOK_MIN && u.topic >= ON_TOPIC,
  );
  const stories = openers
    .sort(byCuriosity)
    .flatMap((opener) =>
      draftOf("hook-to-climax", [
        opener,
        ...block.units.filter(
          (u) => u.start > opener.start && u.end <= climax.start && u.topic >= ON_TOPIC,
        ),
        climax,
      ]).filter((d) => d.duration <= cap),
    );
  // A climax span that opens on a hook tells the story alone: the curious start, then the
  // reactions, all at the peak's excitement.
  const inside = block.span.slice(0, 1).every((u) => u.hook >= HOOK_MIN)
    ? draftOf("hook-to-climax", block.span).filter((d) => d.duration <= cap)
    : [];
  return [...stories.slice(0, 1), ...inside];
}

/** Both stories of a topic block, each cut to every band's length where it fits. */
export function storiesOf(block: Block): Draft[] {
  // Longest band first, so a story that fits several keeps its fullest cut first.
  const all = KINDS.map((k) => BANDS[k][1])
    .reverse()
    .flatMap((cap) => [...climaxFirst(block, cap), ...hookToClimax(block, cap)]);
  return [...new Map(all.map((d) => [JSON.stringify(d.ranges), d])).values()];
}

/** Ranges in time order, touching ones joined: how the moments happened in the stream. */
export function chronological(ranges: readonly Range[]): Range[] {
  return [...ranges].sort(byStart).reduce<Range[]>((out, r) => {
    const last = out.at(-1);
    if (last && r.start - last.end <= UNIT_PAUSE) last.end = Math.max(last.end, r.end);
    else out.push({ ...r });
    return out;
  }, []);
}

/** The draft's utterances in time order, with the joined range each one belongs to. */
function linesOf(draft: Draft, said: readonly Said[]): Line[] {
  return chronological(draft.ranges).flatMap((r, range) =>
    said
      .filter((u) => u.start >= r.start - EDGE_SLACK && u.start < r.end - EDGE_SLACK)
      .map((u) => ({ range, said: u })),
  );
}

/**
 * One request per assembled clip, its utterances in time order: the story's reordering is
 * deliberate, and clef reads a clip alone more steadily than beside other clips. Four questions
 * read the whole clip; the rest ask whether each utterance strays from its topic.
 */
export function judgeRequests(drafts: readonly Draft[], said: readonly Said[]): unknown[] {
  return drafts.map((d) => {
    const lines = linesOf(d, said);
    return {
      model: READ_NAME,
      state: {
        description: DRAFT_STATE,
        utterances: lines.map((l, id) => ({
          id,
          range: l.range,
          time: clock(l.said.start),
          text: l.said.text,
        })),
      },
      questions: {
        ...Object.fromEntries(
          Object.entries(CLIP_ASKS).map(([id, instructions]) => [
            id,
            { type: "noul", instructions },
          ]),
        ),
        ...Object.fromEntries(
          lines.slice(0, QUESTIONS_PER_REQUEST - CLIP_QUESTIONS).map((_, k) => [
            `stray.${k}`,
            {
              type: "noul",
              instructions: prompt("draft.stray", { utterance: k, strays: STRAY_KINDS }),
            },
          ]),
        ),
      },
    };
  });
}

export function judged(
  drafts: readonly Draft[],
  responses: readonly unknown[],
  said: readonly Said[],
): Draft[] {
  return drafts.map((d, k) => {
    const answers = answersOf(responses.slice(k, k + 1));
    const reading: Reading = {
      coherence: round((noulOf(answers, "coherent") + noulOf(answers, "unmixed")) / 2),
      standalone: round(noulOf(answers, "standalone")),
      ending: round(noulOf(answers, "ending")),
    };
    const strays = linesOf(d, said)
      .filter((_, i) => noulOf(answers, `stray.${i}`) >= STRAY)
      .map((l) => ({ start: l.said.start, end: l.said.end }));
    const last = d.ranges.at(-1);
    const closes =
      said
        .filter(
          (u) => last && u.start >= last.start - EDGE_SLACK && u.start < last.end - EDGE_SLACK,
        )
        .at(-1)?.closes !== false;
    return { ...d, ...reading, strays, closes, score: scoreOfDraft(d.parts, reading) };
  });
}

/** Climax-first loses its earliest lead-up unit, the furthest from the climax. */
function trimLead(parts: readonly Scored[]): Scored[] {
  return parts.filter((_, i) => i !== 1);
}

/** Hook-to-climax loses its least on-topic middle unit. */
function trimMiddle(parts: readonly Scored[]): Scored[] {
  const middle = parts.slice(1, -1);
  const weakest = middle.reduce<Scored | null>((w, p) => (w && w.topic <= p.topic ? w : p), null);
  return weakest ? parts.filter((p) => p !== weakest) : [];
}

/**
 * A clip to read again without what clef found off its topic: the units mostly made of strays
 * (never the climax or the opening) and the strays at its range ends. Without located strays,
 * an incoherent clip loses its weakest unit instead.
 */
export function adjusted(draft: Draft): Draft[] {
  const stray = (u: Said): boolean =>
    draft.strays.some((s) => s.start === u.start && s.end === u.end);
  const keeps = KEEPS[draft.pattern];
  const parts = draft.parts.filter(
    (p, i) => keeps(i, draft.parts.length) || p.parts.filter(stray).length * 2 <= p.parts.length,
  );
  const ranges = rangesOf(parts).flatMap((r, i) => untailed(r, parts, stray, i === 0));
  const same = JSON.stringify(ranges) === JSON.stringify(draft.ranges);
  return draft.strays.length === 0
    ? draftOf(draft.pattern, TRIM[draft.pattern](draft.parts))
    : same
      ? []
      : shaped(draft.pattern, parts, ranges);
}

/** The range without the strays at its end, and at its start unless it opens the clip. */
function untailed(
  range: Range,
  parts: readonly Scored[],
  stray: (u: Said) => boolean,
  first: boolean,
): Range[] {
  const lines = parts
    .flatMap((p) => p.parts)
    .filter((u) => u.start >= range.start - EDGE_SLACK && u.start < range.end - EDGE_SLACK)
    .sort(byStart);
  const kept = lines.filter((u) => !stray(u));
  const head = first ? lines[0] : kept.find((u) => u.opens);
  const tail = kept.at(-1);
  return head && tail && head.start < tail.end
    ? [{ start: first ? range.start : head.start, end: tail.end }]
    : [];
}

/**
 * The clips per band the judge most likely calls good, each at GOOD_LEAST or above. Clips of one
 * video never share a moment.
 */
export function pick(
  drafts: readonly Draft[],
  good: (draft: Draft) => number,
  least: number,
): Draft[] {
  return drafts
    .filter((d) => good(d) >= least)
    .sort((a, b) => good(b) - good(a))
    .reduce<Draft[]>((kept, d) => {
      const room = kept.filter((k) => k.kind === d.kind).length < LIMITS[d.kind];
      if (room && kept.every((k) => disjoint(k, d))) kept.push(d);
      return kept;
    }, [])
    .sort(byEarliest);
}

/** The candidates worth the rubric reading: measured coherence, a closing sentence, best first. */
export function slateOf(drafts: readonly Draft[], size: number): Draft[] {
  return drafts
    .filter((d) => d.coherence >= CANDIDATE && d.closes)
    .sort(byScore)
    .slice(0, size);
}

/** A decision model's reading of one candidate as played, with the judge's rubric. */
export function rubricRequest(
  clip: Pick<Draft, "pattern" | "ranges">,
  said: readonly Said[],
  model: string,
): unknown {
  return {
    model,
    state: {
      description: RUBRIC_STATE,
      story: RUBRIC_STORY[clip.pattern],
      utterances: clip.ranges.flatMap((r, k) =>
        said
          .filter((u) => u.start >= r.start - EDGE_SLACK && u.start < r.end - EDGE_SLACK)
          .map((u) => ({ range: k + 1, text: u.text.replace(/\n/g, "") })),
      ),
    },
    questions: RUBRIC,
  };
}

/** clairic's reading of one candidate as played: one line per utterance, marked with its range. */
export function clairicRequest(
  clip: Pick<Draft, "pattern" | "ranges">,
  said: readonly Said[],
): unknown {
  const story = CLAIRIC_STORY[clip.pattern];
  return {
    model: "clairic",
    state: clip.ranges
      .flatMap((r, k) =>
        said
          .filter((u) => u.start >= r.start - EDGE_SLACK && u.start < r.end - EDGE_SLACK)
          .map((u) => prompt("clairic.line", { range: k + 1, text: u.text.replace(/\n/g, "") })),
      )
      .join("\n"),
    fields: Object.fromEntries(
      Object.entries(CLAIRIC_FIELDS).map(([id, f]) => [
        id,
        { ...f, instructions: `${story}${f.instructions}` },
      ]),
    ),
  };
}

const weighed = (answers: Answers, weights: Record<string, number>, bias: number): number =>
  Object.entries(weights).reduce((total, [id, weight]) => {
    const answer = answers[id];
    return total + weight * (answer ? ("score" in answer ? answer.score : answer.noul) : 0);
  }, bias);

const clairicLogit = (
  a: ClairicAnswers,
  weights: Record<string, number>,
  punchlineNull: number,
): number => {
  const means: Record<string, number> = {
    topic: a.topic.mean,
    standalone: a.standalone.mean,
    hook: a.hook.mean,
    ending: a.ending.mean,
    flow: a.flow.mean,
  };
  return Object.entries(weights).reduce(
    (total, [id, weight]) => total + weight * (means[id] ?? 0),
    punchlineNull * a.punchline.p_null,
  );
};

/**
 * The judge's chance of calling the clip good, from clef's rubric answers and clairic's reading.
 * Without a usable clairic reading (a preview model) it falls back to the clef-only fit.
 */
/** The second reading's share of a logit, or null when the reply is not in its form. */
function readerLogit(reading: unknown, reader: Reader): number | null {
  if (reader.kind === "laya") {
    return clefSchema.safeParse(reading).success
      ? weighed(answersOf([reading]), reader.weights, 0)
      : null;
  }
  const read = clairicSchema.safeParse(reading);
  return read.success
    ? clairicLogit(read.data.answers, reader.weights, reader.punchlineNull)
    : null;
}

/**
 * A logistic model's logit over clef's rubric answers and a second reading (clairic or Laya), or
 * null without a usable second reading.
 */
export function readingLogit(
  response: unknown,
  reading: unknown,
  fit: { bias: number; clef: Record<string, number>; reader: Reader },
): number | null {
  const second = readerLogit(reading, fit.reader);
  return second === null ? null : weighed(answersOf([response]), fit.clef, fit.bias) + second;
}

/** The judge's chance of calling the clip good, from clef's rubric answers and the second reading. */
export function goodOf(response: unknown, reading: unknown, place: Place): number {
  const rules = PLACES[place];
  const second = rules.second;
  const both = second
    ? readingLogit(response, reading, {
        ...second.both,
        clef: second.both.weights,
        reader: second.reader,
      })
    : null;
  if (both === null && second && reading != null)
    console.warn(`${second.reader.kind} reply outside the form, using the clef-only fit`);
  const logit = both ?? weighed(answersOf([response]), rules.clefOnly.weights, rules.clefOnly.bias);
  return round(1 / (1 + Math.exp(-logit)));
}

/**
 * The screen's chance the judge calls the clip good, from clef-flash's rubric answers and the second
 * reading; without a usable second reading it screens nothing out, so clef reads the candidate.
 */
export function cheapOf(response: unknown, reading: unknown, place: Place): number {
  const rules = PLACES[place];
  const logit = rules.screenReader
    ? readingLogit(response, reading, {
        ...rules.screen,
        clef: rules.screen.weights,
        reader: rules.screenReader,
      })
    : weighed(answersOf([response]), rules.screen.weights, rules.screen.bias);
  return round(1 / (1 + Math.exp(-(logit ?? Number.POSITIVE_INFINITY))));
}

/**
 * The editor's one request: every candidate's utterances as played, under its story. Greedy
 * sampling with a fixed seed per attempt makes a reload of an unchanged video pick the same clips.
 */
export function editorRequest(
  clips: readonly Draft[],
  said: readonly Said[],
  seed: number,
): unknown {
  const candidates = clips.map((clip, k) =>
    [
      prompt("editor.candidate", {
        number: k + 1,
        seconds: clip.duration,
        story: STORY[clip.pattern],
      }),
      ...clip.ranges.flatMap((r, n) => [
        prompt("editor.range", { range: n + 1 }),
        ...said
          .filter((u) => u.start >= r.start - EDGE_SLACK && u.start < r.end - EDGE_SLACK)
          .map((u) => u.text.replace(/\n/g, "")),
      ]),
    ].join("\n"),
  );
  return {
    messages: [
      { role: "system", content: EDITOR_SYSTEM },
      { role: "user", content: candidates.join("\n\n") },
    ],
    reasoning_effort: "low",
    temperature: 0,
    seed,
    max_completion_tokens: EDITOR_TOKENS,
    response_format: { type: "json_object" },
  };
}

/**
 * The editor's verdict per candidate, by its index in the request; a candidate it left out or
 * answered outside the form is not there. A reply that is not the form at all throws, so the
 * queue retries the video.
 */
export function editsOf(response: unknown, count: number): Map<number, Summary> {
  const content = chatSchema.parse(response).choices[0]?.message.content ?? "";
  const reply = editorSchema.parse(
    safeJson(content.slice(content.indexOf("{"), content.lastIndexOf("}") + 1)),
  );
  return new Map(
    reply.clips.flatMap((c) => {
      const edit = editSchema.safeParse(c);
      const index = edit.success ? edit.data.id - 1 : -1;
      return edit.success && index >= 0 && index < count
        ? [[index, { title: edit.data.title, summary: edit.data.summary }]]
        : [];
    }),
  );
}

/** Parsed JSON, or null for text that is not JSON. */
export function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

const sha256 = async (bytes: Uint8Array<ArrayBuffer> | ArrayBuffer): Promise<string> =>
  hex(await crypto.subtle.digest("SHA-256", bytes));

/** Everything the result depends on: the algorithm, the models, the cues, hints and scores. */
export const inputHash = (data: VideoData, place: Place): Promise<string> =>
  sha256(
    new TextEncoder().encode(
      JSON.stringify({
        algorithm: ALGORITHM,
        models:
          place === "mac"
            ? [MAC_MODELS.score, MAC_MODELS.summary, PLACES.mac]
            : [...new Set([REGION_MODEL, READ_MODEL, GATE_MODEL]), SUMMARY_MODEL, CLAIRIC_MODEL],
        cues: data.cues.map((c) => [
          c.start,
          c.end,
          c.text,
          c.speaker ?? null,
          c.terms ?? null,
          c.opens ?? null,
          c.closes ?? null,
        ]),
        excitement: data.excitement ?? null,
      }),
    ),
  );

/** Runs the requests one after another: parallel calls hit Workers AI's per-minute limit. */
async function inTurn(ai: Ai, model: string, inputs: readonly unknown[]): Promise<unknown[]> {
  return inputs.reduce<Promise<unknown[]>>(async (done, input) => {
    const list = await done;
    list.push(await ai.run(model, input));
    return list;
  }, Promise.resolve([]));
}

/** Each region's topic block, reading the regions one after another. */
export async function blocksOf(
  ai: Ai,
  regions: readonly Region[],
  rules: Pick<PlaceRules, "early" | "shifts">,
): Promise<Block[]> {
  return regions.reduce<Promise<Block[]>>(async (done, region) => {
    const list = await done;
    const answers = await regionAnswers(ai, region, rules.early);
    const blocks = answers
      ? rules.shifts.flatMap((shift) => [
          blockOf(
            region,
            answers.map((a) => shifted(a, shift)),
          ),
        ])
      : [];
    return [...list, ...blocks.flatMap((b) => (b ? [b] : []))];
  }, Promise.resolve([]));
}

/** A region reply with its boundary and topic chances moved by `shift`, kept within 0 to 1. */
function shifted(response: unknown, shift: number): unknown {
  const parsed = clefSchema.safeParse(response);
  if (shift === 0 || !parsed.success) return response;
  const moved = Object.entries(parsed.data.answers).map(([id, a]) => [
    id,
    "noul" in a && (id.startsWith("boundary.") || id.startsWith("topic."))
      ? { ...a, noul: Math.min(1, Math.max(0, a.noul + shift)) }
      : a,
  ]);
  return { answers: Object.fromEntries(moved) };
}

/**
 * A region's clef answers, the request holding its climax asked first; null, with its other
 * requests never sent, when no opening there reaches EARLY_HOOK. The requests themselves do not
 * change, and clef answers a request the same whenever it is sent.
 */
async function regionAnswers(ai: Ai, region: Region, early: boolean): Promise<unknown[] | null> {
  const requests = clefRequests(region);
  const at = Math.max(
    0,
    requests.findIndex((r) => `clip.${region.climax}` in Object(Object(r).questions)),
  );
  const first = await ai.run(REGION_MODEL, requests[at]);
  const hooks = Object.entries(answersOf([first])).flatMap(([id, a]) =>
    id.startsWith("hook.") && "score" in a ? [a.score] : [],
  );
  return early && Math.max(0, ...hooks) < EARLY_HOOK
    ? null
    : [
        first,
        ...(await inTurn(
          ai,
          REGION_MODEL,
          requests.filter((_, i) => i !== at),
        )),
      ];
}

/**
 * Drafts read as whole clips, on sentence boundaries. One that is incoherent or has strays is
 * repaired and read again, up to REPAIRS times; every reading stays a candidate for pick.
 */
export async function coherent(
  ai: Ai,
  drafts: readonly Draft[],
  said: readonly Said[],
): Promise<Draft[]> {
  const read = (list: readonly Draft[]): Promise<Draft[]> =>
    inTurn(ai, READ_MODEL, judgeRequests(list, said)).then((r) => judged(list, r, said));
  const rounds = async (list: readonly Draft[], left: number): Promise<Draft[]> => {
    const done = await read(list);
    const next =
      left > 0
        ? done
            .filter(needsRepair)
            .flatMap(adjusted)
            .map((d) => closed(d, said))
        : [];
    return next.length > 0 ? [...done, ...(await rounds(next, left - 1))] : done;
  };
  return drafts.length > 0
    ? rounds(
        drafts.map((d) => closed(d, said)),
        REPAIRS,
      )
    : [];
}

/**
 * Builds the record: the slate is read with the rubric, and only the clips that ship and are not
 * already titled in `previous` (same story and ranges) go to the editor, in one request.
 */
/** Whether a candidate with gate chance `chance` ships, given the broadcast's chances best first. */
export function passing(chance: number, ranked: readonly number[], rules: PlaceRules): boolean {
  const relative = rules.relative;
  if (chance >= rules.least) return true;
  return relative !== null && chance >= relative.least && ranked.indexOf(chance) < relative.top;
}

export async function generate({
  ai: given,
  place,
  data,
  movieKey,
  previous,
  now,
}: Generation): Promise<ClipRecord> {
  const meter = metered(given);
  const ai = remembered(meter);
  const said = utterances(data.cues);
  const rules = PLACES[place];
  const clock = stageClock();
  const regions = regionsOf(data, rules);
  const blocks = await blocksOf(ai, regions, rules);
  clock.lap("regions");
  const all = blocks.flatMap(storiesOf);
  // Shifted blocks repeat most stories; Cloudflare keeps v9's list as it is (records stay v9's).
  const stories =
    rules.shifts.length > 1
      ? [...new Map(all.map((d) => [JSON.stringify([d.pattern, d.ranges]), d])).values()]
      : all;
  const slate = slateOf(await coherent(ai, stories, said), rules.slate);
  clock.lap("reads");
  // Cloudflare reads each candidate with clairic too; the Mac's gate is clef's alone.
  const readings = rules.second ? await clairicReadings(ai, slate, said) : slate.map(() => null);
  const quick = await inTurn(
    ai,
    SCREEN_MODEL,
    slate.map((d) => rubricRequest(d, said, SCREEN_NAME)),
  );
  clock.lap("screen");
  const kept = slate.flatMap((d, i) =>
    cheapOf(quick[i], readings[i], place) >= rules.floor ? [{ d, reading: readings[i] }] : [],
  );
  const goods = await inTurn(
    ai,
    GATE_MODEL,
    kept.map(({ d }) => rubricRequest(d, said, GATE_NAME)),
  );
  clock.lap("gate");
  const chance = new Map(kept.map(({ d, reading }, i) => [d, goodOf(goods[i], reading, place)]));
  const ranked = [...chance.values()].sort(descending);
  const passes = new Set([...chance].flatMap(([d, c]) => (passing(c, ranked, rules) ? [d] : [])));
  const shipped = pick(slate, (d) => (passes.has(d) ? (chance.get(d) ?? 0) : -1), 0);
  const known = new Map(
    (previous?.clips ?? []).map((c) => [JSON.stringify([c.pattern, c.ranges]), c]),
  );
  const fresh = shipped.filter((d) => !known.has(JSON.stringify([d.pattern, d.ranges])));
  const edits =
    fresh.length > 0 ? await edited(ai, fresh, said, EDITOR_ATTEMPTS) : new Map<number, Summary>();
  const titled = new Map(
    fresh.flatMap((d, i) => [edits.get(i)].flatMap((e) => (e ? [[d, e]] : []))),
  );
  clock.lap("editor");
  if (titled.size < fresh.length) {
    console.warn(
      `clip editor titled ${titled.size} of ${fresh.length} clips; the rest are dropped`,
    );
  }
  const drafts = shipped.filter(
    (d) => titled.has(d) || known.has(JSON.stringify([d.pattern, d.ranges])),
  );
  const clips: Clip[] = drafts.map((d, i) => {
    const summary = known.get(JSON.stringify([d.pattern, d.ranges])) ?? titled.get(d);
    return {
      id: `c${i + 1}`,
      kind: d.kind,
      pattern: d.pattern,
      duration: d.duration,
      score: d.score,
      ranges: d.ranges,
      title: summary?.title ?? "",
      summary: summary?.summary ?? "",
    };
  });
  // One line per video: what the record cost, measured from every response, and how many candidates
  // each stage left, so a low count is explained by the log alone.
  const funnel: Funnel = {
    regions: regions.length,
    stories: stories.length,
    slate: slate.length,
    screened: kept.length,
    gated: passes.size,
    best: ranked.slice(0, FUNNEL_BEST),
    shipped: shipped.length,
    titled: titled.size + (shipped.length - fresh.length),
    clips: clips.length,
  };
  console.log(
    JSON.stringify({ clipsUsage: data.id, models: meter.usage, funnel, seconds: clock.seconds }),
  );
  return {
    version: ALGORITHM,
    videoId: data.id,
    movieKey,
    inputHash: await inputHash(data, place),
    models: place === "mac" ? MAC_MODELS : { score: GATE_MODEL, summary: SUMMARY_MODEL },
    createdAt: now.toISOString(),
    excitement: Boolean(data.excitement),
    clips,
  };
}

/**
 * clairic's readings of the slate, one after another. clairic is a preview model, so any failure
 * leaves that clip to the clef-only fit instead of failing the video.
 */
async function clairicReadings(
  ai: Ai,
  slate: readonly Draft[],
  said: readonly Said[],
): Promise<unknown[]> {
  return slate.reduce<Promise<unknown[]>>(async (done, d) => {
    const list = await done;
    list.push(
      await ai.run(CLAIRIC_MODEL, clairicRequest(d, said)).catch((error: unknown) => {
        console.warn(`clairic failed, using the clef-only fit: ${String(error)}`);
        return null;
      }),
    );
    return list;
  }, Promise.resolve([]));
}

/** The editor's verdicts; a reply outside the form asks again, so the clef readings are kept. */
async function edited(
  ai: Ai,
  slate: readonly Draft[],
  said: readonly Said[],
  left: number,
): Promise<Map<number, Summary>> {
  try {
    const edits = editsOf(
      await ai.run(SUMMARY_MODEL, editorRequest(slate, said, left)),
      slate.length,
    );
    // A reply titling only some clips dropped the rest silently (suspected in 10/28 shipping 1); ask again
    // and keep every title either reply gave.
    if (edits.size === slate.length || left <= 1) return edits;
    const more = await edited(ai, slate, said, left - 1).catch(() => new Map<number, Summary>());
    return new Map<number, Summary>([...more, ...edits]);
  } catch (error) {
    if (left <= 1) throw error;
    return edited(ai, slate, said, left - 1);
  }
}

/**
 * clef answers an identical request identically (128 of 128 answers on a repeat), so within one
 * generation a request clef was already sent (a repaired draft rebuilt as one read before, a
 * candidate on the slate twice) reuses that answer instead of a call.
 */
export function remembered(ai: Ai): Ai {
  const answers = new Map<string, Promise<unknown>>();
  return {
    run: (model, input) => {
      if (!STEADY.has(model)) return ai.run(model, input);
      const key = JSON.stringify(input);
      const known = answers.get(key) ?? ai.run(model, input);
      answers.set(key, known);
      return known;
    },
  };
}

/** Wall seconds of each stage of one generation, for the log line (where a run's time goes). */
function stageClock(): { lap(stage: string): void; seconds: Record<string, number> } {
  const seconds: Record<string, number> = {};
  let last = Date.now();
  return {
    seconds,
    lap: (stage) => {
      const now = Date.now();
      seconds[stage] = Math.round((now - last) / 100) / 10;
      last = now;
    },
  };
}

/** Counts every call per model and the input tokens its response reports. */
export function metered(ai: Ai): Ai & { usage: Record<string, Usage> } {
  const usage: Record<string, Usage> = {};
  return {
    usage,
    run: async (model, input) => {
      const answer = await ai.run(model, input);
      const read = usageSchema.safeParse(answer);
      const spent = read.success
        ? (read.data.usage.input_tokens ?? read.data.usage.prompt_tokens ?? 0)
        : 0;
      const sum = usage[model] ?? { calls: 0, tokens: 0 };
      usage[model] = { calls: sum.calls + 1, tokens: sum.tokens + spent };
      return answer;
    },
  };
}

/** The content key of an R2 movie without downloading it: two 1 MiB range reads. */
export async function movieKey(bucket: Bucket, key: string): Promise<string | null> {
  const head = await bucket.head(key);
  if (!head) return null;
  const read = async (offset: number): Promise<ArrayBuffer> => {
    const object = await bucket.get(key, {
      range: { offset, length: Math.min(KEY_SAMPLE_BYTES, head.size - offset) },
    });
    return head.size > offset && object ? object.arrayBuffer() : new ArrayBuffer(0);
  };
  const parts = [
    String(head.size),
    await read(0),
    await read(Math.max(head.size - KEY_SAMPLE_BYTES, 0)),
  ];
  return (await sha256(await new Blob(parts).arrayBuffer())).slice(0, KEY_HEX);
}

async function readJson(bucket: Bucket, key: string): Promise<unknown> {
  const object = await bucket.get(key);
  return object ? object.json() : null;
}

async function putJson(bucket: Bucket, key: string, value: unknown): Promise<unknown> {
  return putAttempt(bucket, key, JSON.stringify(value), R2_PUT_ATTEMPTS);
}

async function putAttempt(
  bucket: Bucket,
  key: string,
  body: string,
  left: number,
): Promise<unknown> {
  try {
    return await bucket.put(key, body, JSON_PUT);
  } catch (error) {
    if (left <= 1) throw error;
    await new Promise((done) => setTimeout(done, R2_RETRY_MS));
    return putAttempt(bucket, key, body, left - 1);
  }
}

async function readRecord(bucket: Bucket, movie: string): Promise<ClipRecord | null> {
  const parsed = clipRecordSchema.safeParse(await readJson(bucket, recordKey(movie)));
  return parsed.success ? parsed.data : null;
}

/** The movie the cues follow: the edited one, or the original of a recording not edited yet. */
async function movieOf(bucket: Bucket, videoId: string): Promise<string | null> {
  const edited = `media/${videoId}.mp4`;
  if (await bucket.head(edited)) return edited;
  const catalog = catalogSchema.safeParse(await readJson(bucket, "catalog.json"));
  const original = catalog.success
    ? catalog.data.videos.find((v) => v.id === videoId)?.original
    : null;
  return original ? `media-original/${original.movie}` : null;
}

/**
 * Queue step for one video (an R2 upload event): it only points the video at its movie key. The
 * Worker runs no model; the Mac computes clips and stores them with `store`.
 */
export async function refresh(env: ClipEnv, request: ClipRequest): Promise<Outcome> {
  const raw = VIDEO_ID.test(request.videoId)
    ? await readJson(env.MEDIA, `data/${request.videoId}.json`)
    : null;
  if (!raw) return "no-data";
  videoDataSchema.parse(raw);
  const movie = await movieOf(env.MEDIA, request.videoId);
  const key = movie ? await movieKey(env.MEDIA, movie) : null;
  if (!key) return "no-movie";
  await putJson(env.MEDIA, pointerKey(request.videoId), { movieKey: key });
  await env.MEDIA.delete(pendingKey(request.videoId));
  return "skipped";
}

/**
 * Stores a record the Mac computed for a video under the published movie's key, which is derived
 * here rather than trusted from the upload so a viewer of that movie finds it. No model runs.
 */
export async function store(env: ClipEnv, videoId: string, body: unknown): Promise<Outcome> {
  const parsed = clipRecordSchema.safeParse(body);
  if (!parsed.success || parsed.data.videoId !== videoId) return "invalid";
  if (!VIDEO_ID.test(videoId) || !(await env.MEDIA.head(`data/${videoId}.json`))) return "no-data";
  const movie = await movieOf(env.MEDIA, videoId);
  const key = movie ? await movieKey(env.MEDIA, movie) : null;
  if (!key) return "no-movie";
  await Promise.all([
    putJson(env.MEDIA, recordKey(key), { ...parsed.data, movieKey: key }),
    putJson(env.MEDIA, pointerKey(videoId), { movieKey: key }),
  ]);
  await env.MEDIA.delete(pendingKey(videoId));
  return "written";
}

/** Why a record uploaded for a video is refused, or null when it is valid. */
export function recordIssue(videoId: string, body: unknown): string | null {
  const parsed = clipRecordSchema.safeParse(body);
  if (!parsed.success) return z.prettifyError(parsed.error);
  return parsed.data.videoId === videoId
    ? null
    : `videoId ${parsed.data.videoId} is not ${videoId}`;
}

/** A queue message: our own request, or an R2 event for an uploaded data/<id>.json. */
export function requestOf(body: unknown): ClipRequest | null {
  const own = ownRequestSchema.safeParse(body);
  if (own.success) return { videoId: own.data.videoId, force: own.data.force === true };
  const event = eventSchema.safeParse(body);
  const id = event.success ? event.data.object.key.match(DATA_KEY)?.[1] : undefined;
  return id ? { videoId: id, force: false } : null;
}

/**
 * Stored clips of a video. `key` (a viewer's own movie key) reads that content's record directly;
 * without it the key last computed for the video is used. A forced recomputation reads as pending,
 * and a key other than the published movie's reads as a mismatch.
 */
export async function lookup(env: ClipEnv, request: LookupRequest): Promise<Lookup> {
  const { videoId, key, now } = request;
  if (!VIDEO_ID.test(videoId)) return { status: "missing" };
  const [queued, pointer] = await Promise.all([
    readJson(env.MEDIA, pendingKey(videoId)).then((v) => pendingSchema.safeParse(v)),
    readJson(env.MEDIA, pointerKey(videoId)).then((v) => pointerSchema.safeParse(v)),
  ]);
  // A marker older than PENDING_SECONDS belongs to a computation that gave up: queue again.
  const pending =
    queued.success && now.getTime() / 1000 - queued.data.at < PENDING_SECONDS ? queued : null;
  if (pending?.data.force) return { status: "pending", force: true };
  const movie = key ?? (pointer.success ? pointer.data.movieKey : null);
  const record = movie ? await readRecord(env.MEDIA, movie) : null;
  if (record) return { status: "ready", record };
  if (pending) return { status: "pending", force: false };
  // The viewer's movie is not the one published: computing again would not produce its key. The
  // published movie itself whose clips the Mac has not made yet is missing, not a mismatch.
  return pointer.success && key !== null && key !== pointer.data.movieKey
    ? { status: "mismatch" }
    : { status: "missing" };
}
