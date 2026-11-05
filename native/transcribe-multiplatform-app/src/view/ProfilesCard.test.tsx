// Runs with bun: profiles are listed, saved, restored, overwritten and deleted through the backend.
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Profile, Settings, Snapshot } from "../backend/types.ts";
import { initialState } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { ProfilesCard } from "./ProfilesCard.tsx";

const SETTINGS: Settings = {
  palette: [{ fill: "#111111", outline: "#222222" }],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  rim_ratio: 0.03,
  line_height: 1.4,
  shadow: { enabled: false, strength: 0.5 },
  width_ratio: 0.8,
  placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
  language: "auto",
  diarize_model: "b",
  speaker_count: 0,
  face_model: "c",
  model_dirs: [],
  active_profile: "",
};
const SNAPSHOT: Snapshot = {
  version: "1.2.3",
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [],
  models_dir: "/models",
  profiles: [],
};
const PROFILE: Profile = {
  name: "A",
  look: {
    palette: SETTINGS.palette,
    unassigned: SETTINGS.unassigned,
    font_family: SETTINGS.font_family,
    font_ratio: SETTINGS.font_ratio,
    outline_ratio: SETTINGS.outline_ratio,
    rim_ratio: SETTINGS.rim_ratio,
    line_height: SETTINGS.line_height,
    shadow: SETTINGS.shadow,
    width_ratio: SETTINGS.width_ratio,
    placement: SETTINGS.placement,
  },
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const storeOf = (): AppStore => ({
  state: initialState(SNAPSHOT),
  setState: vi.fn(),
  updateSettings: vi.fn(),
  applySnapshot: vi.fn(),
  replaceSnapshot: vi.fn(),
  dropFiles: vi.fn(),
});

function show(settings: Settings, profiles: Profile[]): AppStore {
  const store: AppStore = storeOf();
  render(
    <main>
      <ProfilesCard store={store} settings={settings} profiles={profiles} />
    </main>,
  );
  return store;
}

const failureOf = (store: AppStore): unknown =>
  vi.mocked(store.setState).mock.calls[0]?.[0](initialState(SNAPSHOT)).loadError;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(SNAPSHOT);
});

afterEach(cleanup);

it("says when nothing is saved yet", () => {
  show(SETTINGS, []);
  expect(screen.getByText("No profiles saved yet.").tagName).toBe("P");
});

it("saves the settings under the typed name and clears the field", async () => {
  const store: AppStore = show(SETTINGS, []);
  const field: HTMLElement = screen.getByRole("textbox", { name: "Profile name" });
  fireEvent.input(field, { target: { value: "Stream" } });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Save as a new profile" }));
  });
  expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe("save_profile");
  expect(vi.mocked(invoke).mock.calls[0]?.[1]).toHaveProperty("name", "Stream");
  expect(vi.mocked(store.replaceSnapshot).mock.calls.length).toBe(1);
  await waitFor((): void => {
    expect(field instanceof HTMLInputElement ? field.value : null).toBe("");
  });
});

it("shows a refused name in the banner", async () => {
  vi.mocked(invoke).mockRejectedValue({ code: "profile_exists", name: "A" });
  const store: AppStore = show(SETTINGS, [PROFILE]);
  fireEvent.input(screen.getByRole("textbox", { name: "Profile name" }), { target: { value: "A" } });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Save as a new profile" }));
  });
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "profile_exists", name: "A" });
  });
  expect(vi.mocked(store.replaceSnapshot).mock.calls.length).toBe(0);
});

it("marks only the selected profile", () => {
  show({ ...SETTINGS, active_profile: "A" }, [PROFILE, { ...PROFILE, name: "B" }]);
  expect(screen.getAllByText("Selected").length).toBe(1);
  expect(screen.queryByText("Unsaved changes")).toBe(null);
});

it("marks unsaved changes on the selected profile", () => {
  show({ ...SETTINGS, active_profile: "A", font_ratio: 0.06 }, [PROFILE]);
  expect(screen.getByText("Unsaved changes").tagName).toBe("SPAN");
});

it("does not mark changes against a profile that is not selected", () => {
  show({ ...SETTINGS, font_ratio: 0.06 }, [PROFILE]);
  expect(screen.queryByText("Unsaved changes")).toBe(null);
});

it("loads a profile at once", async () => {
  const store: AppStore = show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Load A" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["load_profile", { name: "A" }]]);
  expect(vi.mocked(store.replaceSnapshot).mock.calls.length).toBe(1);
});

it("overwrites only after confirming", async () => {
  show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Overwrite A" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(screen.getByRole("alertdialog", { name: "Overwrite A?" }).tagName).toBe("DIV");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Overwrite A" }));
  });
  expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe("overwrite_profile");
});

it("deletes only after confirming", async () => {
  show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete A" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete A" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["delete_profile", { name: "A" }]]);
});

it("keeps the typed name when saving fails", async () => {
  vi.mocked(invoke).mockRejectedValue({ code: "profile_exists", name: "A" });
  const store: AppStore = show(SETTINGS, [PROFILE]);
  const field: HTMLElement = screen.getByRole("textbox", { name: "Profile name" });
  fireEvent.input(field, { target: { value: "A" } });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Save as a new profile" }));
  });
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "profile_exists", name: "A" });
  });
  expect(field instanceof HTMLInputElement ? field.value : null).toBe("A");
});

it("exports a profile to the place chosen in the dialog", async () => {
  vi.mocked(save).mockResolvedValue("/out/A.json");
  show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Export A" }));
  });
  await waitFor((): void => {
    expect(vi.mocked(invoke).mock.calls).toStrictEqual([
      ["export_profile", { name: "A", path: "/out/A.json" }],
    ]);
  });
});

it("exports nothing when the save dialog is dismissed", async () => {
  vi.mocked(save).mockResolvedValue(null);
  show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Export A" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("shows a failed export in the banner", async () => {
  vi.mocked(save).mockResolvedValue("/out/A.json");
  vi.mocked(invoke).mockRejectedValue(new Error("read only"));
  const store: AppStore = show(SETTINGS, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Export A" }));
  });
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: read only" });
  });
});

it("imports the chosen file and lists what comes back", async () => {
  vi.mocked(open).mockResolvedValue("/in/B.json");
  const store: AppStore = show(SETTINGS, []);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Import from a file" }));
  });
  await waitFor((): void => {
    expect(vi.mocked(store.replaceSnapshot).mock.calls.length).toBe(1);
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["import_profile", { path: "/in/B.json" }]]);
});

it("imports nothing when the open dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  show(SETTINGS, []);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Import from a file" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("explains a file that is not a profile", async () => {
  vi.mocked(open).mockResolvedValue("/in/bad.json");
  vi.mocked(invoke).mockRejectedValue({ code: "profile_invalid", file: "bad.json" });
  const store: AppStore = show(SETTINGS, []);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Import from a file" }));
  });
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "profile_invalid", file: "bad.json" });
  });
});

it("reports a dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = show(SETTINGS, []);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Import from a file" }));
  });
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("resets the look only after confirming, and says what stays", async () => {
  const store: AppStore = show(SETTINGS, []);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Restore the original look and position" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(
    screen.getByText(
      "The current look and position go back to the app's original settings. Other profiles, the number of speakers, models and the language do not change. This cannot be undone.",
    ).tagName,
  ).toBe("P");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reset_look"]]);
  await waitFor((): void => {
    expect(vi.mocked(store.replaceSnapshot).mock.calls.length).toBe(1);
  });
});

it("names the selected profile as the only one the reset touches", async () => {
  show({ ...SETTINGS, active_profile: "A" }, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Restore the original look and position" }));
  });
  expect(
    screen.getByText(
      "The look and position of the profile A go back to the app's original settings. Other profiles, the number of speakers, models and the language do not change. This cannot be undone.",
    ).tagName,
  ).toBe("P");
});

it("treats a selection left over from a deleted profile as no selection", async () => {
  show({ ...SETTINGS, active_profile: "Gone" }, [PROFILE]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Restore the original look and position" }));
  });
  expect(
    screen.getByText(
      "The current look and position go back to the app's original settings. Other profiles, the number of speakers, models and the language do not change. This cannot be undone.",
    ).tagName,
  ).toBe("P");
});
