// Runs with bun: says what the chosen subtitle format cannot show, wherever those settings are.
import type { VNode } from "preact";
import type { SubtitleFormat } from "../backend/types.ts";
import type { Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";

export interface FormatLimitProps {
  format: SubtitleFormat;
}

export function FormatLimit({ format }: FormatLimitProps): VNode | null {
  const messages: Messages = useMessages();
  return format === "drt" ? null : <p className="text-muted-foreground">{messages.formatLimits[format]}</p>;
}
