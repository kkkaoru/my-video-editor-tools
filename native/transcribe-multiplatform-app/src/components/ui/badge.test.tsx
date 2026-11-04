// Runs with bun: each badge look.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Badge } from "./badge.tsx";

afterEach(cleanup);

it("uses the outline look by default", () => {
  render(<Badge>New</Badge>);
  expect(screen.getByText("New").className).toBe(
    "inline-flex min-h-6 w-fit shrink-0 items-center justify-center gap-1 rounded-4xl border px-2 py-0.5 text-xs font-medium whitespace-nowrap border-border text-foreground",
  );
});

it("shows the default variant", () => {
  render(<Badge variant="default">New</Badge>);
  expect(screen.getByText("New").className).toBe(
    "inline-flex min-h-6 w-fit shrink-0 items-center justify-center gap-1 rounded-4xl border px-2 py-0.5 text-xs font-medium whitespace-nowrap border-transparent bg-primary text-primary-foreground",
  );
});

it("shows the success variant", () => {
  render(<Badge variant="success">New</Badge>);
  expect(screen.getByText("New").className).toBe(
    "inline-flex min-h-6 w-fit shrink-0 items-center justify-center gap-1 rounded-4xl border px-2 py-0.5 text-xs font-medium whitespace-nowrap border-success text-success",
  );
});

it("shows the destructive variant", () => {
  render(
    <Badge variant="destructive" className="w-full">
      New
    </Badge>,
  );
  expect(screen.getByText("New").className).toBe(
    "inline-flex min-h-6 shrink-0 items-center justify-center gap-1 rounded-4xl border px-2 py-0.5 text-xs font-medium whitespace-nowrap border-destructive text-destructive w-full",
  );
});
