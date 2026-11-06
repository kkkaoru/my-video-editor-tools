// Runs with bun: the update on offer and the check on the settings tab say where things stand.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import type { BackendError, UpdateInfo } from "../backend/types.ts";
import { INITIAL_UPDATE, type UpdateState } from "../state/update-state.ts";
import { UpdateCheck, UpdateOffer } from "./UpdatePanels.tsx";

const INFO: UpdateInfo = { version: "2.0.0", notes: "Faster." };
const FAILURE: BackendError = { code: "internal", detail: "No network" };

afterEach(cleanup);

function offer(state: Partial<UpdateState>, busy: boolean): Mock<() => void> {
  const install: Mock<() => void> = vi.fn();
  render(
    <UpdateOffer
      updater={{ state: { ...INITIAL_UPDATE, available: INFO, ...state }, check: vi.fn(), install }}
      busy={busy}
    />,
  );
  return install;
}

function checker(state: Partial<UpdateState>): Mock<() => void> {
  const check: Mock<() => void> = vi.fn();
  render(<UpdateCheck updater={{ state: { ...INITIAL_UPDATE, ...state }, check, install: vi.fn() }} />);
  return check;
}

it("offers nothing without a new version", () => {
  const { container } = render(
    <UpdateOffer updater={{ state: INITIAL_UPDATE, check: vi.fn(), install: vi.fn() }} busy={false} />,
  );
  expect(container.childElementCount).toBe(0);
});

it("names the version and its notes, and installs only when pressed", () => {
  const install = offer({}, false);
  expect(screen.getByRole("heading", { name: "Version 2.0.0 is available" }).tagName).toBe("H2");
  expect(screen.getByText("Faster.").tagName).toBe("P");
  expect(install).toHaveBeenCalledTimes(0);
  fireEvent.click(screen.getByRole("button", { name: "Update and restart" }));
  expect(install).toHaveBeenCalledTimes(1);
});

it("leaves out notes the release does not have", () => {
  const { container } = render(
    <UpdateOffer
      updater={{
        state: { ...INITIAL_UPDATE, available: { ...INFO, notes: null } },
        check: vi.fn(),
        install: vi.fn(),
      }}
      busy={false}
    />,
  );
  expect(container.querySelector("p")).toBe(null);
});

it("leaves out empty notes", () => {
  const { container } = render(
    <UpdateOffer
      updater={{
        state: { ...INITIAL_UPDATE, available: { ...INFO, notes: "" } },
        check: vi.fn(),
        install: vi.fn(),
      }}
      busy={false}
    />,
  );
  expect(container.querySelector("p")).toBe(null);
});

it("holds the install back while videos are in progress, and says why", () => {
  offer({}, true);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Update and restart" }).disabled).toBe(true);
  expect(
    screen.getByText("You can update when the current videos and model downloads are finished.").tagName,
  ).toBe("P");
});

it("shows the download's share, and hides the button while it runs", () => {
  offer({ installing: true, progress: 0.25 }, false);
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Update download" });
  expect(bar.getAttribute("aria-valuenow")).toBe("0.25");
  expect(bar.getAttribute("aria-valuetext")).toBe("25%");
  expect(screen.getByRole("status").textContent).toBe(
    "Downloading the update. The app restarts when it is ready.",
  );
  expect(screen.queryByRole("button", { name: "Update and restart" })).toBe(null);
});

it("keeps the bar moving without a number until the download reports", () => {
  offer({ installing: true, progress: null }, false);
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(null);
});

it("shows a failed install and lets it be tried again", () => {
  const install = offer({ installError: FAILURE }, false);
  expect(screen.getByRole("alert").textContent).toBe("The update did not finish: No network");
  fireEvent.click(screen.getByRole("button", { name: "Try the update again" }));
  expect(install).toHaveBeenCalledTimes(1);
});

it("checks when pressed", () => {
  const check = checker({});
  fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(check).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("status")).toBe(null);
});

it("says a check is running, and allows no second one", () => {
  checker({ checking: true, available: INFO });
  expect(screen.getByRole("status").textContent).toBe("Checking for updates");
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Check for updates" }).disabled).toBe(true);
});

it("allows no check during an install", () => {
  checker({ installing: true });
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Check for updates" }).disabled).toBe(true);
});

it("says when this is the latest version", () => {
  checker({ upToDate: true });
  expect(screen.getByRole("status").textContent).toBe("This is the latest version.");
});

it("says which version was found", () => {
  checker({ available: INFO });
  expect(screen.getByRole("status").textContent).toBe("Version 2.0.0 is available");
});

it("shows a failed check, and the button tries again", () => {
  const check = checker({ checkError: FAILURE });
  expect(screen.getByRole("alert").textContent).toBe("Could not check for updates: No network");
  fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(check).toHaveBeenCalledTimes(1);
});
