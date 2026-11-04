// Runs with bun: choosing, downloading and removing models, and adding model folders.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ModelStatus, Settings, Snapshot } from "../backend/types.ts";
import { type DownloadRow, initialState } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { formatBytes, ModelsPane } from "./ModelsPane.tsx";

const SETTINGS: Settings = {
  palette: [],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  shadow: { enabled: false, strength: 0.5 },
  width_ratio: 0.8,
  placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
  language: "auto",
  diarize_model: "b",
  face_model: "c",
  model_dirs: [],
};
const MODEL: ModelStatus = {
  id: "whisper-large-v3-turbo",
  role: "transcribe",
  label: "Whisper",
  source_url: "",
  download_bytes: 0,
  found_at: null,
  blocked: null,
  recommended: false,
  hidden: false,
};
const SNAPSHOT: Snapshot = {
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [MODEL],
  models_dir: "/models",
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const storeWith = (snapshot: Snapshot, downloads: ReadonlyMap<string, DownloadRow>): AppStore => ({
  state: { ...initialState(snapshot), downloads },
  setState: vi.fn(),
  updateSettings: vi.fn(),
  applySnapshot: vi.fn(),
  dropVideos: vi.fn(),
});

function showModel(model: ModelStatus, download: DownloadRow | null): AppStore {
  const snapshot: Snapshot = { ...SNAPSHOT, models: [model] };
  const store: AppStore = storeWith(snapshot, new Map(download === null ? [] : [[model.id, download]]));
  render(<ModelsPane store={store} snapshot={snapshot} />);
  return store;
}

const failureOf = (store: AppStore): unknown =>
  vi.mocked(store.setState).mock.calls[0]?.[0](initialState(SNAPSHOT)).loadError;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(SNAPSHOT);
});

afterEach(cleanup);

it("rounds sizes to megabytes or gigabytes", () => {
  expect(formatBytes(1)).toBe("1 MB");
  expect(formatBytes(1024 * 1024 * 1024)).toBe("1.0 GB");
  expect(formatBytes(1610612736)).toBe("1.5 GB");
});

it("groups visible models under their role, in a fixed order", () => {
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    models: [
      { ...MODEL, id: "r", role: "runtime", label: "Runtime" },
      { ...MODEL, id: "f", role: "face", label: "Face" },
      { ...MODEL, id: "d", role: "diarize", label: "Diarizer" },
      { ...MODEL, id: "t", role: "transcribe", label: "Transcriber" },
      { ...MODEL, id: "h", role: "transcribe", label: "Hidden", hidden: true },
    ],
  };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(
    screen
      .getAllByRole("heading", { level: 2 })
      .map((heading: HTMLElement): string | null => heading.textContent),
  ).toStrictEqual(["Models", "transcription", "speaker separation", "face detection", "runtime"]);
  expect(screen.queryByText("Hidden")).toBe(null);
});

it("skips a role with no visible model", () => {
  render(<ModelsPane store={storeWith(SNAPSHOT, new Map())} snapshot={SNAPSHOT} />);
  expect(screen.queryByRole("heading", { name: "runtime" })).toBe(null);
});

it("explains a model from its note, and shows nothing for an unknown model", () => {
  const snapshot: Snapshot = { ...SNAPSHOT, models: [MODEL, { ...MODEL, id: "unknown", label: "Other" }] };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(
    screen.getByText("Transcription. The same model runs on Mac (Metal) and GeForce (CUDA).").tagName,
  ).toBe("P");
});

it("shows the recommended badge", () => {
  showModel({ ...MODEL, recommended: true }, null);
  expect(screen.getByText("Recommended here").tagName).toBe("SPAN");
});

it("shows where a present model lives", () => {
  showModel({ ...MODEL, found_at: "/shared/whisper" }, null);
  expect(screen.getByText("Available").tagName).toBe("SPAN");
  expect(screen.getByText("Location: /shared/whisper").tagName).toBe("P");
});

it("shows where a missing model comes from", () => {
  showModel({ ...MODEL, source_url: "https://example.test/m" }, null);
  expect(screen.getByText("Source: https://example.test/m").tagName).toBe("P");
});

it("hides the source of a present model", () => {
  showModel({ ...MODEL, found_at: "/shared/whisper", source_url: "https://example.test/m" }, null);
  expect(screen.queryByText("Source: https://example.test/m")).toBe(null);
});

it("explains a blocked model and offers no action", () => {
  showModel({ ...MODEL, download_bytes: 1, blocked: { code: "needs_nvidia" } }, null);
  expect(screen.getByText("Whisper needs an NVIDIA GPU. Choose another model in Models.").tagName).toBe("P");
  expect(
    screen.queryAllByRole("button").map((button: HTMLElement): string | null => button.textContent),
  ).toStrictEqual(["Add a folder with existing models"]);
});

it("selects a present diarizer", () => {
  const store: AppStore = showModel({ ...MODEL, id: "d", role: "diarize", found_at: "/shared/d" }, null);
  fireEvent.click(screen.getByRole("button", { name: "Use Whisper" }));
  expect(screen.getByRole("button", { name: "Use Whisper" }).getAttribute("aria-pressed")).toBe("false");
  expect(vi.mocked(store.updateSettings).mock.calls[0]?.[0].diarize_model).toBe("d");
});

it("offers no choice of transcription model", () => {
  showModel({ ...MODEL, found_at: "/shared/whisper" }, null);
  expect(screen.queryByRole("button", { name: "Use Whisper" })).toBe(null);
});

it("offers no choice of a blocked diarizer", () => {
  showModel({ ...MODEL, role: "diarize", found_at: "/shared/d", blocked: { code: "needs_nvidia" } }, null);
  expect(screen.queryByRole("button", { name: "Use Whisper" })).toBe(null);
});

it("offers no choice of a missing diarizer", () => {
  showModel({ ...MODEL, role: "diarize" }, null);
  expect(screen.queryByRole("button", { name: "Use Whisper" })).toBe(null);
});

it("marks the model in use", () => {
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    settings: { ...SETTINGS, diarize_model: "d" },
    models: [{ ...MODEL, id: "d", role: "diarize", label: "Diarizer", found_at: "/d" }],
  };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(screen.getByRole("button", { name: "In use: Diarizer" }).getAttribute("aria-pressed")).toBe("true");
});

it("offers no selection for a role without a setting", () => {
  showModel({ ...MODEL, role: "face", found_at: "/f" }, null);
  expect(screen.queryByRole("button", { name: "Use Whisper" })).toBe(null);
});

it("starts a download and records it", () => {
  const store: AppStore = showModel({ ...MODEL, download_bytes: 2097152 }, null);
  fireEvent.click(screen.getByRole("button", { name: "Download Whisper (2 MB)" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["start_download", { id: "whisper-large-v3-turbo" }]]);
  expect(
    vi
      .mocked(store.setState)
      .mock.calls[0]?.[0](initialState(SNAPSHOT))
      .downloads.get("whisper-large-v3-turbo"),
  ).toStrictEqual({ running: true, doneBytes: 0, totalBytes: 2097152, error: null });
});

it("reports a download that cannot start", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("offline"));
  const store: AppStore = showModel({ ...MODEL, download_bytes: 2097152 }, null);
  fireEvent.click(screen.getByRole("button", { name: "Download Whisper (2 MB)" }));
  await waitFor((): void => {
    expect(vi.mocked(store.setState)).toHaveBeenCalledTimes(2);
  });
  expect(vi.mocked(store.setState).mock.calls[1]?.[0](initialState(SNAPSHOT)).loadError).toStrictEqual({
    code: "internal",
    detail: "Error: offline",
  });
});

it("offers nothing for a model with no download", () => {
  showModel(MODEL, null);
  expect(screen.queryByRole("button", { name: /Whisper/ })).toBe(null);
});

it("offers no download for a model already present", () => {
  showModel({ ...MODEL, download_bytes: 1, found_at: "/shared/whisper" }, null);
  expect(screen.queryByRole("button", { name: /Download/ })).toBe(null);
});

it("shows progress and cancels a running download", () => {
  showModel(
    { ...MODEL, download_bytes: 4194304 },
    { running: true, doneBytes: 1048576, totalBytes: 4194304, error: null },
  );
  expect(screen.getByText("1 MB of 4 MB").tagName).toBe("P");
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Download of Whisper" });
  expect(bar.getAttribute("aria-valuenow")).toBe("0.25");
  expect(bar.getAttribute("aria-valuetext")).toBe("1 MB of 4 MB");
  fireEvent.click(screen.getByRole("button", { name: "Cancel the download of Whisper" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["cancel_download", { id: "whisper-large-v3-turbo" }]]);
});

it("shows an indeterminate bar until the total is known", () => {
  showModel(MODEL, { running: true, doneBytes: 0, totalBytes: 0, error: null });
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(null);
});

it("reports a cancel that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("stuck"));
  const store: AppStore = showModel(MODEL, { running: true, doneBytes: 0, totalBytes: 0, error: null });
  fireEvent.click(screen.getByRole("button", { name: "Cancel the download of Whisper" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: stuck" });
  });
});

it("explains a failed download and offers it again", () => {
  showModel(
    { ...MODEL, download_bytes: 1048576 },
    { running: false, doneBytes: 0, totalBytes: 0, error: { code: "internal", detail: "disk full" } },
  );
  expect(screen.getByRole("alert").textContent).toBe("disk full");
  expect(screen.getByRole("button", { name: "Download Whisper (1 MB)" }).tagName).toBe("BUTTON");
});

it("deletes the app's own copy and applies the new snapshot", async () => {
  const store: AppStore = showModel({ ...MODEL, found_at: "/models/whisper" }, null);
  fireEvent.click(screen.getByRole("button", { name: "Delete the downloaded copy of Whisper" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["delete_model", { id: "whisper-large-v3-turbo" }]]);
  await waitFor((): void => {
    expect(vi.mocked(store.applySnapshot)).toHaveBeenCalledTimes(1);
  });
});

it("reports a delete that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("busy"));
  const store: AppStore = showModel({ ...MODEL, found_at: "/models/whisper" }, null);
  fireEvent.click(screen.getByRole("button", { name: "Delete the downloaded copy of Whisper" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: busy" });
  });
});

it("never offers to delete a copy outside the app's folder", () => {
  showModel({ ...MODEL, found_at: "/shared/whisper" }, null);
  expect(screen.queryByRole("button", { name: /Delete/ })).toBe(null);
});

it("lists the extra model folders", () => {
  const snapshot: Snapshot = { ...SNAPSHOT, settings: { ...SETTINGS, model_dirs: ["/x", "/y"] } };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(screen.getByText("Folders searched: /x / /y").tagName).toBe("P");
});

it("hides the folder list when there are none", () => {
  render(<ModelsPane store={storeWith(SNAPSHOT, new Map())} snapshot={SNAPSHOT} />);
  expect(screen.queryByText(/Folders searched/)).toBe(null);
});

it("adds a chosen folder and applies the saved snapshot", async () => {
  vi.mocked(open).mockResolvedValue("/extra");
  const snapshot: Snapshot = { ...SNAPSHOT, settings: { ...SETTINGS, model_dirs: ["/x"] } };
  const store: AppStore = storeWith(snapshot, new Map());
  render(<ModelsPane store={store} snapshot={snapshot} />);
  fireEvent.click(screen.getByRole("button", { name: "Add a folder with existing models" }));
  await waitFor((): void => {
    expect(vi.mocked(store.applySnapshot)).toHaveBeenCalledTimes(1);
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    [
      "save_settings",
      {
        settings: {
          palette: [],
          unassigned: { fill: "#AAAAAA", outline: "#000000" },
          font_family: "Sans",
          font_ratio: 0.05,
          outline_ratio: 0.1,
          shadow: { enabled: false, strength: 0.5 },
          width_ratio: 0.8,
          placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
          language: "auto",
          diarize_model: "b",
          face_model: "c",
          model_dirs: ["/x", "/extra"],
        },
      },
    ],
  ]);
});

it("saves nothing when the folder dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  render(<ModelsPane store={storeWith(SNAPSHOT, new Map())} snapshot={SNAPSHOT} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Add a folder with existing models" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("reports a folder dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(SNAPSHOT, new Map());
  render(<ModelsPane store={store} snapshot={SNAPSHOT} />);
  fireEvent.click(screen.getByRole("button", { name: "Add a folder with existing models" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});
