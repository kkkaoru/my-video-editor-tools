// Runs with bun: a button runs its action in each look.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { Button } from "./button.tsx";

afterEach(cleanup);

it("runs its action", () => {
  const onClick: Mock<() => void> = vi.fn();
  render(<Button onClick={onClick}>Go</Button>);
  fireEvent.click(screen.getByRole("button", { name: "Go" }));
  expect(onClick).toHaveBeenCalledTimes(1);
});

it("uses the solid look by default", () => {
  render(<Button>Go</Button>);
  expect(screen.getByRole("button").className).toBe(
    "inline-flex min-h-11 max-w-full min-w-11 items-center justify-center gap-2 rounded-lg border border-transparent px-4 py-2 text-left text-sm font-medium break-words transition-colors select-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 bg-primary text-primary-foreground hover:bg-primary-hover",
  );
});

it("shows the outline look with a caller's classes", () => {
  render(
    <Button variant="outline" className="w-fit">
      Go
    </Button>,
  );
  expect(screen.getByRole("button").className).toBe(
    "inline-flex min-h-11 max-w-full min-w-11 items-center justify-center gap-2 rounded-lg border px-4 py-2 text-left text-sm font-medium break-words transition-colors select-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 border-border bg-background text-foreground hover:bg-muted w-fit",
  );
});
