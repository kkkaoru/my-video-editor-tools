// Runs with bun: the collapsible says whether it is open, and shows its content only then.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "./collapsible.tsx";

afterEach(cleanup);

it("opens from its trigger, saying so", () => {
  render(
    <Collapsible>
      <CollapsibleTrigger>More</CollapsibleTrigger>
      <CollapsibleContent>Details</CollapsibleContent>
    </Collapsible>,
  );
  const trigger: HTMLElement = screen.getByRole("button", { name: "More" });
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText("Details")).toBe(null);
  fireEvent.click(trigger);
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByText("Details").getAttribute("data-slot")).toBe("collapsible-content");
});
