// Runs with bun. Entry point: `bun run deploy` deploys the viewer Worker. VIEWER_DOMAIN (custom
// domain, comma separated) comes from deploy.env; secrets come from secrets.json (both untracked).
// The caption font is rebuilt first from the installed TTF (CAPTION_FONT overrides its path).
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { prepareCaptionFont } from "../sync/captionFont.ts";
import { parseEnvFile } from "../sync/envFile.ts";
import { createNodeFs } from "../sync/nodeFs.ts";
import { createNodeRunner } from "../sync/nodeProcess.ts";

const projectDirectory = resolve(import.meta.dirname, "..", "..");
const fs = createNodeFs();
const source =
  process.env.CAPTION_FONT ??
  join(homedir(), "Library", "Fonts", "NotoSansJP-VariableFont_wght.ttf");
const font = await prepareCaptionFont({
  fs,
  runner: createNodeRunner(process.env),
  projectDirectory,
  source,
});
console.log(`caption font: ${font}`);
const environment = parseEnvFile(await fs.readText(`${projectDirectory}/deploy.env`));
const child = spawn("cf", ["deploy", "--secrets-file", "secrets.json"], {
  cwd: projectDirectory,
  env: { ...process.env, ...environment },
  stdio: "inherit",
});
child.on("close", (code) => process.exit(code ?? 1));
