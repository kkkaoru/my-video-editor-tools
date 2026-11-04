// Runs with bun: a card is built from its slotted parts.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./card.tsx";

afterEach(cleanup);

it("gives each part its slot and element", () => {
  render(
    <Card>
      <CardHeader>
        <CardTitle>Title</CardTitle>
        <CardDescription>Hint</CardDescription>
      </CardHeader>
      <CardContent>Body</CardContent>
    </Card>,
  );
  expect(screen.getByRole("heading", { level: 2, name: "Title" }).getAttribute("data-slot")).toBe(
    "card-title",
  );
  expect(screen.getByText("Title").parentElement?.getAttribute("data-slot")).toBe("card-header");
  expect(screen.getByText("Hint").tagName).toBe("P");
  expect(screen.getByText("Hint").getAttribute("data-slot")).toBe("card-description");
  expect(screen.getByText("Body").getAttribute("data-slot")).toBe("card-content");
  expect(screen.getByText("Body").parentElement?.getAttribute("data-slot")).toBe("card");
});

it("merges a caller's classes on the content", () => {
  render(<CardContent className="px-8">Body</CardContent>);
  expect(screen.getByText("Body").className).toBe("min-w-0 px-8");
});
