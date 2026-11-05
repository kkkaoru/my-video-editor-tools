// Runs with bun: the only module that talks to Tauri, so everything else is testable without it.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { DownloadEvent, JobEvent, Settings, Snapshot } from "./types.ts";

export interface DropHandlers {
  onHover: (hovering: boolean) => void;
  onDrop: (paths: string[]) => void;
}

const JOB_EVENT: string = "job";
const DOWNLOAD_EVENT: string = "download";
const PROFILE_EXTENSION: string = "json";
const HOVER_EVENTS: ReadonlySet<string> = new Set(["enter", "over"]);

export const fetchSnapshot = (): Promise<Snapshot> => invoke<Snapshot>("snapshot");

export const saveSettings = (settings: Settings): Promise<Snapshot> =>
  invoke<Snapshot>("save_settings", { settings });

export const addVideos = (paths: string[]): Promise<number[]> => invoke<number[]>("add_videos", { paths });

export const relayoutTranscript = (path: string): Promise<number> =>
  invoke<number>("relayout_transcript", { path });

export const reformatFiles = (paths: string[]): Promise<number[]> =>
  invoke<number[]>("reformat_files", { paths });

export const saveProfile = (name: string, settings: Settings): Promise<Snapshot> =>
  invoke<Snapshot>("save_profile", { name, settings });

export const overwriteProfile = (name: string, settings: Settings): Promise<Snapshot> =>
  invoke<Snapshot>("overwrite_profile", { name, settings });

export const loadProfile = (name: string): Promise<Snapshot> => invoke<Snapshot>("load_profile", { name });

export const deleteProfile = (name: string): Promise<Snapshot> =>
  invoke<Snapshot>("delete_profile", { name });

export const exportProfile = (name: string, path: string): Promise<void> =>
  invoke<void>("export_profile", { name, path });

export const importProfile = (path: string): Promise<Snapshot> =>
  invoke<Snapshot>("import_profile", { path });

export const resetLook = (): Promise<Snapshot> => invoke<Snapshot>("reset_look");

export const fontFamilies = (): Promise<string[]> => invoke<string[]>("font_families");

export const resetModels = (): Promise<Snapshot> => invoke<Snapshot>("reset_models");

export const cancelJob = (id: number): Promise<void> => invoke<void>("cancel_job", { id });

export const startDownload = (id: string): Promise<void> => invoke<void>("start_download", { id });

export const cancelDownload = (id: string): Promise<void> => invoke<void>("cancel_download", { id });

export const deleteModel = (id: string): Promise<Snapshot> => invoke<Snapshot>("delete_model", { id });

export const onJobEvent = (handler: (event: JobEvent) => void): Promise<UnlistenFn> =>
  listen<JobEvent>(JOB_EVENT, ({ payload }) => handler(payload));

export const onDownloadEvent = (handler: (event: DownloadEvent) => void): Promise<UnlistenFn> =>
  listen<DownloadEvent>(DOWNLOAD_EVENT, ({ payload }) => handler(payload));

export const onFileDrop = ({ onHover, onDrop }: DropHandlers): Promise<UnlistenFn> =>
  getCurrentWebview().onDragDropEvent(({ payload }) =>
    payload.type === "drop" ? onDrop(payload.paths) : onHover(HOVER_EVENTS.has(payload.type)),
  );

export const pickFiles = (extensions: string[], filterName: string): Promise<string[] | null> =>
  open({ multiple: true, filters: [{ name: filterName, extensions }] });

export const pickProfileFile = (filterName: string): Promise<string | null> =>
  open({ filters: [{ name: filterName, extensions: [PROFILE_EXTENSION] }] });

export const pickProfileDestination = (name: string, filterName: string): Promise<string | null> =>
  save({
    defaultPath: `${name}.${PROFILE_EXTENSION}`,
    filters: [{ name: filterName, extensions: [PROFILE_EXTENSION] }],
  });

export const pickFolder = (): Promise<string | null> => open({ directory: true });
