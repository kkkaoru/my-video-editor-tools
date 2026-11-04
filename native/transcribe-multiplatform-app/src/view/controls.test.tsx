// Runs with bun: smoke.
import { render } from "@testing-library/preact";
import { expect, it } from "vitest";
import { ActionButton } from "./controls.tsx";
it("smoke", () => {
  const view = render(<ActionButton onClick={() => undefined}>Go</ActionButton>);
  expect(view.getByRole("button").textContent).toBe("Go");
});
