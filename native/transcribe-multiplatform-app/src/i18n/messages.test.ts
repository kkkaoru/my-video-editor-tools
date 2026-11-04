// Runs with bun: placeholders fill in and backend errors read as sentences.
import { expect, it } from "vitest";
import { enMessages } from "./locale-en.ts";
import { errorText, format } from "./messages.ts";

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
    "Big needs 32 GB of memory; this computer has 16 GB. Choose a smaller model.",
  );
});

it("falls back to zero when the memory numbers are absent", () => {
  expect(errorText(enMessages, { code: "needs_memory" }, "Big")).toBe(
    "Big needs 0 GB of memory; this computer has 0 GB. Choose a smaller model.",
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
