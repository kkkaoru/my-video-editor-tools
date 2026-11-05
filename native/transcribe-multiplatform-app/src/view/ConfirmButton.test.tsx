// Runs with bun: an action that cannot be undone runs only after it is confirmed.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { ConfirmButton } from "./ConfirmButton.tsx";

afterEach(cleanup);

async function openDialog(onConfirm: () => void): Promise<void> {
  render(
    <main>
      <ConfirmButton
        title="Delete A?"
        description="This cannot be undone."
        confirmLabel="Delete A"
        cancelLabel="Keep"
        onConfirm={onConfirm}
      >
        Delete
      </ConfirmButton>
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  });
}

it("names the action and its consequence", async () => {
  await openDialog(vi.fn());
  const dialog: HTMLElement = screen.getByRole("alertdialog", { name: "Delete A?" });
  expect(dialog.closest("main")?.tagName).toBe("MAIN");
  expect(screen.getByText("This cannot be undone.").tagName).toBe("P");
});

it("does nothing until confirmed", async () => {
  const onConfirm: Mock<() => void> = vi.fn();
  await openDialog(onConfirm);
  expect(onConfirm).toHaveBeenCalledTimes(0);
});

it("runs the action when confirmed", async () => {
  const onConfirm: Mock<() => void> = vi.fn();
  await openDialog(onConfirm);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Delete A" }));
  });
  expect(onConfirm).toHaveBeenCalledTimes(1);
});

it("does not run the action when cancelled", async () => {
  const onConfirm: Mock<() => void> = vi.fn();
  await openDialog(onConfirm);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Keep" }));
  });
  expect(onConfirm).toHaveBeenCalledTimes(0);
});
