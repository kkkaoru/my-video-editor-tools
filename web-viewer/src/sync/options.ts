// Runs with bun. Command-line options of the sync command.
import { parseArgs } from "node:util";

export interface SyncOptions {
  dryRun: boolean;
  dataOnly: boolean;
  // Upload only the originals (media-original/); publish no data or catalog.
  originalsOnly?: boolean;
  // Upload the movies of this catalog video ID only; data and the catalog stay global.
  only?: string;
  // How long to wait in total for unplugged library volumes before failing (run.ts has the default).
  volumeWaitMinutes?: number;
  // How long to wait for another sync's lock before failing (lock.ts has the default).
  lockWaitMinutes?: number;
}

function parseMinutes(text: string, flag: string): number {
  const minutes = Number(text);
  if (!Number.isFinite(minutes) || minutes < 0) {
    throw new Error(`${flag} must be a non-negative number, got ${text}`);
  }
  return minutes;
}

export function parseOptions(argv: string[]): SyncOptions {
  const { values } = parseArgs({
    args: argv,
    options: {
      "dry-run": { type: "boolean" },
      "data-only": { type: "boolean" },
      "originals-only": { type: "boolean" },
      only: { type: "string" },
      "volume-wait-minutes": { type: "string" },
      "lock-wait-minutes": { type: "string" },
    },
    strict: true,
  });
  return {
    dryRun: values["dry-run"] === true,
    dataOnly: values["data-only"] === true,
    originalsOnly: values["originals-only"] === true,
    ...(values.only === undefined ? {} : { only: values.only }),
    ...(values["volume-wait-minutes"] === undefined
      ? {}
      : {
          volumeWaitMinutes: parseMinutes(values["volume-wait-minutes"], "--volume-wait-minutes"),
        }),
    ...(values["lock-wait-minutes"] === undefined
      ? {}
      : { lockWaitMinutes: parseMinutes(values["lock-wait-minutes"], "--lock-wait-minutes") }),
  };
}
