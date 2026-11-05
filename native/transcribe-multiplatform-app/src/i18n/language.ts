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

const WORD_JOINER: string = "\u2060";
const BREAKS_INSIDE_WORDS: RegExp = /[\p{sc=Han}\p{sc=Hiragana}\p{sc=Katakana}\p{sc=Hangul}]/u;
const PARTICLE_START: RegExp = /^\p{sc=Hiragana}/u;
const SENTENCE_MARK_END: RegExp = /[、。！？]$/u;

/** Browsers wrap Japanese, Chinese and Korean between any two characters; this keeps "モデル" and "選べます" on one line. */
export const keepWordsWhole = (text: string, tag: string): string =>
  Array.from(new Intl.Segmenter(tag, { granularity: "word" }).segment(text))
    .map(({ segment, isWordLike }, index, all): string => {
      const word =
        isWordLike === true && BREAKS_INSIDE_WORDS.test(segment)
          ? Array.from(segment).join(WORD_JOINER)
          : segment;
      const next = all[index + 1]?.segment ?? "";
      return PARTICLE_START.test(next) && !SENTENCE_MARK_END.test(segment) && segment.trim() !== ""
        ? word + WORD_JOINER
        : word;
    })
    .join("");

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
