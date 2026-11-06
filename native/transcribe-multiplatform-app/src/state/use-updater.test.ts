// Runs with bun: the startup check stays quiet; the check and the install the person asks for report.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, renderHook, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import { useUpdater } from "./use-updater.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const INFO = { version: "2.0.0", notes: "New" };
const stop: Mock<() => void> = vi.fn();

const answer = (result: unknown): void => {
  vi.mocked(invoke).mockResolvedValue(result);
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listen).mockResolvedValue(stop);
  answer(null);
});

afterEach(cleanup);

it("checks once at startup, and offers what it finds", async () => {
  answer(INFO);
  const { result, rerender } = renderHook(() => useUpdater());
  rerender();
  await waitFor((): void => {
    expect(result.current.state.available).toStrictEqual(INFO);
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["check_update"]]);
});

it("does not say up to date after the startup check", async () => {
  const { result } = renderHook(() => useUpdater());
  await act(async (): Promise<void> => {
    await Promise.resolve();
  });
  expect(result.current.state.upToDate).toBe(false);
});

it("only logs a failed startup check", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(vi.fn());
  vi.mocked(invoke).mockRejectedValue({ code: "internal", detail: "offline" });
  const { result } = renderHook(() => useUpdater());
  await waitFor((): void => {
    expect(warn).toHaveBeenCalledTimes(1);
  });
  expect(result.current.state.checkError).toBe(null);
  warn.mockRestore();
});

it("reports a check the person asked for", async () => {
  const { result } = renderHook(() => useUpdater());
  act((): void => result.current.check());
  expect(result.current.state.checking).toBe(true);
  await waitFor((): void => {
    expect(result.current.state.upToDate).toBe(true);
  });
});

it("shows the failure of a check the person asked for", async () => {
  const { result } = renderHook(() => useUpdater());
  await act(async (): Promise<void> => {
    await Promise.resolve();
  });
  vi.mocked(invoke).mockRejectedValue({ code: "internal", detail: "offline" });
  act((): void => result.current.check());
  await waitFor((): void => {
    expect(result.current.state.checkError).toStrictEqual({ code: "internal", detail: "offline" });
  });
});

it("installs only when asked, and stays installing because success restarts the app", async () => {
  const { result } = renderHook(() => useUpdater());
  expect(vi.mocked(invoke).mock.calls.some(([command]): boolean => command === "install_update")).toBe(false);
  act((): void => result.current.install());
  await act(async (): Promise<void> => {
    await Promise.resolve();
  });
  expect(vi.mocked(invoke).mock.lastCall).toStrictEqual(["install_update"]);
  expect(result.current.state.installing).toBe(true);
});

it("reports a failed install, ready to try again", async () => {
  const { result } = renderHook(() => useUpdater());
  vi.mocked(invoke).mockRejectedValue(new Error("no disk"));
  const warn = vi.spyOn(console, "warn").mockImplementation(vi.fn());
  act((): void => result.current.install());
  await waitFor((): void => {
    expect(result.current.state.installError).toStrictEqual({ code: "internal", detail: "Error: no disk" });
  });
  expect(result.current.state.installing).toBe(false);
  warn.mockRestore();
});

it("follows the download's progress, and stops listening when it goes", async () => {
  const stopThis: Mock<() => void> = vi.fn();
  vi.mocked(listen).mockResolvedValue(stopThis);
  const { result, unmount } = renderHook(() => useUpdater());
  expect(vi.mocked(listen).mock.calls[0]?.[0]).toBe("update-progress");
  const handler = vi.mocked(listen).mock.calls[0]?.[1];
  act((): void => handler?.({ event: "update-progress", id: 1, payload: 0.25 }));
  expect(result.current.state.progress).toBe(0.25);
  unmount();
  await waitFor((): void => {
    expect(stopThis).toHaveBeenCalled();
  });
});

it("logs a listener that cannot be stopped", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(vi.fn());
  vi.mocked(listen).mockRejectedValue(new Error("gone"));
  const { unmount } = renderHook(() => useUpdater());
  unmount();
  await waitFor((): void => {
    expect(warn).toHaveBeenCalledWith("stopping the update progress failed", new Error("gone"));
  });
  warn.mockRestore();
});
