// Runs with bun. Real file-system adapter for the FileSystem port.
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  copyFile,
  link,
  mkdir,
  open,
  readdir,
  readFile,
  rename,
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { dirname } from "node:path";
import type { FileStat, FileSystem } from "./ports.ts";

const NOT_FOUND = "ENOENT";
const EXISTS = "EEXIST";

function hasCode(error: unknown, code: string): boolean {
  return error instanceof Error && "code" in error && error.code === code;
}

const isNotFound = (error: unknown): boolean => hasCode(error, NOT_FOUND);

// Written beside the target, then hard-linked into place: link() fails when the target exists,
// so the file appears atomically and always whole (a reader never sees it half written).
async function createExclusive(args: {
  path: string;
  text: string;
  mode: number;
}): Promise<boolean> {
  const partial = `${args.path}.${process.pid}.${Date.now()}.partial`;
  await writeFile(partial, args.text, { mode: args.mode, flag: "wx" });
  try {
    await link(partial, args.path);
    return true;
  } catch (error) {
    if (hasCode(error, EXISTS)) {
      return false;
    }
    throw error;
  } finally {
    await rm(partial, { force: true });
  }
}

async function statOrNull(path: string): Promise<FileStat | null> {
  try {
    const result = await stat(path, { bigint: true });
    return { size: Number(result.size), mtimeNs: result.mtimeNs, isFile: result.isFile() };
  } catch (error) {
    if (isNotFound(error)) {
      return null;
    }
    throw error;
  }
}

function hashFile(path: string): Promise<string> {
  return new Promise((resolveHash, reject) => {
    const hash = createHash("sha256");
    createReadStream(path)
      .on("data", (block) => hash.update(block))
      .on("error", reject)
      .on("end", () => resolveHash(hash.digest("hex")));
  });
}

export function createNodeFs(): FileSystem {
  return {
    stat: statOrNull,
    readdir: (path) => readdir(path),
    sha256: hashFile,
    readText: (path) => readFile(path, "utf8"),
    mkdir: async (path) => {
      await mkdir(path, { recursive: true, mode: 0o700 });
    },
    writeText: async ({ path, text, mode }) => {
      await mkdir(dirname(path), { recursive: true, mode: 0o700 });
      await writeFile(path, text, { mode });
    },
    createExclusive,
    copy: ({ from, to }) => copyFile(from, to),
    link: async ({ target, path }) => {
      await rm(path, { recursive: true, force: true });
      await symlink(target, path);
    },
    removeTree: (path) => rm(path, { recursive: true, force: true }),
    rename: ({ from, to }) => rename(from, to),
    readChunk: async ({ path, start, length }) => {
      const handle = await open(path, "r");
      try {
        const buffer = new Uint8Array(length);
        const { bytesRead } = await handle.read(buffer, 0, length, start);
        return buffer.subarray(0, bytesRead);
      } finally {
        await handle.close();
      }
    },
  };
}
