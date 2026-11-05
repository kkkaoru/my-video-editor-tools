// Runs with bun: choosing a tab shows its panel.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs.tsx";

afterEach(cleanup);

it("shows the panel of the chosen tab", async () => {
  render(
    <Tabs defaultValue="one" className="p-1">
      <TabsList className="p-2">
        <TabsTrigger value="one" className="p-3">
          One
        </TabsTrigger>
        <TabsTrigger value="two">Two</TabsTrigger>
      </TabsList>
      <TabsContent value="one" className="p-4">
        First
      </TabsContent>
      <TabsContent value="two">Second</TabsContent>
    </Tabs>,
  );
  expect(screen.getByRole("tabpanel").textContent).toBe("First");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "Two" }));
  });
  expect(screen.getByRole("tab", { name: "Two" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByText("Second").getAttribute("role")).toBe("tabpanel");
});
