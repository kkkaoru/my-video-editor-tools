// Runs with bun: the license dialog loads when opened and shows each text as it is.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import type { LicenseList } from "../backend/licenses.ts";
import { LicensesDialog } from "./LicensesDialog.tsx";

const LIST: LicenseList = {
  entries: [
    { name: "alpha", version: "1.0.0", license: "MIT", url: "https://example.com/alpha", text: 0 },
    { name: "beta", version: "2.0.0", license: "", url: "", text: null },
  ],
  texts: ["The alpha license text"],
};

afterEach(cleanup);

async function opened(load: () => Promise<LicenseList>): Promise<void> {
  render(
    <main>
      <LicensesDialog load={load} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Open source licenses" }));
  });
}

it("loads nothing until it is opened, and then once", async () => {
  const load: Mock<() => Promise<LicenseList>> = vi.fn().mockResolvedValue(LIST);
  render(
    <main>
      <LicensesDialog load={load} />
    </main>,
  );
  expect(load).toHaveBeenCalledTimes(0);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Open source licenses" }));
  });
  await waitFor((): void => {
    expect(screen.getAllByRole("listitem").length).toBe(2);
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Open source licenses" }));
  });
  expect(load).toHaveBeenCalledTimes(1);
});

it("says it is loading first", async () => {
  await opened(() => new Promise<LicenseList>((): void => undefined));
  expect(screen.getByRole("status").textContent).toBe("Loading the licenses");
});

it("names each row, with an unknown license said so", async () => {
  await opened(() => Promise.resolve(LIST));
  expect(screen.getByRole("button", { name: "alpha 1.0.0 MIT" }).tagName).toBe("BUTTON");
  expect(screen.getByRole("button", { name: "beta 2.0.0 Not declared" }).tagName).toBe("BUTTON");
});

it("shows the text and the address when a row is opened", async () => {
  await opened(() => Promise.resolve(LIST));
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "alpha 1.0.0 MIT" }));
  });
  expect(screen.getByText("The alpha license text").tagName).toBe("PRE");
  expect(screen.getByText("https://example.com/alpha").tagName).toBe("P");
});

it("never makes up a text a package does not ship", async () => {
  await opened(() => Promise.resolve(LIST));
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "beta 2.0.0 Not declared" }));
  });
  expect(screen.getByText("This package does not include a license text.").tagName).toBe("P");
});

it("says so when the list cannot be read, and tries again next time", async () => {
  const load: Mock<() => Promise<LicenseList>> = vi
    .fn()
    .mockRejectedValueOnce(new Error("gone"))
    .mockResolvedValue(LIST);
  await opened(load);
  await waitFor((): void => {
    expect(screen.getByRole("alert").textContent).toBe("The license list could not be read.");
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Open source licenses" }));
  });
  await waitFor((): void => {
    expect(screen.getAllByRole("listitem").length).toBe(2);
  });
});

it("gives focus back to the button when closed", async () => {
  await opened(() => Promise.resolve(LIST));
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
  });
  await waitFor((): void => {
    expect(document.activeElement?.textContent).toBe("Open source licenses");
  });
});
