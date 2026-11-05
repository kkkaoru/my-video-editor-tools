// Runs with bun: the accordion opens one row at a time and reports which.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { Accordion, AccordionItem, AccordionPanel, AccordionTrigger } from "./accordion.tsx";

afterEach(cleanup);

it("opens the chosen row, with a caller's classes on every part", () => {
  const onValueChange: Mock<(value: string[]) => void> = vi.fn();
  render(
    <Accordion value={["a"]} onValueChange={onValueChange}>
      <AccordionItem value="a" className="item-class">
        <AccordionTrigger className="trigger-class">First</AccordionTrigger>
        <AccordionPanel className="panel-class">First body</AccordionPanel>
      </AccordionItem>
      <AccordionItem value="b">
        <AccordionTrigger>Second</AccordionTrigger>
        <AccordionPanel>Second body</AccordionPanel>
      </AccordionItem>
    </Accordion>,
  );
  expect(screen.getByText("First body").getAttribute("data-slot")).toBe("accordion-panel");
  expect(screen.getByRole("button", { name: "First" }).getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByRole("button", { name: "Second" }).getAttribute("aria-expanded")).toBe("false");
  expect(screen.getByRole("button", { name: "First" }).className.endsWith("trigger-class")).toBe(true);
  expect(screen.getByText("First body").className).toBe("pb-4 panel-class");
  fireEvent.click(screen.getByRole("button", { name: "Second" }));
  expect(onValueChange.mock.calls[0]?.[0]).toStrictEqual(["b"]);
});
