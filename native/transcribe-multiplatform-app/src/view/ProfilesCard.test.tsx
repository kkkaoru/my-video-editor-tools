// Runs with bun: profiles are listed, saved, restored, overwritten and deleted through the backend.
import { invoke } from "@tauri-apps/api/core";
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
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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
