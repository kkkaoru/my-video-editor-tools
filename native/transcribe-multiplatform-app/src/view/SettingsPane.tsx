// Runs with bun: the version, and the display language and appearance that override the system's.
import type { VNode } from "preact";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import type { Settings, UiTheme } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import type { Updater } from "../state/use-updater.ts";
import { SelectField, type SelectOption } from "./fields.tsx";
import { UpdateCheck } from "./UpdatePanels.tsx";

export interface SettingsPaneProps {
  version: string;
  settings: Settings;
  onChange: (settings: Settings) => void;
  updater: Updater;
}

/** The backend keeps "follow the system" as an empty language; a select needs a real value for it. */
const SYSTEM_CHOICE: string = "system";
const THEMES: UiTheme[] = ["system", "light", "dark"];
/** Each language under its own name, so it can be found by someone who cannot read the current one. */
const NATIVE_LANGUAGES: { value: string; label: string; lang: string }[] = [
  { value: "en", label: "English", lang: "en" },
  { value: "ja", label: "日本語", lang: "ja" },
  { value: "zh-Hans", label: "简体中文", lang: "zh-Hans" },
  { value: "zh-Hant", label: "繁體中文", lang: "zh-Hant" },
  { value: "ko", label: "한국어", lang: "ko" },
  { value: "de", label: "Deutsch", lang: "de" },
  { value: "fr", label: "Français", lang: "fr" },
  { value: "es", label: "Español", lang: "es" },
];

const themeName = (theme: UiTheme, messages: Messages): string =>
  ({ system: messages.uiThemeSystem, light: messages.uiThemeLight, dark: messages.uiThemeDark })[theme];

export function SettingsPane({ version, settings, onChange, updater }: SettingsPaneProps): VNode {
  const messages: Messages = useMessages();
  return (
    <div className="grid grid-cols-1 gap-6">
      <div className="grid grid-cols-1 gap-2">
        <p className="text-muted-foreground">{format(messages.help.version, { version })}</p>
        <UpdateCheck updater={updater} />
      </div>
      <Card>
        <CardHeader>
          <CardTitle>{messages.tabSettings}</CardTitle>
        </CardHeader>
        <CardContent className="grid max-w-md grid-cols-1 gap-4">
          <SelectField
            label={messages.uiLanguageLabel}
            value={settings.ui_language === "" ? SYSTEM_CHOICE : settings.ui_language}
            options={[
              { value: SYSTEM_CHOICE, label: messages.uiLanguageSystem, disabled: false },
              ...NATIVE_LANGUAGES.map((language): SelectOption => ({ ...language, disabled: false })),
            ]}
            onChange={(choice: string): void =>
              onChange({ ...settings, ui_language: choice === SYSTEM_CHOICE ? "" : choice })
            }
          />
          <SelectField
            label={messages.uiThemeLabel}
            value={settings.ui_theme}
            options={THEMES.map(
              (theme: UiTheme): SelectOption => ({
                value: theme,
                label: themeName(theme, messages),
                disabled: false,
              }),
            )}
            onChange={(value: string): void =>
              THEMES.filter((theme: UiTheme): boolean => theme === value).forEach(
                (ui_theme: UiTheme): void => {
                  onChange({ ...settings, ui_theme });
                },
              )
            }
          />
        </CardContent>
      </Card>
    </div>
  );
}
