// Runs with bun: choosing, downloading and removing models, and adding model folders.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ModelStatus, Settings, Snapshot } from "../backend/types.ts";
import { type DownloadRow, initialState } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { formatBytes, hasRecommendedSpeakerModel, ModelsPane, missingDownloads } from "./ModelsPane.tsx";

const SETTINGS: Settings = {
  palette: [],
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
  face_model: "c",
  model_dirs: [],
  active_profile: "",
  line_height: 1.448,
  separation_backend: "onnx",
  speaker_count: 0,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
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
};
const SNAPSHOT: Snapshot = {
  version: "1.2.3",
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [MODEL],
  speaker_plan: null,
  speaker_missing: null,
  models_dir: "/models",
  profiles: [],
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
  replaceSnapshot: vi.fn(),
  dropFiles: vi.fn(),
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
  expect(formatBytes(0)).toBe("0 MB");
  expect(formatBytes(200_779_392)).toBe("201 MB");
  expect(formatBytes(107_012_128)).toBe("107 MB");
  expect(formatBytes(1_000_000_000)).toBe("1.0 GB");
  expect(formatBytes(1_610_612_736)).toBe("1.6 GB");
});

it("groups visible models under their role, in a fixed order", () => {
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    models: [
      { ...MODEL, id: "r", role: "runtime", label: "Runtime" },
      { ...MODEL, id: "f", role: "face", label: "Face" },
      { ...MODEL, id: "d", role: "diarize", label: "Diarizer" },
      { ...MODEL, id: "t", role: "transcribe", label: "Transcriber" },
    ],
  };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(
    screen
      .getAllByRole("heading", { level: 2 })
      .map((heading: HTMLElement): string | null => heading.textContent),
  ).toStrictEqual(["Models", "transcription", "speaker separation", "face detection", "runtime"]);
});

it("skips a role with no model", () => {
  render(<ModelsPane store={storeWith(SNAPSHOT, new Map())} snapshot={SNAPSHOT} />);
  expect(screen.queryByRole("heading", { name: "runtime" })).toBe(null);
});

it("explains a model from its note, and shows nothing for an unknown model", () => {
  const snapshot: Snapshot = { ...SNAPSHOT, models: [MODEL, { ...MODEL, id: "unknown", label: "Other" }] };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(
    screen.getByText("Transcription. The same model runs on Mac (Metal) and Windows (Vulkan).").tagName,
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
  showModel({ ...MODEL, download_bytes: 1, blocked: { code: "needs_memory" } }, null);
  expect(
    screen.getByText("Whisper needs 0 GB of RAM; this computer has 0 GB. Choose a smaller model.").tagName,
  ).toBe("P");
  expect(
    screen.queryAllByRole("button").map((button: HTMLElement): string | null => button.textContent),
  ).toStrictEqual(["Add a folder with existing models", "Reset models and download again"]);
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
          face_model: "c",
          model_dirs: ["/x", "/extra"],
          active_profile: "",
          line_height: 1.448,
          separation_backend: "onnx",
          speaker_count: 0,
          subtitle_format: "fcpxml",
          ui_language: "",
          ui_theme: "system",
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

it("downloads what is recommended and missing", () => {
  expect(
    missingDownloads({
      ...SNAPSHOT,
      models: [
        { ...MODEL, id: "t", recommended: true, download_bytes: 5 },
        { ...MODEL, id: "t2", download_bytes: 5 },
        { ...MODEL, id: "d", role: "diarize", recommended: true, download_bytes: 9 },
        { ...MODEL, id: "d2", role: "diarize", download_bytes: 2 },
      ],
    }).map(({ id }): string => id),
  ).toStrictEqual(["t", "d"]);
});

it("offers one button for everything missing and starts each download with its own bar", () => {
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    models: [
      { ...MODEL, id: "t", recommended: true, download_bytes: 2097152 },
      { ...MODEL, id: "d", role: "diarize", label: "Diarizer", recommended: true, download_bytes: 1048576 },
    ],
  };
  const store: AppStore = storeWith(snapshot, new Map());
  render(<ModelsPane store={store} snapshot={snapshot} />);
  fireEvent.click(screen.getByRole("button", { name: "Download what is missing: 2 (3 MB)" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    ["start_download", { id: "t" }],
    ["start_download", { id: "d" }],
  ]);
  expect(vi.mocked(store.setState).mock.calls.length).toBe(2);
});

it("does not offer what is already downloading", () => {
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    models: [{ ...MODEL, id: "t", recommended: true, download_bytes: 2097152 }],
  };
  render(
    <ModelsPane
      store={storeWith(
        snapshot,
        new Map([["t", { running: true, doneBytes: 0, totalBytes: 2097152, error: null }]]),
      )}
      snapshot={snapshot}
    />,
  );
  expect(screen.queryByRole("button", { name: /Download what is missing/ })).toBe(null);
});

it("resets only after confirming, then downloads what is now missing", async () => {
  const fresh: Snapshot = {
    ...SNAPSHOT,
    models: [{ ...MODEL, id: "t", recommended: true, download_bytes: 2097152 }],
  };
  vi.mocked(invoke).mockResolvedValue(fresh);
  const store: AppStore = storeWith(SNAPSHOT, new Map());
  render(
    <main>
      <ModelsPane store={store} snapshot={SNAPSHOT} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Reset models and download again" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(screen.getByRole("alertdialog", { name: "Reset the models and download them again?" }).tagName).toBe(
    "DIV",
  );
  expect(
    screen.getByText(
      "Everything this app downloaded in /models is deleted, then the models this computer needs are downloaded again. Folders you added are never touched. This cannot be undone.",
    ).tagName,
  ).toBe("P");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete and download again" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reset_models"], ["start_download", { id: "t" }]]);
  expect(vi.mocked(store.applySnapshot).mock.calls.length).toBe(1);
});

it("shows why a reset was refused", async () => {
  vi.mocked(invoke).mockRejectedValue({ code: "models_busy" });
  const store: AppStore = storeWith(SNAPSHOT, new Map());
  render(
    <main>
      <ModelsPane store={store} snapshot={SNAPSHOT} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Reset models and download again" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete and download again" }));
  });
  expect(failureOf(store)).toStrictEqual({ code: "models_busy" });
});

it("reports a bulk download that cannot start", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("offline"));
  const snapshot: Snapshot = {
    ...SNAPSHOT,
    models: [{ ...MODEL, id: "t", recommended: true, download_bytes: 2097152 }],
  };
  const store: AppStore = storeWith(snapshot, new Map());
  render(<ModelsPane store={store} snapshot={snapshot} />);
  fireEvent.click(screen.getByRole("button", { name: /Download what is missing/ }));
  await waitFor((): void => {
    expect(vi.mocked(store.setState).mock.calls.length).toBe(2);
  });
});

it("can delete a runtime wherever it was found, so a broken one can be replaced", () => {
  showModel({ ...MODEL, id: "geforce-runtime", role: "runtime", found_at: "/data/runtime/geforce" }, null);
  expect(screen.getByRole("button", { name: "Delete the downloaded copy of Whisper" }).tagName).toBe(
    "BUTTON",
  );
});

it("downloads no speaker model on a computer with none recommended", () => {
  expect(
    missingDownloads({
      ...SNAPSHOT,
      models: [
        { ...MODEL, id: "t", recommended: true, download_bytes: 5 },
        { ...MODEL, id: "small", role: "diarize", download_bytes: 2 },
      ],
    }).map(({ id }): string => id),
  ).toStrictEqual(["t"]);
});

it("knows whether a speaker model is recommended here", () => {
  expect(hasRecommendedSpeakerModel(SNAPSHOT)).toBe(false);
  expect(
    hasRecommendedSpeakerModel({ ...SNAPSHOT, models: [{ ...MODEL, role: "diarize", recommended: true }] }),
  ).toBe(true);
});

it("says what to do on a computer with no recommended speaker model", () => {
  render(<ModelsPane store={storeWith(SNAPSHOT, new Map())} snapshot={SNAPSHOT} />);
  expect(
    screen.getByText(
      "No model for telling speakers apart is recommended for this computer. You can still make subtitles for 1 speaker.",
    ).tagName,
  ).toBe("P");
});

it("stays quiet when a speaker model is recommended", () => {
  const snapshot: Snapshot = { ...SNAPSHOT, models: [{ ...MODEL, role: "diarize", recommended: true }] };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(screen.queryByText(/No speaker separation model/)).toBe(null);
});

it("says when the voice separation model is needed", () => {
  const separator: ModelStatus = {
    ...SNAPSHOT.models[0],
    id: "mossformer2-ss-16k",
    role: "separate",
    label: "MossFormer2 SS 16K",
  };
  const snapshot: Snapshot = { ...SNAPSHOT, models: [separator] };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(screen.getByRole("heading", { name: "voice separation" }).tagName).toBe("H2");
  expect(screen.getByText("Used for 2 or more speakers").getAttribute("data-slot")).toBe("badge");
  expect(screen.getByText("Splits overlapping voices. Used by “High quality”.").tagName).toBe("P");
  expect(
    screen.queryByText("Needed when 2 or more people speak, or when the number is detected automatically."),
  ).toBe(null);
});

it("describes the Nemotron 3.5 encoder with its size, its one job and the full model", () => {
  const encoder: ModelStatus = {
    ...SNAPSHOT.models[0],
    id: "nemotron-3.5-speaker",
    role: "diarize",
    label: "Nemotron 3.5",
  };
  const snapshot: Snapshot = { ...SNAPSHOT, models: [encoder] };
  render(<ModelsPane store={storeWith(snapshot, new Map())} snapshot={snapshot} />);
  expect(
    screen.getByText(
      "An encoder used only to tell speakers apart. It runs on the GPU or the CPU. Whisper turbo does the transcribing. A full Nemotron 3.5 model already in your folders also works.",
    ).tagName,
  ).toBe("P");
});
