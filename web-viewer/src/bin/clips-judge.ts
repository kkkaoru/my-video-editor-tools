// Runs with bun. Entry point: `bun run clips:judge -- --records DIR --judged FILE --local-cache DIR`
// judges saved clip records without any LLM judge or Cloudflare model: the surrogate (clef's rubric
// and Laya's answers to it from the local server on port 8791, started with --judges, and the old DeepSeek verdicts of the
// judged clips of the same moment). Prints one line per clip and a summary with the PPI-corrected good
// rate of the clips it calls good.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { z } from "zod";
import {
  clipRecordSchema,
  GATE_MODEL,
  LAYA_MODEL,
  rubricRequest,
  utterances,
  videoDataSchema,
} from "../clips.ts";
import { cachedAi, neighboursOf, shippedRate, surrogateOf } from "../clips-eval.ts";
import { localAi } from "../clips-mac.ts";

const LOCAL_SERVER: string = "http://127.0.0.1:8791";
const projectDirectory = resolve(import.meta.dirname, "..", "..");
const rangeSchema = z.object({ start: z.number(), end: z.number() });
const judgedSchema = z.array(
  z.object({ video: z.string(), ranges: z.array(rangeSchema), good: z.boolean() }),
);
const argsSchema = z.object({
  records: z.string(),
  judged: z.string(),
  "local-cache": z.string(),
});
const { values } = parseArgs({
  options: {
    records: { type: "string" },
    judged: { type: "string" },
    "local-cache": { type: "string" },
  },
});
const parsed = argsSchema.safeParse(values);
if (!parsed.success) {
  console.error("usage: bun run clips:judge -- --records DIR --judged FILE --local-cache DIR");
  process.exit(2);
}
const options = parsed.data;
const judged = judgedSchema.parse(JSON.parse(readFileSync(options.judged, "utf8")));
const store = (dir: string) => {
  mkdirSync(dir, { recursive: true });
  return {
    read: async (key: string) =>
      existsSync(join(dir, `${key}.json`)) ? readFileSync(join(dir, `${key}.json`), "utf8") : null,
    write: async (key: string, text: string) => writeFileSync(join(dir, `${key}.json`), text),
  };
};
const meter = { calls: 0, fresh: 0 };
const local = cachedAi(localAi(fetch, LOCAL_SERVER), store(options["local-cache"]), meter);
const tally = { clips: 0, good: 0, bad: 0, unjudged: 0, chance: 0 };
const calledGood: number[] = [];
for (const file of readdirSync(options.records).filter((f) => f.endsWith(".json"))) {
  const record = clipRecordSchema.parse(
    JSON.parse(readFileSync(join(options.records, file), "utf8")),
  );
  const said = utterances(
    videoDataSchema.parse(
      JSON.parse(readFileSync(join(projectDirectory, "data", `${record.videoId}.json`), "utf8")),
    ).cues,
  );
  for (const c of record.clips) {
    if (!c.pattern) continue;
    const clip = { pattern: c.pattern, ranges: c.ranges };
    const rubric = await local.run(GATE_MODEL, rubricRequest(clip, said, "clef"));
    const reading = await local.run(LAYA_MODEL, rubricRequest(clip, said, "laya"));
    const verdict = surrogateOf(
      rubric,
      reading,
      neighboursOf(record.videoId, c.ranges, judged),
      c.duration,
    );
    tally.clips += 1;
    tally[verdict.verdict] += 1;
    tally.chance += verdict.chance;
    if (verdict.verdict === "good") calledGood.push(verdict.chance);
    console.log(
      JSON.stringify({
        video: record.videoId,
        clip: c.id,
        kind: c.kind,
        title: c.title,
        ...verdict,
      }),
    );
  }
}
console.log(
  JSON.stringify({
    ...tally,
    goodRateOfCalledGood: shippedRate(calledGood),
    meanChance: Math.round((tally.chance / Math.max(1, tally.clips)) * 1000) / 1000,
    calls: meter,
  }),
);
