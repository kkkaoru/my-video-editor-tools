// Runs with bun: every measured figure the speaker texts quote, in one place; all from one Mac.
import type { Params } from "./messages.ts";

export const SPEAKER_FIGURES: Params = {
  encoderGb: "2.85",
  encoderGpuSeconds: 4,
  encoderCpuSeconds: 33,
  separateCoreMlGb: "1.1",
  separateGb: "1.65",
  separateSeconds: 8,
  jobPeakGb: "3",
  whisperCpuMinutes: 14,
  separateCoreMlMinutes: 5,
  separateCpuMinutes: 14,
};
