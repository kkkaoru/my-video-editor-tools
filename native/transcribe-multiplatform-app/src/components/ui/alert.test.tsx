// Runs with bun: an alert is announced and accepts a caller's classes.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Alert } from "./alert.tsx";

afterEach(cleanup);

it("announces its text", () => {
  render(<Alert>Failed</Alert>);
  expect(screen.getByRole("alert").textContent).toBe("Failed");
});

it("merges a caller's classes over its own", () => {
  render(<Alert className="w-fit">Failed</Alert>);
  expect(screen.getByRole("alert").className).toBe(
    "flex gap-2 rounded-lg border border-destructive bg-card px-3 py-2 text-left text-sm break-words whitespace-pre-wrap text-destructive [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0 w-fit",
  );
});
