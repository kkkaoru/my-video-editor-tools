// Runs with bun: the dialog opens inside the main landmark, named by its title and description.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "./alert-dialog.tsx";

afterEach(cleanup);

it("opens named by its title and closes", async () => {
  render(
    <main>
      <AlertDialog>
        <AlertDialogTrigger>Open</AlertDialogTrigger>
        <AlertDialogContent className="w-96">
          <AlertDialogTitle className="text-lg">Sure?</AlertDialogTitle>
          <AlertDialogDescription className="text-base">No way back.</AlertDialogDescription>
          <AlertDialogClose>Close</AlertDialogClose>
        </AlertDialogContent>
      </AlertDialog>
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
  });
  const dialog: HTMLElement = screen.getByRole("alertdialog", { name: "Sure?" });
  expect(dialog.closest("main")?.tagName).toBe("MAIN");
  expect(screen.getByText("No way back.").id).toBe(dialog.getAttribute("aria-describedby"));
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
  });
  await waitFor((): void => {
    expect(screen.queryByRole("alertdialog")).toBe(null);
  });
});
