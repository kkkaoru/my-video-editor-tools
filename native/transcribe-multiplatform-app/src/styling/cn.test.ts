// Runs with bun: class names merge, with the later Tailwind utility winning.
import { expect, it } from "vitest";
import { cn } from "./cn.ts";

it("joins class names and skips empty ones", () => {
  expect(cn("a", false, undefined, "b")).toBe("a b");
});

it("lets a later Tailwind utility replace an earlier one", () => {
  expect(cn("px-2 py-1", "px-4")).toBe("py-1 px-4");
});
