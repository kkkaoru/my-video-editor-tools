// Runs with bun: saves the caption look under a name, and restores it later.
import type { VNode } from "preact";
import { useId, useState } from "preact/hooks";
import { Badge } from "@/components/ui/badge.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Input } from "@/components/ui/input.tsx";
import { Label } from "@/components/ui/label.tsx";
import { deleteProfile, loadProfile, overwriteProfile, saveProfile } from "../backend/backend.ts";
import type { Profile, Settings, Snapshot } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { lookChanged } from "../state/app-state.ts";
import { type AppStore, reportFailure } from "../state/use-app-store.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";

export interface ProfilesCardProps {
  store: AppStore;
  settings: Settings;
  profiles: Profile[];
}

interface ProfileRowProps {
  profile: Profile;
  settings: Settings;
  apply: (action: Promise<Snapshot>) => Promise<void>;
}

function ProfileRow({ profile, settings, apply }: ProfileRowProps): VNode {
  const messages: Messages = useMessages();
  const { name } = profile;
  const selected: boolean = settings.active_profile === name;
  return (
    <li className="grid gap-2 border-t border-border pt-3 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium break-all">{name}</span>
        {selected ? <Badge variant="default">{messages.profileSelected}</Badge> : null}
        {selected && lookChanged(settings, profile.look) ? <Badge>{messages.profileChanged}</Badge> : null}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={(): void => void apply(loadProfile(name))}>
          {format(messages.profileLoad, { name })}
        </Button>
        <ConfirmButton
          title={format(messages.profileOverwriteTitle, { name })}
          description={messages.profileOverwriteDescription}
          confirmLabel={format(messages.profileOverwrite, { name })}
          cancelLabel={messages.dialogCancel}
          onConfirm={(): void => void apply(overwriteProfile(name, settings))}
        >
          {format(messages.profileOverwrite, { name })}
        </ConfirmButton>
        <ConfirmButton
          title={format(messages.profileDeleteTitle, { name })}
          description={messages.profileDeleteDescription}
          confirmLabel={format(messages.profileDelete, { name })}
          cancelLabel={messages.dialogCancel}
          onConfirm={(): void => void apply(deleteProfile(name))}
        >
          {format(messages.profileDelete, { name })}
        </ConfirmButton>
      </div>
    </li>
  );
}

export function ProfilesCard({ store, settings, profiles }: ProfilesCardProps): VNode {
  const messages: Messages = useMessages();
  const inputId: string = useId();
  const [name, setName] = useState<string>("");
  const apply = (action: Promise<Snapshot>): Promise<void> =>
    action.then(store.replaceSnapshot).catch((error: unknown): void => reportFailure(store.setState, error));
  return (
    <Card>
      <CardHeader>
        <CardTitle>{messages.profilesHeading}</CardTitle>
        <CardDescription>{messages.profilesHint}</CardDescription>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-4">
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(event: Event): void => {
            event.preventDefault();
            // The field empties only after a success: a refused name stays there to be corrected.
            void apply(
              saveProfile(name, settings).then((snapshot: Snapshot): Snapshot => {
                setName("");
                return snapshot;
              }),
            );
          }}
        >
          <div className="grid min-w-48 flex-1 gap-1">
            <Label htmlFor={inputId}>{messages.profileName}</Label>
            <Input
              id={inputId}
              value={name}
              required
              pattern=".*\S.*"
              maxLength={80}
              onInput={({ currentTarget }): void => setName(currentTarget.value)}
            />
          </div>
          <Button type="submit">{messages.profileSave}</Button>
        </form>
        {profiles.length === 0 ? (
          <p className="text-muted-foreground">{messages.profileEmpty}</p>
        ) : (
          <ul className="grid grid-cols-1 gap-3">
            {profiles.map(
              (profile: Profile): VNode => (
                <ProfileRow key={profile.name} profile={profile} settings={settings} apply={apply} />
              ),
            )}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
