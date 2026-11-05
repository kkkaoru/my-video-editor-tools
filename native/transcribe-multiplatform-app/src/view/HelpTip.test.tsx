// Runs with bun: the explanation opens from the button, and names itself for assistive technology.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { HelpTip } from "./HelpTip.tsx";

afterEach(cleanup);

it("stays closed until asked", () => {
  render(
    <main>
      <HelpTip label="Why?">Because.</HelpTip>
    </main>,
  );
  expect(screen.getByRole("button", { name: "Why?" }).textContent).toBe("?");
  expect(screen.queryByText("Because.")).toBe(null);
});

it("opens on click with a named popup", async () => {
  render(
    <main>
      <HelpTip label="Why?">Because.</HelpTip>
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Why?" }));
  });
  expect(screen.getByRole("dialog", { name: "Why?" }).textContent).toBe("Because.");
});
