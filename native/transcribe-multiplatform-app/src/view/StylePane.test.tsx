// Runs with bun: every style control reports its change, and the preview follows the settings.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import { initialState } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { defaultPalette, StylePane } from "./StylePane.tsx";

const SETTINGS: Settings = {
  palette: [
    { fill: "#111111", outline: "#222222" },
    { fill: "#333333", outline: "#444444" },
  ],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  width_ratio: 0.8,
  placement: { anchor: "video_bottom", offset_x_percent: 1, offset_y_percent: 2 },
  language: "auto",
  transcribe_model: "a",
  diarize_model: "b",
  face_model: "yunet",
  model_dirs: [],
};
const SNAPSHOT: Snapshot = {
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, nvidia_name: null, nvidia_memory_gib: 0 },
  settings: SETTINGS,
  models: [],
  models_dir: "/models",
};

const storeOf = (): AppStore => ({
  state: initialState(SNAPSHOT),
  setState: vi.fn(),
  updateSettings: vi.fn(),
  applySnapshot: vi.fn(),
  dropVideos: vi.fn(),
});

function show(settings: Settings): AppStore {
  const store: AppStore = storeOf();
  render(
    <StylePane
      store={store}
      settings={settings}
      faceOptions={[
        { value: "yunet", label: "YuNet", disabled: false },
        { value: "yolo", label: "YOLO", disabled: false },
      ]}
    />,
  );
  return store;
}

const lastSaved = (store: AppStore): Settings | undefined =>
  vi.mocked(store.updateSettings).mock.lastCall?.[0];

const descriptionText = (): string | null | undefined => screen.getByText(/Sample caption at/).textContent;

async function pressAndChoose(combobox: string, option: string): Promise<void> {
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: combobox }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: option }));
    fireEvent.click(screen.getByRole("option", { name: option }));
  });
}

afterEach(cleanup);

it("offers ten starting colours with white outlines", () => {
  expect(defaultPalette().map(({ fill }): string => fill)).toStrictEqual([
    "#C48CFF",
    "#7CDC8C",
    "#6CD4FF",
    "#FF9E5E",
    "#FF7FA8",
    "#F2D64B",
    "#5EE0C8",
    "#8FA8FF",
    "#E08CE0",
    "#B5E05E",
  ]);
  expect(new Set(defaultPalette().map(({ outline }): string => outline))).toStrictEqual(new Set(["#FFFFFF"]));
});

it("moves the captions to another anchor", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Above the face" }));
  expect(lastSaved(store)?.placement).toStrictEqual({
    anchor: "face_above",
    offset_x_percent: 1,
    offset_y_percent: 2,
  });
});

it("keeps the anchor when the active choice is pressed again", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Bottom of the video" }));
  expect(vi.mocked(store.updateSettings).mock.calls).toStrictEqual([]);
});

it("reports the horizontal fine tuning", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.keyDown(screen.getByRole("slider", { name: "Fine-tune left and right (percent of width)" }), {
    key: "ArrowRight",
  });
  expect(lastSaved(store)?.placement.offset_x_percent).toBe(1.5);
});

it("reports the vertical fine tuning", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.keyDown(screen.getByRole("slider", { name: "Fine-tune up and down (percent of height)" }), {
    key: "ArrowLeft",
  });
  expect(lastSaved(store)?.placement.offset_y_percent).toBe(1.5);
});

it("reports the face model", async () => {
  const store: AppStore = show(SETTINGS);
  await pressAndChoose("Face detection model", "YOLO");
  expect(lastSaved(store)?.face_model).toBe("yolo");
});

it("reports the spoken language", async () => {
  const store: AppStore = show(SETTINGS);
  await pressAndChoose("Spoken language", "日本語");
  expect(lastSaved(store)?.language).toBe("ja");
});

it("converts the text size to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("textbox", { name: "Text size (percent of the short edge)" }), {
    target: { value: "10" },
  });
  expect(lastSaved(store)?.font_ratio).toBe(0.1);
});

it("converts the caption width to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("textbox", { name: "Longest line (percent of the width)" }), {
    target: { value: "50" },
  });
  expect(lastSaved(store)?.width_ratio).toBe(0.5);
});

it("converts the outline width to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("textbox", { name: "Outline thickness (percent of the text)" }), {
    target: { value: "20" },
  });
  expect(lastSaved(store)?.outline_ratio).toBe(0.2);
});

it("describes the preview in the landscape frame by default", () => {
  show(SETTINGS);
  expect(descriptionText()).toBe("Sample caption at 51% across and 90% down the 1920×1080 frame.");
});

it("shows the vertical frame narrower", () => {
  show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Vertical 1080×1920" }));
  expect(descriptionText()).toBe("Sample caption at 51% across and 90% down the Vertical 1080×1920 frame.");
});

it("shows the 4K frame", () => {
  show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "4K" }));
  expect(descriptionText()).toBe("Sample caption at 51% across and 90% down the 4K frame.");
});

it("draws a face only for the face anchors", () => {
  expect(
    render(
      <StylePane
        store={storeOf()}
        settings={{ ...SETTINGS, placement: { ...SETTINGS.placement, anchor: "face_below" } }}
        faceOptions={[]}
      />,
    ).container.querySelectorAll(".rounded-full.bg-neutral-400").length,
  ).toBe(1);
  cleanup();
  expect(
    render(<StylePane store={storeOf()} settings={SETTINGS} faceOptions={[]} />).container.querySelectorAll(
      ".rounded-full.bg-neutral-400",
    ).length,
  ).toBe(0);
});

it("draws the sample in the first speaker's colours", () => {
  const sample: HTMLElement | null = render(
    <StylePane store={storeOf()} settings={SETTINGS} faceOptions={[]} />,
  ).container.querySelector("div.outlined-text");
  expect(sample?.style.color).toBe("#111111");
});

it("still draws the sample with an empty palette", () => {
  show({ ...SETTINGS, palette: [] });
  expect(descriptionText()).toBe("Sample caption at 51% across and 90% down the 1920×1080 frame.");
  expect(screen.queryByRole("button", { name: "Remove a speaker" })).toBe(null);
});

it("changes the fill of one speaker, in capitals", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByLabelText("Fill of speaker 2"), { target: { value: "#abcdef" } });
  expect(lastSaved(store)?.palette).toStrictEqual([
    { fill: "#111111", outline: "#222222" },
    { fill: "#ABCDEF", outline: "#444444" },
  ]);
});

it("changes the outline of one speaker, in capitals", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByLabelText("Outline of speaker 1"), { target: { value: "#abcdef" } });
  expect(lastSaved(store)?.palette).toStrictEqual([
    { fill: "#111111", outline: "#ABCDEF" },
    { fill: "#333333", outline: "#444444" },
  ]);
});

it("adds a white speaker", () => {
  const store: AppStore = show(SETTINGS);
  expect(screen.getByRole("heading", { name: "Speaker colours (up to 2 speakers)" }).tagName).toBe("H3");
  fireEvent.click(screen.getByRole("button", { name: "Add a speaker" }));
  expect(lastSaved(store)?.palette).toStrictEqual([
    { fill: "#111111", outline: "#222222" },
    { fill: "#333333", outline: "#444444" },
    { fill: "#FFFFFF", outline: "#FFFFFF" },
  ]);
});

it("removes the last speaker", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Remove a speaker" }));
  expect(lastSaved(store)?.palette).toStrictEqual([{ fill: "#111111", outline: "#222222" }]);
});

it("keeps at least one speaker", () => {
  show({ ...SETTINGS, palette: [{ fill: "#111111", outline: "#222222" }] });
  expect(screen.queryByRole("button", { name: "Remove a speaker" })).toBe(null);
});

it("restores the first colours", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Restore the first colours" }));
  expect(lastSaved(store)?.palette.length).toBe(10);
});
