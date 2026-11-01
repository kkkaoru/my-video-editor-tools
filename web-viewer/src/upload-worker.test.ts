// Runs with bun (vitest): the temporary upload Worker's /get contract, with and without Range.
import { expect, it } from "vitest";
import worker from "./upload-worker.js";

const BODY = "abcdefghij";

const environment = {
  UPLOAD_TOKEN: "token",
  MEDIA: {
    get: async (key: string, options?: { range: { offset: number; length: number } }) => {
      if (key !== "k") return null;
      const range = options?.range;
      const body = range ? BODY.slice(range.offset, range.offset + range.length) : BODY;
      return { body, size: BODY.length };
    },
  },
};

function get(key: string, range?: string) {
  const headers: Record<string, string> = { "X-Upload-Token": "token" };
  if (range !== undefined) headers.Range = range;
  return worker.fetch(new Request(`https://example.test/get?key=${key}`, { headers }), environment);
}

it("streams the whole object without a Range header", async () => {
  const response = await get("k");
  expect(response.status).toBe(200);
  expect(response.headers.get("Content-Length")).toBe("10");
  expect(response.headers.get("Content-Range")).toBeNull();
  expect(await response.text()).toBe("abcdefghij");
});

it("answers a byte range with 206 and its Content-Range", async () => {
  const response = await get("k", "bytes=4-7");
  expect(response.status).toBe(206);
  expect(response.headers.get("Content-Length")).toBe("4");
  expect(response.headers.get("Content-Range")).toBe("bytes 4-7/10");
  expect(await response.text()).toBe("efgh");
});

it("clips a range that runs past the end", async () => {
  const response = await get("k", "bytes=8-20");
  expect(response.headers.get("Content-Range")).toBe("bytes 8-9/10");
  expect(await response.text()).toBe("ij");
});

it("rejects a malformed or empty range", async () => {
  expect((await get("k", "bytes=-5")).status).toBe(416);
  expect((await get("k", "bytes=7-3")).status).toBe(416);
});

it("reports a missing object for ranged and whole reads", async () => {
  expect((await get("missing", "bytes=0-3")).status).toBe(404);
  expect((await get("missing")).status).toBe(404);
});
