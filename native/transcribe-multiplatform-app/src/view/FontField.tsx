// Runs with bun: picks the one font family every speaker's captions use, from those installed here.
import type { VNode } from "preact";
import { useMemo, useState } from "preact/hooks";
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxStatus,
  ComboboxTrigger,
  ComboboxValue,
} from "@/components/ui/combobox.tsx";
import { fontFamilies } from "../backend/backend.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { whenPresent } from "./fields.tsx";

export interface FontFieldProps {
  value: string;
  onChange: (family: string) => void;
  onFailure: (error: unknown) => void;
}

/** A computer can have thousands of families; only this many rows are ever in the DOM, typing finds the rest. */
const VISIBLE_ROWS: number = 100;

/** Plain substring search: the library's collation-aware default is too slow for thousands of names per key press. */
export const matchesFamily = (family: string, query: string): boolean =>
  family.toLowerCase().includes(query.toLowerCase());

/** The saved family stays listed even when this computer does not have it. */
export const familyChoices = (fonts: string[] | null, value: string): string[] => {
  const installed: string[] = fonts ?? [];
  return installed.includes(value) ? installed : [value, ...installed];
};

export function FontField({ value, onChange, onFailure }: FontFieldProps): VNode {
  const messages: Messages = useMessages();
  const [fonts, setFonts] = useState<string[] | null>(null);
  const choices: string[] = useMemo((): string[] => familyChoices(fonts, value), [fonts, value]);
  // Read on first open, because the first scan reads the header of every installed font.
  const loadFonts = (open: boolean): void => {
    if (open && fonts === null) {
      fontFamilies().then(setFonts).catch(onFailure);
    }
  };
  return (
    <div className="grid grid-cols-1 gap-2">
      <span className="text-sm font-medium">{messages.fontFamily}</span>
      <Combobox
        items={choices}
        value={value}
        limit={VISIBLE_ROWS}
        filter={matchesFamily}
        onOpenChange={loadFonts}
        onValueChange={(next: string | null): void => whenPresent(next, onChange)}
      >
        <ComboboxTrigger aria-label={messages.fontFamily}>
          <ComboboxValue />
        </ComboboxTrigger>
        <ComboboxContent aria-label={messages.fontFamily}>
          <ComboboxInput aria-label={messages.fontSearch} />
          {fonts === null ? <ComboboxStatus>{messages.fontLoading}</ComboboxStatus> : null}
          <ComboboxEmpty>{messages.fontNoMatch}</ComboboxEmpty>
          <ComboboxList>
            {(family: string): VNode => (
              <ComboboxItem key={family} value={family}>
                {family}
              </ComboboxItem>
            )}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
      <p className="text-muted-foreground">{messages.fontHint}</p>
      {fonts === null || fonts.includes(value) ? null : (
        <p className="text-muted-foreground">{format(messages.fontNotInstalled, { font: value })}</p>
      )}
    </div>
  );
}
