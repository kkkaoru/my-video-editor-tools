// Runs with bun: every backend call reaches Tauri with the expected command and arguments.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { beforeEach, expect, it, type Mock, vi } from "vitest";
import {
  addVideos,
  cancelDownload,
  cancelJob,
  deleteModel,
  fetchSnapshot,
  onDownloadEvent,
  onFileDrop,
  onJobEvent,
  pickFiles,
  pickFolder,
  relayoutTranscript,
  saveSettings,
  startDownload,
} from "./backend.ts";
import type { DownloadEvent, JobEvent, Settings } from "./types.ts";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  width_ratio: 0.8,
  placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
  language: "auto",
  transcribe_model: "a",
  diarize_model: "b",
  face_model: "c",
  model_dirs: [],
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue("answer");
});

it("fetches the snapshot", async () => {
  expect(await fetchSnapshot()).toBe("answer");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["snapshot"]]);
});

it("saves the settings", async () => {
  await saveSettings(SETTINGS);
  expect(vi.mocked(invoke)).toHaveBeenCalledWith("save_settings", { settings: SETTINGS });
});

it("adds videos by path", async () => {
  await addVideos(["/a.mp4", "/b.mov"]);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["add_videos", { paths: ["/a.mp4", "/b.mov"] }]]);
});

it("relays out a transcript", async () => {
  await relayoutTranscript("/a.transcript.json");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    ["relayout_transcript", { path: "/a.transcript.json" }],
  ]);
});

it("cancels a job", async () => {
  await cancelJob(7);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["cancel_job", { id: 7 }]]);
});

it("starts a download", async () => {
  await startDownload("yunet");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["start_download", { id: "yunet" }]]);
});

it("cancels a download", async () => {
  await cancelDownload("yunet");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["cancel_download", { id: "yunet" }]]);
});

it("deletes a model", async () => {
  await deleteModel("yunet");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["delete_model", { id: "yunet" }]]);
});

it("hands job event payloads to the handler", async () => {
  const stop: () => void = vi.fn();
  vi.mocked(listen).mockResolvedValue(stop);
  const received: JobEvent[] = [];
  expect(await onJobEvent((event: JobEvent): number => received.push(event))).toBe(stop);
  vi.mocked(listen).mock.calls[0]?.[1]({ event: "job", id: 1, payload: { state: "cancelled", id: 3 } });
  expect(vi.mocked(listen).mock.calls[0]?.[0]).toBe("job");
  expect(received).toStrictEqual([{ state: "cancelled", id: 3 }]);
});

it("hands download event payloads to the handler", async () => {
  vi.mocked(listen).mockResolvedValue(vi.fn());
  const received: DownloadEvent[] = [];
  await onDownloadEvent((event: DownloadEvent): number => received.push(event));
  vi.mocked(listen).mock.calls[0]?.[1]({ event: "download", id: 1, payload: { state: "finished", id: "a" } });
  expect(vi.mocked(listen).mock.calls[0]?.[0]).toBe("download");
  expect(received).toStrictEqual([{ state: "finished", id: "a" }]);
});

it("reports drops and hovering from the webview drag events", async () => {
  onDragDropEvent.mockResolvedValue(vi.fn());
  const hovers: boolean[] = [];
  const drops: string[][] = [];
  await onFileDrop({
    onHover: (hovering: boolean): number => hovers.push(hovering),
    onDrop: (paths: string[]): number => drops.push(paths),
  });
  const emit: DropCallback | undefined = onDragDropEvent.mock.calls[0]?.[0];
  emit?.({ payload: { type: "enter" } });
  emit?.({ payload: { type: "over" } });
  emit?.({ payload: { type: "leave" } });
  emit?.({ payload: { type: "drop", paths: ["/a.mp4"] } });
  expect(hovers).toStrictEqual([true, true, false]);
  expect(drops).toStrictEqual([["/a.mp4"]]);
});

it("picks several files with a named filter", async () => {
  vi.mocked(open).mockResolvedValue(["/a.mp4"]);
  expect(await pickFiles(["mp4"], "Videos")).toStrictEqual(["/a.mp4"]);
  expect(vi.mocked(open).mock.calls).toStrictEqual([
    [{ multiple: true, filters: [{ name: "Videos", extensions: ["mp4"] }] }],
  ]);
});

it("picks one folder", async () => {
  vi.mocked(open).mockResolvedValue("/models");
  expect(await pickFolder()).toBe("/models");
  expect(vi.mocked(open).mock.calls).toStrictEqual([[{ directory: true }]]);
});
