// Runs with bun: every style control reports its change, and the preview follows the settings.
import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import { initialState } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { defaultPalette, StylePane } from "./StylePane.tsx";

// The edge-aligned thumb stays visibility:hidden until measured and happy-dom measures nothing,
// which also empties its accessible name, so its input is found by the label attribute.
const SLIDER_INPUT: { selector: string } = { selector: "input[type=range]" };
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
  safe_left_percent: 5,
  safe_right_percent: 5,
  placement: {
    anchor: "video_bottom",
    offset_x_percent: 1,
    offset_y_percent: 2,
  },
  language: "auto",
  diarize_model: "b",
  face_model: "yunet",
  model_dirs: [],
  active_profile: "",
  rim_ratio: 0.034,
  line_height: 1.448,
  speaker_count: 0,
  subtitle_format: "fcpxml",
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

function show(settings: Settings): AppStore {
  const store: AppStore = storeOf();
  render(
    <main>
      <StylePane
        store={store}
        settings={settings}
        profiles={[]}
        faceOptions={[
          { value: "yunet", label: "YuNet", disabled: false },
          { value: "yolo", label: "YOLO", disabled: false },
        ]}
      />
    </main>,
  );
  return store;
}

const lastSaved = (store: AppStore): Settings | undefined =>
  vi.mocked(store.updateSettings).mock.lastCall?.[0];

const descriptionText = (): string | null | undefined => screen.getByText(/Preview caption at/).textContent;

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

it("starts speaker 1 white on black and the others in colour on white", () => {
  expect(defaultPalette()).toStrictEqual([
    { fill: "#FFFFFF", outline: "#000000" },
    { fill: "#005A9C", outline: "#FFFFFF" },
    { fill: "#6A1FA8", outline: "#FFFFFF" },
    { fill: "#0B672C", outline: "#FFFFFF" },
    { fill: "#A3123A", outline: "#FFFFFF" },
    { fill: "#8A4500", outline: "#FFFFFF" },
    { fill: "#7A1D7A", outline: "#FFFFFF" },
    { fill: "#006262", outline: "#FFFFFF" },
  ]);
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
  fireEvent.keyDown(screen.getByLabelText("Fine-tune left and right (percent of width)", SLIDER_INPUT), {
    key: "ArrowRight",
  });
  expect(lastSaved(store)?.placement.offset_x_percent).toBe(1.5);
});

it("reports the vertical fine tuning", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.keyDown(screen.getByLabelText("Fine-tune up and down (percent of height)", SLIDER_INPUT), {
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
  fireEvent.keyDown(screen.getByLabelText("Text size (percent of the short edge)", SLIDER_INPUT), {
    key: "ArrowRight",
  });
  expect(lastSaved(store)?.font_ratio).toBeCloseTo(0.052);
});

it("converts the caption width to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Longest line (percent of the width)" }), {
    target: { value: "50" },
  });
  expect(lastSaved(store)?.width_ratio).toBe(0.5);
});

it("converts the outline width to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Outline thickness (percent of the text)" }), {
    target: { value: "20" },
  });
  expect(lastSaved(store)?.outline_ratio).toBe(0.2);
});

it("reports the line spacing as it is", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.keyDown(screen.getByLabelText("Line spacing (times the text size)", SLIDER_INPUT), {
    key: "ArrowRight",
  });
  expect(lastSaved(store)?.line_height).toBeCloseTo(1.449);
});

it("converts the rim width to a ratio", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(
    screen.getByRole("spinbutton", { name: "Black rim outside the outline (percent of the text)" }),
    { target: { value: "5" } },
  );
  expect(lastSaved(store)?.rim_ratio).toBe(0.05);
});

it("describes the preview in the landscape frame by default", () => {
  show(SETTINGS);
  expect(descriptionText()).toBe(
    "Preview caption at 51% across and 83% down the 1920×1080 frame. Text size at this resolution: 54 px.",
  );
});

it("shows the vertical frame narrower", () => {
  show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Vertical 1080×1920" }));
  expect(descriptionText()).toBe(
    "Preview caption at 51% across and 77% down the Vertical 1080×1920 frame. Text size at this resolution: 54 px.",
  );
});

it("shows the 4K frame", () => {
  show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "4K" }));
  expect(descriptionText()).toBe(
    "Preview caption at 51% across and 75% down the 4K frame. Text size at this resolution: 108 px.",
  );
});

it("draws a face only for the face anchors", () => {
  expect(
    render(
      <StylePane
        store={storeOf()}
        settings={{ ...SETTINGS, placement: { ...SETTINGS.placement, anchor: "face_below" } }}
        profiles={[]}
        faceOptions={[]}
      />,
    ).container.querySelectorAll("ellipse").length,
  ).toBe(2);
  cleanup();
  expect(
    render(
      <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
    ).container.querySelectorAll("ellipse").length,
  ).toBe(0);
});

it("draws the sample with the first speaker's colours at its place in the frame", () => {
  const { container } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  expect(container.querySelector("g g:last-child")?.getAttribute("fill")).toBe("#111111");
  expect(Number.parseFloat(container.querySelector("text")?.getAttribute("x") ?? "")).toBeCloseTo(979.2);
});

it("warns when a speaker's fill and outline are below AA", () => {
  show({ ...SETTINGS, palette: [{ fill: "#777777", outline: "#888888" }] });
  expect(screen.getByRole("alert").textContent).toMatch(/Speaker 1/);
});

it("gives no warning for a readable pair", () => {
  show({
    ...SETTINGS,
    unassigned: { fill: "#FFFFFF", outline: "#000000" },
    palette: [{ fill: "#FFFFFF", outline: "#000000" }],
  });
  expect(screen.queryByRole("alert")).toBe(null);
});

it("changes the unknown speaker's colours", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByLabelText("Unknown speaker: fill"), { target: { value: "#abcdef" } });
  expect(lastSaved(store)?.unassigned).toStrictEqual({ fill: "#ABCDEF", outline: "#000000" });
});

it("still draws the sample with an empty palette", () => {
  show({ ...SETTINGS, palette: [] });
  expect(descriptionText()).toBe(
    "Preview caption at 51% across and 83% down the 1920×1080 frame. Text size at this resolution: 54 px.",
  );
  expect(screen.queryByRole("button", { name: "Remove a speaker" })).toBe(null);
});

it("changes the fill of one speaker, in capitals", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByLabelText("Speaker 2: fill"), { target: { value: "#abcdef" } });
  expect(lastSaved(store)?.palette).toStrictEqual([
    { fill: "#111111", outline: "#222222" },
    { fill: "#ABCDEF", outline: "#444444" },
  ]);
});

it("changes the outline of one speaker, in capitals", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByLabelText("Speaker 1: outline"), { target: { value: "#abcdef" } });
  expect(lastSaved(store)?.palette).toStrictEqual([
    { fill: "#111111", outline: "#ABCDEF" },
    { fill: "#333333", outline: "#444444" },
  ]);
});

it("adds a white speaker", () => {
  const store: AppStore = show(SETTINGS);
  expect(screen.getByRole("heading", { name: "Speaker colours (up to 2 speakers)" }).tagName).toBe("H2");
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

it("stops adding speakers at the eight the speaker models can tell apart", () => {
  show({ ...SETTINGS, palette: defaultPalette() });
  expect(screen.queryByRole("button", { name: "Add a speaker" })).toBe(null);
});

it("restores the first colours", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.click(screen.getByRole("button", { name: "Restore the first colours" }));
  expect(lastSaved(store)?.palette.length).toBe(8);
});

it("lists the unknown speaker after every numbered speaker", () => {
  const { container } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  expect(
    Array.from(new Set(container.querySelectorAll('input[type="color"] ~ span.whitespace-nowrap'))).map(
      (name: Element): string | null => name.textContent,
    ),
  ).toStrictEqual(["Speaker 1", "Speaker 2", "Unknown speaker"]);
});

it("draws the preview and the colour samples in the chosen font", () => {
  const { container } = render(
    <StylePane
      store={storeOf()}
      settings={{ ...SETTINGS, font_family: "Hiragino Sans" }}
      profiles={[]}
      faceOptions={[]}
    />,
  );
  expect(
    new Set(
      Array.from(container.querySelectorAll("g[font-family]")).map((group: Element) =>
        group.getAttribute("font-family"),
      ),
    ),
  ).toStrictEqual(new Set(['"Hiragino Sans", "Noto Sans JP", "Hiragino Sans", "Yu Gothic UI", sans-serif']));
});

it("saves the chosen font family", async () => {
  vi.mocked(invoke).mockResolvedValue(["Arial"]);
  const store: AppStore = show(SETTINGS);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Font family" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("option", { name: "Arial" }));
  });
  expect(lastSaved(store)?.font_family).toBe("Arial");
});

it("reports a font list that cannot be read", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("no fonts"));
  const store: AppStore = show(SETTINGS);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Font family" }));
  });
  expect(vi.mocked(store.setState).mock.calls[0]?.[0](initialState(SNAPSHOT)).loadError).toStrictEqual({
    code: "internal",
    detail: "Error: no fonts",
  });
});

it("draws the person as a bust only for the face anchors", () => {
  const { container, rerender } = render(
    <StylePane
      store={storeOf()}
      settings={{ ...SETTINGS, placement: { ...SETTINGS.placement, anchor: "face_above" } }}
      profiles={[]}
      faceOptions={[]}
    />,
  );
  expect(container.querySelectorAll('[data-slot="face-silhouette"]').length).toBe(1);
  rerender(<StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />);
  expect(container.querySelectorAll('[data-slot="face-silhouette"]').length).toBe(0);
});

it("hides the decorative preview from assistive technology, and describes it in text", () => {
  const { container } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  expect(container.querySelector('[data-slot="caption-preview"] svg')?.getAttribute("aria-hidden")).toBe(
    "true",
  );
});

it("redraws the preview at once when a value changes", () => {
  const { container, rerender } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  const before: string | null | undefined =
    container.querySelector("g[font-size]")?.getAttribute("font-size") ?? null;
  rerender(
    <StylePane
      store={storeOf()}
      settings={{ ...SETTINGS, font_ratio: 0.1 }}
      profiles={[]}
      faceOptions={[]}
    />,
  );
  expect(before === container.querySelector("g[font-size]")?.getAttribute("font-size")).toBe(false);
});

it("puts the preview before the controls, so a narrow window shows it on top", () => {
  const { container } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  const preview: Element | null = container.querySelector('[data-slot="caption-preview"]');
  const firstSlider: Element | null = container.querySelector('[data-slot="slider"]');
  expect(
    preview !== null && firstSlider !== null
      ? preview.compareDocumentPosition(firstSlider) & Node.DOCUMENT_POSITION_FOLLOWING
      : 0,
  ).toBe(4);
});

it("saves the left and right safe zones", () => {
  const store: AppStore = show(SETTINGS);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Left safe zone (percent of width)" }), {
    target: { value: "12" },
  });
  expect(lastSaved(store)?.safe_left_percent).toBe(12);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Right safe zone (percent of width)" }), {
    target: { value: "7.5" },
  });
  expect(lastSaved(store)?.safe_right_percent).toBe(7.5);
});

it("marks both safe zones on the preview", () => {
  const { container } = render(
    <StylePane store={storeOf()} settings={SETTINGS} profiles={[]} faceOptions={[]} />,
  );
  expect(container.querySelectorAll('[data-slot="safe-zone"]').length).toBe(2);
});

it("draws nothing for a safe zone of no width", () => {
  const settings: Settings = {
    ...SETTINGS,
    safe_left_percent: 0,
    safe_right_percent: 5,
  };
  const { container } = render(
    <StylePane store={storeOf()} settings={settings} profiles={[]} faceOptions={[]} />,
  );
  expect(container.querySelectorAll('[data-slot="safe-zone"]').length).toBe(1);
});
