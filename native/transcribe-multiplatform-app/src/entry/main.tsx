// Runs with bun: loads the first snapshot, then mounts the app.
import { render } from "preact";
import { fetchSnapshot } from "../backend/backend.ts";
import type { Snapshot } from "../backend/types.ts";
import { App } from "../view/App.tsx";
import "./styles.css";

export function mount(host: HTMLElement, snapshot: Snapshot): void {
  render(<App initial={snapshot} />, host);
}

export function reportStartupFailure(host: HTMLElement, error: unknown): void {
  const message: HTMLElement = document.createElement("p");
  message.setAttribute("role", "alert");
  message.textContent = `Startup failed: ${String(error)}`;
  host.replaceChildren(message);
}

export async function start(host: HTMLElement | null): Promise<void> {
  if (host === null) {
    throw new Error("the #app element is missing");
  }
  await fetchSnapshot().then(
    (snapshot: Snapshot): void => mount(host, snapshot),
    (error: unknown): void => reportStartupFailure(host, error),
  );
}

void start(document.getElementById("app"));
