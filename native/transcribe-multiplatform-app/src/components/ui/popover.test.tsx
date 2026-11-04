// Runs with bun: the popover opens from its trigger, inside the main landmark.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Popover, PopoverContent, PopoverTrigger } from "./popover.tsx";

afterEach(cleanup);

it("opens inside main with a caller's classes", async () => {
  render(
    <main>
      <Popover>
        <PopoverTrigger aria-label="Why?" className="size-12">
          ?
        </PopoverTrigger>
        <PopoverContent aria-label="Why?" className="w-96">
          Because.
        </PopoverContent>
      </Popover>
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Why?" }));
  });
  const popup: HTMLElement = screen.getByRole("dialog", { name: "Why?" });
  expect(popup.closest("main")?.tagName).toBe("MAIN");
  expect(popup.className).toBe(
    "max-w-[calc(100vw-1rem)] rounded-lg border border-border bg-popover p-3 text-sm text-popover-foreground shadow-md w-96",
  );
});
