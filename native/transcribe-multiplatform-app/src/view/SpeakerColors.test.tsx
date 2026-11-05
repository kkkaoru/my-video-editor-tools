// Runs with bun: each speaker's row opens one at a time and edits that speaker's layers.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import type { SpeakerColor, SubtitleFormat } from "../backend/types.ts";
import { defaultPalette, SpeakerColors } from "./SpeakerColors.tsx";

const SPEAKER_ONE: SpeakerColor = {
  fill: "#111111",
  fill_width: 0,
  outline: "#EEEEEE",
  outline_width: 0.1,
  rim: "#123456",
  rim_width: 0.03,
  shadow: "#654321",
  shadow_x: 0.03,
  shadow_y: -0.03,
  shadow_blur: 5,
};
const SPEAKER_TWO: SpeakerColor = { ...SPEAKER_ONE, fill: "#333333", rim: null, shadow: null };
const UNKNOWN: SpeakerColor = { ...SPEAKER_ONE, fill: "#555555" };

interface Shown {
  onChange: Mock<(palette: SpeakerColor[]) => void>;
  onUnassignedChange: Mock<(color: SpeakerColor) => void>;
  onOpenSlotChange: Mock<(slot: number | null) => void>;
}

interface ShowOptions {
  palette: SpeakerColor[];
  openSlot: number | null;
  subtitleFormat: SubtitleFormat;
}

function show({ palette, openSlot, subtitleFormat }: ShowOptions): Shown {
  const shown: Shown = { onChange: vi.fn(), onUnassignedChange: vi.fn(), onOpenSlotChange: vi.fn() };
  render(
    <main>
      <SpeakerColors
        palette={palette}
        unassigned={UNKNOWN}
        fontFamily="Test Font"
        subtitleFormat={subtitleFormat}
        openSlot={openSlot}
        onOpenSlotChange={shown.onOpenSlotChange}
        onChange={shown.onChange}
        onUnassignedChange={shown.onUnassignedChange}
      />
    </main>,
  );
  return shown;
}

const firstOpen = (): Shown =>
  show({ palette: [SPEAKER_ONE, SPEAKER_TWO], openSlot: 0, subtitleFormat: "drt" });

const typeInto = (name: string, value: string): void => {
  fireEvent.input(screen.getByRole("spinbutton", { name }), { target: { value } });
};

afterEach(cleanup);

it("starts speaker 1 white on black and the others in colour on white, all with a transparent outer edge", () => {
  const later = (fill: string): SpeakerColor => ({
    fill,
    fill_width: 0,
    outline: "#FFFFFF",
    outline_width: 0.115,
    rim: null,
    rim_width: 3 / 88,
    shadow: null,
    shadow_x: 0.03,
    shadow_y: -0.03,
    shadow_blur: 5,
  });
  expect(defaultPalette()).toStrictEqual([
    {
      fill: "#FFFFFF",
      fill_width: 0,
      outline: "#000000",
      outline_width: 0.115,
      rim: null,
      rim_width: 3 / 88,
      shadow: null,
      shadow_x: 0.03,
      shadow_y: -0.03,
      shadow_blur: 5,
    },
    later("#005A9C"),
    later("#6A1FA8"),
    later("#0B672C"),
    later("#A3123A"),
    later("#8A4500"),
    later("#7A1D7A"),
    later("#006262"),
  ]);
});

it("lists every speaker and the unknown one as short rows, opening only the one asked for", () => {
  firstOpen();
  expect(
    Array.from(
      document.querySelectorAll('[data-slot="speaker-name"]'),
      ({ textContent }: Element) => textContent,
    ),
  ).toStrictEqual(["Speaker 1", "Speaker 2", "Unknown speaker"]);
  expect(screen.getAllByRole("button", { name: /^Speaker 1: Fill/ }).length).toBe(1);
  expect(screen.queryByRole("button", { name: /^Speaker 2: Fill/ })).toBe(null);
});

it("reports the row opened and closed", () => {
  const { onOpenSlotChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
  expect(onOpenSlotChange.mock.lastCall).toStrictEqual([1]);
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1" }));
  expect(onOpenSlotChange.mock.lastCall).toStrictEqual([null]);
});

it("opens nothing when no row is chosen", () => {
  show({ palette: [SPEAKER_ONE], openSlot: null, subtitleFormat: "drt" });
  expect(screen.queryByRole("button", { name: /^Speaker 1: Fill/ })).toBe(null);
});

it("keeps every row in the same order", () => {
  firstOpen();
  expect(
    Array.from(
      document.querySelectorAll('[data-slot="layer-row"] > span:first-child'),
      ({ textContent }: Element) => textContent,
    ),
  ).toStrictEqual([
    "Fill",
    "Outline",
    "Outer edge",
    "Shadow colour",
    "Shadow left/right",
    "Shadow up/down",
    "Shadow blur",
  ]);
});

it("saves each layer's thickness as a ratio of the text", () => {
  const { onChange } = firstOpen();
  typeInto("Speaker 1: Fill thickness (percent of the text size)", "2");
  expect(onChange.mock.lastCall?.[0][0]?.fill_width).toBe(0.02);
  typeInto("Speaker 1: Outline thickness (percent of the text size)", "20");
  expect(onChange.mock.lastCall?.[0][0]?.outline_width).toBe(0.2);
  typeInto("Speaker 1: Outer edge thickness (percent of the text size)", "5");
  expect(onChange.mock.lastCall?.[0][0]?.rim_width).toBe(0.05);
});

it("shows the thickness as a percent", () => {
  firstOpen();
  expect(
    screen.getByRole<HTMLInputElement>("spinbutton", {
      name: "Speaker 1: Outline thickness (percent of the text size)",
    }).value,
  ).toBe("10");
});

it("saves the shadow's position as a percent of the text, and its blur as it is", () => {
  const { onChange } = firstOpen();
  typeInto("Speaker 1: Shadow left/right (percent of the text size)", "4");
  expect(onChange.mock.lastCall?.[0][0]?.shadow_x).toBe(0.02);
  typeInto("Speaker 1: Shadow up/down (percent of the text size; minus is down)", "-6");
  expect(onChange.mock.lastCall?.[0][0]?.shadow_y).toBe(-0.03);
  typeInto("Speaker 1: Shadow blur (0 to 50)", "20");
  expect(onChange.mock.lastCall?.[0][0]?.shadow_blur).toBe(20);
});

it("turns off what a transparent colour has no use for", () => {
  show({ palette: [SPEAKER_ONE, SPEAKER_TWO], openSlot: 1, subtitleFormat: "drt" });
  const disabled = (name: string): boolean =>
    screen.getByRole<HTMLInputElement>("spinbutton", { name }).disabled;
  expect(screen.getByRole("button", { name: "Speaker 2: Outer edge: Transparent" }).dataset.slot).toBe(
    "color-swatch",
  );
  expect(disabled("Speaker 2: Outer edge thickness (percent of the text size)")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Speaker 2: Shadow" }));
  expect(screen.getByRole("button", { name: "Speaker 2: Shadow colour: Transparent" }).dataset.slot).toBe(
    "color-swatch",
  );
  expect(disabled("Speaker 2: Shadow left/right (percent of the text size)")).toBe(true);
  expect(disabled("Speaker 2: Shadow up/down (percent of the text size; minus is down)")).toBe(true);
  expect(disabled("Speaker 2: Shadow blur (0 to 50)")).toBe(true);
  expect(disabled("Speaker 2: Fill thickness (percent of the text size)")).toBe(false);
});

it("lets the fill and the outline be a colour only", () => {
  firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Fill: #111111" }));
  expect(screen.queryByRole("button", { name: "Transparent" })).toBe(null);
});

it("changes one speaker's colours, leaving the others", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Outline: #EEEEEE" }));
  fireEvent.input(screen.getByLabelText("Any colour"), { target: { value: "#abcdef" } });
  expect(onChange.mock.lastCall).toStrictEqual([[{ ...SPEAKER_ONE, outline: "#ABCDEF" }, SPEAKER_TWO]]);
});

it("makes the outer edge transparent from its colour choices", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Outer edge: #123456" }));
  fireEvent.click(screen.getByRole("button", { name: "Transparent" }));
  expect(onChange.mock.lastCall?.[0][0]?.rim).toBe(null);
});

it("makes the shadow transparent, and any colour brings it back", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Shadow colour: #654321" }));
  fireEvent.click(screen.getByRole("button", { name: "Transparent" }));
  expect(onChange.mock.lastCall?.[0][0]?.shadow).toBe(null);
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Shadow colour: #654321" }));
  fireEvent.input(screen.getByLabelText("Any colour"), { target: { value: "#000001" } });
  expect(onChange.mock.lastCall?.[0][0]?.shadow).toBe("#000001");
});

it("changes the fill colour", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Speaker 1: Fill: #111111" }));
  fireEvent.input(screen.getByLabelText("Any colour"), { target: { value: "#abcdef" } });
  expect(onChange.mock.lastCall?.[0][0]?.fill).toBe("#ABCDEF");
});

it("changes the unknown speaker apart from the palette", () => {
  const { onChange, onUnassignedChange } = show({
    palette: [SPEAKER_ONE],
    openSlot: 1,
    subtitleFormat: "drt",
  });
  typeInto("Unknown speaker: Fill thickness (percent of the text size)", "2");
  expect(onUnassignedChange.mock.lastCall).toStrictEqual([{ ...UNKNOWN, fill_width: 0.02 }]);
  expect(onChange.mock.calls).toStrictEqual([]);
});

it("warns about a pair that is hard to read, even with its row closed", () => {
  show({
    palette: [{ ...SPEAKER_ONE, fill: "#777777", outline: "#888888" }],
    openSlot: null,
    subtitleFormat: "drt",
  });
  expect(screen.getByRole("alert").textContent).toMatch(/Speaker 1/);
});

it("gives no warning for a readable pair", () => {
  firstOpen();
  expect(screen.queryByRole("alert")).toBe(null);
});

it("adds a speaker with a transparent outer edge", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Add a speaker" }));
  expect(onChange.mock.lastCall?.[0][2]).toStrictEqual({
    fill: "#FFFFFF",
    fill_width: 0,
    outline: "#FFFFFF",
    outline_width: 0.115,
    rim: null,
    rim_width: 3 / 88,
    shadow: null,
    shadow_x: 0.03,
    shadow_y: -0.03,
    shadow_blur: 5,
  });
});

it("removes the last speaker", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Remove a speaker" }));
  expect(onChange.mock.lastCall).toStrictEqual([[SPEAKER_ONE]]);
});

it("keeps at least one speaker", () => {
  show({ palette: [SPEAKER_ONE], openSlot: 0, subtitleFormat: "drt" });
  expect(screen.queryByRole("button", { name: "Remove a speaker" })).toBe(null);
});

it("stops adding speakers at the eight the speaker models can tell apart", () => {
  show({ palette: defaultPalette(), openSlot: 0, subtitleFormat: "drt" });
  expect(screen.queryByRole("button", { name: "Add a speaker" })).toBe(null);
});

it("restores the first colours", () => {
  const { onChange } = firstOpen();
  fireEvent.click(screen.getByRole("button", { name: "Restore the first colours" }));
  expect(onChange.mock.lastCall?.[0].length).toBe(8);
});

it("says what the chosen format cannot show", () => {
  show({ palette: [SPEAKER_ONE], openSlot: 0, subtitleFormat: "srt" });
  expect(
    screen.getByText(
      "SRT keeps only the text colour: no outlines, outer edges or shadows. Choose DRT to use them.",
    ).tagName,
  ).toBe("P");
});

it("says nothing of limits for the DRT format", () => {
  firstOpen();
  expect(screen.queryByText("FCPXML shows no shadow. Choose DRT to use one.")).toBe(null);
});

it("moves a thickness by its slider too", () => {
  const { onChange } = firstOpen();
  fireEvent.keyDown(
    screen.getByLabelText("Speaker 1: Outline thickness (percent of the text size)", {
      selector: "input[type=range]",
    }),
    { key: "ArrowRight" },
  );
  expect(onChange.mock.lastCall?.[0][0]?.outline_width).toBe(0.105);
});

it("hides a later speaker's unused shadow behind one control that says whether it is open", () => {
  show({ palette: [SPEAKER_ONE, SPEAKER_TWO], openSlot: 1, subtitleFormat: "drt" });
  const toggle: HTMLElement = screen.getByRole("button", { name: "Speaker 2: Shadow" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("spinbutton", { name: "Speaker 2: Shadow blur (0 to 50)" })).toBe(null);
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByRole("button", { name: "Speaker 2: Shadow colour: Transparent" }).dataset.slot).toBe(
    "color-swatch",
  );
  expect(
    screen.getByRole<HTMLInputElement>("spinbutton", { name: "Speaker 2: Shadow blur (0 to 50)" }).disabled,
  ).toBe(true);
});

it("always shows the first speaker's shadow, with no control to hide it", () => {
  firstOpen();
  expect(screen.queryByRole("button", { name: "Speaker 1: Shadow" })).toBe(null);
  expect(screen.getByRole("button", { name: "Speaker 1: Shadow colour: #654321" }).dataset.slot).toBe(
    "color-swatch",
  );
});

it("never hides a later speaker's shadow that is in use", () => {
  show({ palette: [SPEAKER_ONE, { ...SPEAKER_TWO, shadow: "#112233" }], openSlot: 1, subtitleFormat: "drt" });
  expect(screen.queryByRole("button", { name: "Speaker 2: Shadow" })).toBe(null);
  expect(screen.getByRole("button", { name: "Speaker 2: Shadow colour: #112233" }).dataset.slot).toBe(
    "color-swatch",
  );
});

it("stays open when a later speaker's shadow goes back to transparent", () => {
  const speaker: SpeakerColor = { ...SPEAKER_TWO, shadow: "#112233" };
  const { onChange } = show({ palette: [SPEAKER_ONE, speaker], openSlot: 1, subtitleFormat: "drt" });
  fireEvent.click(screen.getByRole("button", { name: "Speaker 2: Shadow colour: #112233" }));
  fireEvent.click(screen.getByRole("button", { name: "Transparent" }));
  expect(onChange.mock.lastCall?.[0][1]?.shadow).toBe(null);
});

it("groups the shadow's settings under one name for a screen reader", () => {
  firstOpen();
  const group: HTMLElement = screen.getByRole("group", { name: "Speaker 1: Shadow" });
  expect(group.querySelectorAll('[data-slot="layer-row"]').length).toBe(4);
});

it("heads the first speaker's shadow settings like a later speaker's", () => {
  firstOpen();
  expect(screen.getByText("Shadow").tagName).toBe("SPAN");
});

it("heads the columns, saying the number is a thickness relative to the text", () => {
  firstOpen();
  expect(
    Array.from(
      document.querySelector('[data-slot="layer-heads"]')?.children ?? [],
      ({ textContent }: Element) => textContent,
    ),
  ).toStrictEqual(["", "Colour", "Thickness (percent of the text size)"]);
});
