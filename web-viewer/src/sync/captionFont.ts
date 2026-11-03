// Runs with bun. The caption font the page serves (public/fonts, not in Git): before each deploy it
// is rebuilt as a lossless WOFF2 from the Noto Sans JP variable TTF the native renderer uses, and
// the deploy fails unless that TTF has the pinned SHA-256. A WOFF2 already built from it is reused.
import { join } from "node:path";
import { z } from "zod";
import type { FileSystem, ProcessRunner } from "./ports.ts";

// NotoSansJP-VariableFont_wght.ttf as installed for the native caption renderer.
export const FONT_SOURCE_SHA256 =
  "b81fe0ce33403cab3c68834f2ae081cb9b0ba55de427554e7e0437ac389fa105";
export const FONT_TARGET = "public/fonts/NotoSansJP-VariableFont_wght.woff2";
const STAMP = "cache/caption-font.json";
// fontTools and brotli come from the repository's locked environment (pyproject).
const CONVERT =
  "import sys\nfrom fontTools.ttLib import TTFont\nf = TTFont(sys.argv[1])\nf.flavor = 'woff2'\nf.save(sys.argv[2])";

const stampSchema = z.object({ source: z.string(), woff2: z.string() });

export interface CaptionFontRequest {
  fs: FileSystem;
  runner: ProcessRunner;
  projectDirectory: string;
  source: string;
}

async function builtFromSource(fs: FileSystem, projectDirectory: string): Promise<boolean> {
  const target = join(projectDirectory, FONT_TARGET);
  const stampPath = join(projectDirectory, STAMP);
  if ((await fs.stat(target)) === null || (await fs.stat(stampPath)) === null) {
    return false;
  }
  const stamp = stampSchema.safeParse(JSON.parse(await fs.readText(stampPath)));
  return (
    stamp.success &&
    stamp.data.source === FONT_SOURCE_SHA256 &&
    stamp.data.woff2 === (await fs.sha256(target))
  );
}

// Returns "reused" or "built"; throws when the TTF is missing, differs or cannot be converted.
export async function prepareCaptionFont(request: CaptionFontRequest): Promise<string> {
  const { fs, runner, projectDirectory, source } = request;
  if ((await fs.stat(source)) === null) {
    throw new Error(`Caption font not found: ${source}`);
  }
  const sha = await fs.sha256(source);
  if (sha !== FONT_SOURCE_SHA256) {
    throw new Error(`Caption font ${source} has SHA-256 ${sha}, expected ${FONT_SOURCE_SHA256}`);
  }
  if (await builtFromSource(fs, projectDirectory)) {
    return "reused";
  }
  const target = join(projectDirectory, FONT_TARGET);
  const partial = `${target}.partial`;
  await fs.mkdir(join(projectDirectory, "public", "fonts"));
  const result = await runner.run({
    command: "uv",
    args: ["run", "--no-sync", "python", "-I", "-c", CONVERT, source, partial],
    cwd: join(projectDirectory, ".."),
  });
  if (result.code !== 0) {
    await fs.removeTree(partial);
    throw new Error(`Converting the caption font failed: ${result.stderr.slice(-400)}`);
  }
  await fs.rename({ from: partial, to: target });
  await fs.mkdir(join(projectDirectory, "cache"));
  const stamp = { source: FONT_SOURCE_SHA256, woff2: await fs.sha256(target) };
  await fs.writeText({
    path: join(projectDirectory, STAMP),
    text: `${JSON.stringify(stamp)}\n`,
    mode: 0o644,
  });
  return "built";
}
