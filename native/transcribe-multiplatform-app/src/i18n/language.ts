// Runs with bun: picks the UI language from the operating system's locale.
import { deMessages } from "./locale-de.ts";
import { enMessages } from "./locale-en.ts";
import { esMessages } from "./locale-es.ts";
import { frMessages } from "./locale-fr.ts";
import { jaMessages } from "./locale-ja.ts";
import { koMessages } from "./locale-ko.ts";
import { zhHansMessages } from "./locale-zh-hans.ts";
import { zhHantMessages } from "./locale-zh-hant.ts";
import type { Messages } from "./messages.ts";

export interface Language {
  tag: string;
  messages: Messages;
}

const TRADITIONAL_CHINESE: RegExp = /^zh-(hant|tw|hk|mo)/i;

export const LANGUAGES: ReadonlyMap<string, Language> = new Map<string, Language>([
  ["en", { tag: "en", messages: enMessages }],
  ["ja", { tag: "ja", messages: jaMessages }],
  ["zh", { tag: "zh-Hans", messages: zhHansMessages }],
  ["ko", { tag: "ko", messages: koMessages }],
  ["de", { tag: "de", messages: deMessages }],
  ["fr", { tag: "fr", messages: frMessages }],
  ["es", { tag: "es", messages: esMessages }],
]);

const FALLBACK: Language = { tag: "en", messages: enMessages };
const TRADITIONAL: Language = { tag: "zh-Hant", messages: zhHantMessages };

/** `ja-JP`, `ja_JP.UTF-8` and `zh-Hant-TW` all resolve; anything unknown becomes English. */
export const languageFor = (locale: string): Language =>
  TRADITIONAL_CHINESE.test(locale)
    ? TRADITIONAL
    : (LANGUAGES.get(locale.split(/[-_]/)[0].toLowerCase()) ?? FALLBACK);

export const ALL_LANGUAGE_MESSAGES: Messages[] = [
  enMessages,
  jaMessages,
  zhHansMessages,
  zhHantMessages,
  koMessages,
  deMessages,
  frMessages,
  esMessages,
];
