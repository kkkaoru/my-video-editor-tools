// Runs with bun: every backend call reaches Tauri with the expected command and arguments.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { beforeEach, expect, it, type Mock, vi } from "vitest";
import {
  addVideos,
  cancelDownload,
  cancelJob,
  checkUpdate,
  deleteModel,
  deleteProfile,
  exportProfile,
  fetchSnapshot,
  fontFamilies,
  importProfile,
  installUpdate,
  loadProfile,
  onDownloadEvent,
  onFileDrop,
  onJobEvent,
  onUpdateProgress,
  overwriteProfile,
  pickFiles,
  pickFolder,
  pickProfileDestination,
  pickProfileFile,
  reformatFiles,
  relayoutTranscript,
  resetLook,
  resetModels,
  saveProfile,
  saveSettings,
  startDownload,
} from "./backend.ts";
import type { DownloadEvent, JobEvent, Settings } from "./types.ts";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [],
  unassigned: {
    fill: "#AAAAAA",
    fill_width: 0,
    outline: "#000000",
    outline_width: 0.1,
    rim: null,
    rim_width: 0.034,
    shadow: null,
    shadow_x: 0.03,
    shadow_y: -0.03,
    shadow_blur: 5,
  },
  font_family: "Sans",
  font_ratio: 0.05,
  width_ratio: 0.8,
  safe_left_percent: 5,
  safe_right_percent: 5,
  placement: {
    anchor: "video_top",
    offset_x_percent: 0,
    offset_y_percent: 0,
  },
  language: "auto",
  diarize_model: "b",
  face_model: "c",
  model_dirs: [],
  active_profile: "",
  line_height: 1.448,
  speaker_count: 0,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

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
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    [
      "save_settings",
      {
        settings: {
          palette: [],
          unassigned: {
            fill: "#AAAAAA",
            fill_width: 0,
            outline: "#000000",
            outline_width: 0.1,
            rim: null,
            rim_width: 0.034,
            shadow: null,
            shadow_x: 0.03,
            shadow_y: -0.03,
            shadow_blur: 5,
          },
          font_family: "Sans",
          font_ratio: 0.05,
          width_ratio: 0.8,
          safe_left_percent: 5,
          safe_right_percent: 5,
          placement: {
            anchor: "video_top",
            offset_x_percent: 0,
            offset_y_percent: 0,
          },
          language: "auto",
          diarize_model: "b",
          face_model: "c",
          model_dirs: [],
          active_profile: "",
          line_height: 1.448,
          speaker_count: 0,
          subtitle_format: "fcpxml",
          ui_language: "",
          ui_theme: "system",
        },
      },
    ],
  ]);
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
  const stop: Mock<() => void> = vi.fn();
  vi.mocked(listen).mockResolvedValue(stop);
  const received: JobEvent[] = [];
  (await onJobEvent((event: JobEvent): number => received.push(event)))();
  expect(stop).toHaveBeenCalledTimes(1);
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

it("rewrites dropped subtitle files", async () => {
  await reformatFiles(["/a.srt"]);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reformat_files", { paths: ["/a.srt"] }]]);
});

it("saves a new profile with the settings on screen", async () => {
  await saveProfile("A", SETTINGS);
  expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe("save_profile");
  expect(vi.mocked(invoke).mock.calls[0]?.[1]).toHaveProperty("name", "A");
});

it("overwrites a profile with the settings on screen", async () => {
  await overwriteProfile("A", SETTINGS);
  expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe("overwrite_profile");
  expect(vi.mocked(invoke).mock.calls[0]?.[1]).toHaveProperty("name", "A");
});

it("loads a profile", async () => {
  await loadProfile("A");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["load_profile", { name: "A" }]]);
});

it("deletes a profile", async () => {
  await deleteProfile("A");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["delete_profile", { name: "A" }]]);
});

it("reads the installed fonts", async () => {
  vi.mocked(invoke).mockResolvedValue(["Noto Sans JP"]);
  expect(await fontFamilies()).toStrictEqual(["Noto Sans JP"]);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["font_families"]]);
});

it("resets the models", async () => {
  await resetModels();
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reset_models"]]);
});

it("exports a profile to the chosen path", async () => {
  await exportProfile("A", "/a.json");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["export_profile", { name: "A", path: "/a.json" }]]);
});

it("imports a profile from a path", async () => {
  await importProfile("/a.json");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["import_profile", { path: "/a.json" }]]);
});

it("resets the look", async () => {
  await resetLook();
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reset_look"]]);
});

it("picks one profile file", async () => {
  vi.mocked(open).mockResolvedValue("/a.json");
  expect(await pickProfileFile("Profiles")).toBe("/a.json");
  expect(vi.mocked(open).mock.calls).toStrictEqual([
    [{ filters: [{ name: "Profiles", extensions: ["json"] }] }],
  ]);
});

it("asks where to save a profile, named after it", async () => {
  vi.mocked(save).mockResolvedValue("/b.json");
  expect(await pickProfileDestination("A", "Profiles")).toBe("/b.json");
  expect(vi.mocked(save).mock.calls).toStrictEqual([
    [{ defaultPath: "A.json", filters: [{ name: "Profiles", extensions: ["json"] }] }],
  ]);
});

it("asks for an update and installs it by name", async () => {
  vi.mocked(invoke).mockResolvedValue(null);
  await expect(checkUpdate()).resolves.toBe(null);
  expect(vi.mocked(invoke).mock.lastCall).toStrictEqual(["check_update"]);
  await installUpdate();
  expect(vi.mocked(invoke).mock.lastCall).toStrictEqual(["install_update"]);
});

it("passes on the update's progress", async () => {
  vi.mocked(listen).mockResolvedValue(vi.fn());
  const handler: Mock<(share: number) => void> = vi.fn();
  await onUpdateProgress(handler);
  expect(vi.mocked(listen).mock.lastCall?.[0]).toBe("update-progress");
  vi.mocked(listen).mock.lastCall?.[1]({
    event: "update-progress",
    id: 1,
    payload: 0.5,
  });
  expect(handler.mock.calls).toStrictEqual([[0.5]]);
});
