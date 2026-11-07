// Runs with bun: the generated license list, fetched only when someone opens it.
export interface LicenseEntry {
  name: string;
  version: string;
  /** The SPDX expression as the package declares it; empty when it declares none. */
  license: string;
  url: string;
  /** Index into `texts`; null when the package ships no license file. */
  text: number | null;
}

export interface LicenseList {
  entries: LicenseEntry[];
  texts: string[];
}

/** A script, not a fetch: the app's content policy lets scripts load from itself but no fetch of its own files. */
const LICENSES_PATH: string = "/licenses.js";

const isLicenseList = (value: unknown): value is LicenseList =>
  typeof value === "object" &&
  value !== null &&
  "entries" in value &&
  "texts" in value &&
  Array.isArray(value.entries) &&
  Array.isArray(value.texts);

const importScript = (path: string): Promise<{ default: unknown }> => import(/* @vite-ignore */ path);

export async function loadLicenses(load: typeof importScript = importScript): Promise<LicenseList> {
  const { default: value } = await load(LICENSES_PATH);
  if (!isLicenseList(value)) {
    throw new Error(`${LICENSES_PATH}: not a license list`);
  }
  return value;
}
