// Runs with bun: placeholders fill in and backend errors read as sentences.
import { expect, it } from "vitest";
import type { BackendError } from "../backend/types.ts";
import { enMessages } from "./locale-en.ts";
import { errorText, format, roleName } from "./messages.ts";

it("fills every placeholder", () => {
  expect(format("{a} of {b}", { a: 1, b: "two" })).toBe("1 of two");
});

it("keeps a placeholder with no value", () => {
  expect(format("{a} of {b}", { a: 1 })).toBe("1 of {b}");
});

it("keeps a zero", () => {
  expect(format("{a}", { a: 0 })).toBe("0");
});

it("names the model in a known error", () => {
  expect(errorText(enMessages, { code: "model_missing" }, "Whisper")).toBe(
    "Whisper is not installed. Download it in Models.",
  );
});

it("names speaker separation for a diarize role", () => {
  expect(errorText(enMessages, { code: "no_model_for_role", role: "diarize" }, "")).toBe(
    "No model is installed for speaker separation. Download one in Models.",
  );
});

it("names transcription for any other role", () => {
  expect(errorText(enMessages, { code: "no_model_for_role", role: "transcribe" }, "")).toBe(
    "No model is installed for transcription. Download one in Models.",
  );
});

it("states the memory numbers when they are known", () => {
  expect(errorText(enMessages, { code: "needs_memory", required: 32, available: 16 }, "Big")).toBe(
    "Big needs 32 GB of RAM; this computer has 16 GB. Choose a smaller model.",
  );
});

it("falls back to zero when the memory numbers are absent", () => {
  expect(errorText(enMessages, { code: "needs_memory" }, "Big")).toBe(
    "Big needs 0 GB of RAM; this computer has 0 GB. Choose a smaller model.",
  );
});

it("shows the detail of an internal error", () => {
  expect(errorText(enMessages, { code: "internal", detail: "disk full" }, "")).toBe("disk full");
});

it("shows nothing for an internal error without detail", () => {
  expect(errorText(enMessages, { code: "internal" }, "")).toBe("");
});

it("shows the detail of an unknown code", () => {
  expect(errorText(enMessages, { code: "new_code", detail: "explained" }, "")).toBe("explained");
});

it("shows an unknown code when it has no detail", () => {
  expect(errorText(enMessages, { code: "new_code" }, "")).toBe("new_code");
});

it("names every model role", () => {
  expect(roleName(enMessages, "transcribe")).toBe("transcription");
  expect(roleName(enMessages, "diarize")).toBe("speaker separation");
  expect(roleName(enMessages, "face")).toBe("face detection");
  expect(roleName(enMessages, "runtime")).toBe("runtime");
});

it("names the dropped file that has no transcript", () => {
  expect(errorText(enMessages, { code: "transcript_missing", file: "clip.srt" }, "")).toBe(
    "clip.srt has no transcript beside it, so it cannot be written again. Keep the .transcript.json file with the same name in the same folder.",
  );
});

it("names the profile that already exists", () => {
  expect(errorText(enMessages, { code: "profile_exists", name: "A" }, "")).toBe(
    "A profile named A already exists. Choose another name, or overwrite it in the list.",
  );
});

it("says which models are missing and what to do", () => {
  expect(errorText(enMessages, { code: "models_missing", roles: ["transcribe", "diarize"] }, "")).toBe(
    "The videos were not added because no model is available for: transcription, speaker separation. Download them in Models, or add a folder that already has them.",
  );
});

it("names the file that is not a profile", () => {
  expect(errorText(enMessages, { code: "profile_invalid", file: "bad.json" }, "")).toBe(
    "bad.json cannot be read as a profile. Choose a file this app exported.",
  );
});

it("names the memory that runs short", () => {
  const error: BackendError = { code: "needs_memory", required: 24, available: 12, kind: "gpu_memory" };
  expect(errorText(enMessages, error, "Nemotron")).toBe(
    "Nemotron needs 24 GB of GPU memory; this computer has 12 GB. Choose a smaller model.",
  );
  expect(errorText(enMessages, { ...error, kind: "unified" }, "Nemotron")).toBe(
    "Nemotron needs 24 GB of unified memory (shared RAM and GPU memory); this computer has 12 GB. Choose a smaller model.",
  );
  expect(errorText(enMessages, { ...error, kind: "ram" }, "Nemotron")).toBe(
    "Nemotron needs 24 GB of RAM; this computer has 12 GB. Choose a smaller model.",
  );
});

it("names the kinds of model a job lacks", () => {
  expect(
    errorText(enMessages, { code: "speaker_models_missing", missing: ["diarize", "separate"] }, ""),
  ).toBe(
    "These models are needed for 2 or more speakers, or for automatic detection: speaker separation, voice separation. Download them in Models, or set the number of speakers to 1 and add the video again.",
  );
});
