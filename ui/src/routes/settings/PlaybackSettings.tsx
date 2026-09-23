import { useMutation, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, STREAMING_QUALITIES, type ClientSettings } from "@/lib/api"
import { useSettings } from "@/lib/queries"
import { DEFAULT_PLAYBACK_SETTINGS } from "./defaults"
import { same, saveSettings } from "./helpers"
import { PageTitle, Section, SelectField, SettingsError, SettingsLoading, SettingsRow } from "./shared"

type PlaybackDraft = ClientSettings["client"]["playback"]

const SKIP_CHOICES = [
  { value: "disabled", label: "Never" },
  { value: "prompt", label: "Ask me" },
  { value: "always", label: "Always skip" },
] as const

const SEGMENTS = [
  {
    key: "skipIntro",
    id: "settings-introductions",
    title: "Introductions",
    label: "Intro skipping",
    description: "Choose what happens when an intro marker is reached.",
  },
  {
    key: "skipCredits",
    id: "settings-credits",
    title: "Credits",
    label: "Credits skipping",
    description: "Choose what happens when credits begin.",
  },
  {
    key: "skipRecap",
    id: "settings-recaps",
    title: "Recaps",
    label: "Recap skipping",
    description: "Choose what happens when a recap marker is reached.",
  },
  {
    key: "skipCommercial",
    id: "settings-commercials",
    title: "Commercials",
    label: "Commercial skipping",
    description: "Choose what happens when a commercial marker is reached.",
  },
] as const

export default function PlaybackSettings() {
  const cache = useQueryClient()
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.client.playback)
  const mutation = useMutation({
    mutationFn: (value: PlaybackDraft) => api.settingsPatch.playback(value),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.client.playback, submitted)
      saveSettings(cache, saved)
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (settingsQuery.error && !settings) {
    return (
      <SettingsError
        title="Playback settings unavailable"
        error={settingsQuery.error}
        onRetry={() => void settingsQuery.refetch()}
      />
    )
  }
  if (!settings || !draft) return <SettingsLoading />
  const update = <Key extends keyof PlaybackDraft>(key: Key, value: PlaybackDraft[Key]) =>
    setDraft({ ...draft, [key]: value })
  return (
    <div className="settings-page">
      <PageTitle title="Playback" />
      <Section
        title="Streaming quality"
        description="Original sends the source unchanged; lower quality permits transcoding when needed."
      >
        <SettingsRow
          controlId="settings-default-quality"
          title="Default quality"
          description="You can still override this for an individual play."
        >
          <SelectField
            id="settings-default-quality"
            aria-describedby="settings-default-quality-help"
            label="Default streaming quality"
            value={draft.streamingQuality}
            onValueChange={(value) => update("streamingQuality", value)}
            options={STREAMING_QUALITIES.map(({ id, label }) => ({ value: id, label }))}
          />
        </SettingsRow>
      </Section>
      <Section title="Segment skipping" description="MediaFlick uses Jellyfin segment markers when they are available.">
        {SEGMENTS.map((segment) => (
          <SettingsRow key={segment.key} controlId={segment.id} title={segment.title} description={segment.description}>
            <SelectField
              id={segment.id}
              aria-describedby={`${segment.id}-help`}
              label={segment.label}
              value={draft[segment.key]}
              onValueChange={(value) => update(segment.key, value)}
              options={SKIP_CHOICES}
            />
          </SettingsRow>
        ))}
      </Section>
      <SaveBar
        dirty={!same(draft, settings.client.playback)}
        saving={mutation.isPending}
        onSave={() => mutation.mutate(draft)}
        onDiscard={() => setDraft(settings.client.playback)}
        onReset={() => setDraft(DEFAULT_PLAYBACK_SETTINGS)}
      />
    </div>
  )
}
