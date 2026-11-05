// Runs with bun: views read the language from the context, with English as the default.
import { cleanup, render, screen } from "@testing-library/preact";
import type { VNode } from "preact";
import { afterEach, expect, it } from "vitest";
import { jaMessages } from "./locale-ja.ts";
import { MessagesContext, useMessages } from "./use-messages.ts";

const AppName = (): VNode => <p>{useMessages().appName}</p>;

afterEach(cleanup);

it("defaults to English", () => {
  render(<AppName />);
  expect(screen.getByRole("paragraph").textContent).toBe("Transcribe Captions");
});

it("reads the language from the provider", () => {
  render(
    <MessagesContext.Provider value={{ ...jaMessages, appName: "翻訳" }}>
      <AppName />
    </MessagesContext.Provider>,
  );
  expect(screen.getByRole("paragraph").textContent).toBe("翻訳");
});
