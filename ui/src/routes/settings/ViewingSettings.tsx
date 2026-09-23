import { useMutation, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import SettingsNumberField from "@/components/SettingsNumberField"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, type ViewingSettings as ViewingDraft } from "@/lib/api"
import { accountKey, useStatus } from "@/lib/queries"
import { queryKeys } from "@/lib/query-client"
import { isSettingsNumberValid } from "@/lib/settings-numbers"
import { DEFAULT_VIEWING, useViewing } from "@/lib/viewing"
import { same } from "./helpers"
import {
  PageTitle,
  Section,
  SelectField,
  SettingsError,
  SettingsLoading,
  SettingsRow,
  SignInRequired,
} from "./shared"

const POSTER_WIDTHS = [
  [120, "Small"],
  [144, "Medium"],
  [168, "Default"],
  [200, "Large"],
  [240, "Extra large"],
] as const

/** Language codes typed as a comma list, normalized the way the shell stores them. */
function languageList(text: string | undefined) {
  return (text ?? "").split(",").map((value) => value.trim().toLowerCase()).filter(Boolean)
}

function posterWidthOptions(current: number) {
  const known = POSTER_WIDTHS.some(([width]) => width === current)
  return [
    // A width saved by an older build stays visible instead of reading as blank.
    ...(known ? [] : [{ value: String(current), label: `Current — ${current} px`, disabled: true }]),
    ...POSTER_WIDTHS.map(([width, name]) => ({ value: String(width), label: `${name} — ${width} px` })),
  ]
}

export default function ViewingSettings() {
  const cache = useQueryClient()
  const query = useViewing()
  const { data: status } = useStatus()
  const account = accountKey(status)
  const [draft, setDraft, , acceptSaved] = useSourceDraft(query.data, account)
  const [audioText, setAudioText, , acceptAudio] = useSourceDraft(query.data?.audioLanguages.join(", "), account)
  const [subtitleText, setSubtitleText, , acceptSubtitles] =
    useSourceDraft(query.data?.subtitleLanguages.join(", "), account)
  const save = useMutation({
    mutationFn: api.saveViewing,
    onSuccess: (saved, submitted) => {
      cache.setQueryData(queryKeys.viewing(account), saved)
      acceptSaved(saved, submitted)
      acceptAudio(saved.audioLanguages.join(", "), submitted.audioLanguages.join(", "))
      acceptSubtitles(saved.subtitleLanguages.join(", "), submitted.subtitleLanguages.join(", "))
      toast.success("Viewing settings saved")
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (!status?.authenticated) return <SignInRequired name="Viewing" />
  if (query.error && !draft) return <SettingsError error={query.error} onRetry={() => void query.refetch()} />
  if (!draft) return <SettingsLoading />
  const update = <Key extends keyof ViewingDraft>(key: Key, value: ViewingDraft[Key]) =>
    setDraft({ ...draft, [key]: value })
  const numbersValid = isSettingsNumberValid(draft.countdownSeconds, 3, 60) &&
    isSettingsNumberValid(draft.episodeLimit, 0, 20) &&
    isSettingsNumberValid(draft.textScale, 80, 150)
  const dirty = !same(draft, query.data) ||
    audioText !== query.data?.audioLanguages.join(", ") ||
    subtitleText !== query.data?.subtitleLanguages.join(", ")
  const saveDraft = () => {
    const submitted = {
      ...draft,
      audioLanguages: languageList(audioText),
      subtitleLanguages: languageList(subtitleText),
    }
    setDraft(submitted)
    setAudioText(submitted.audioLanguages.join(", "))
    setSubtitleText(submitted.subtitleLanguages.join(", "))
    save.mutate(submitted)
  }

  return (
    <div className="settings-page">
      <PageTitle title="Viewing" />
      <Section title="Episodes">
        <SettingsRow
          controlId="settings-spoiler-protection"
          title="Spoiler protection"
          description="Hide unwatched episode titles, artwork, and summaries. Reveal them on the episode details page."
        >
          <Switch
            id="settings-spoiler-protection"
            aria-describedby="settings-spoiler-protection-help"
            aria-label="Spoiler protection"
            checked={draft.spoilerProtection}
            onCheckedChange={(value) => update("spoilerProtection", value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-next-episode" title="Next episode">
          <SelectField
            id="settings-next-episode"
            label="Next episode"
            value={draft.nextEpisode}
            onValueChange={(value) => update("nextEpisode", value)}
            options={[
              { value: "off", label: "Off" },
              { value: "ask", label: "Ask with countdown" },
              { value: "auto", label: "Automatically play" },
            ]}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-countdown-seconds" title="Countdown seconds">
          <SettingsNumberField
            id="settings-countdown-seconds"
            label="Countdown seconds"
            min={3}
            max={60}
            value={draft.countdownSeconds}
            onValueChange={(value) => update("countdownSeconds", value)}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-episode-limit"
          title="Episode limit"
          description="Stop continuous playback after this many episodes. Zero means unlimited; starting a title manually begins a new session."
        >
          <SettingsNumberField
            id="settings-episode-limit"
            aria-describedby="settings-episode-limit-help"
            label="Episode limit"
            min={0}
            max={20}
            value={draft.episodeLimit}
            onValueChange={(value) => update("episodeLimit", value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-resume-rewind" title="Resume rewind">
          <SelectField
            id="settings-resume-rewind"
            label="Resume rewind"
            value={String(draft.resumeRewindSeconds)}
            onValueChange={(value) => update("resumeRewindSeconds", Number(value))}
            options={[0, 5, 10, 30].map((value) => ({ value: String(value), label: `${value} seconds` }))}
          />
        </SettingsRow>
      </Section>
      <Section
        title="Languages"
        description="Use language codes in preference order, separated by commas (for example en, de, ja). Individual title choices take priority."
      >
        <SettingsRow controlId="settings-audio-languages" title="Audio languages">
          <Input
            id="settings-audio-languages"
            aria-label="Audio languages"
            value={audioText ?? ""}
            onChange={(event) => setAudioText(event.target.value)}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-prefer-original-audio"
          title="Prefer original audio"
          description="Prefer a track explicitly labeled original when available."
        >
          <Switch
            id="settings-prefer-original-audio"
            aria-describedby="settings-prefer-original-audio-help"
            aria-label="Prefer original audio"
            checked={draft.preferOriginalAudio}
            onCheckedChange={(value) => update("preferOriginalAudio", value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-subtitle-languages" title="Subtitle languages">
          <Input
            id="settings-subtitle-languages"
            aria-label="Subtitle languages"
            value={subtitleText ?? ""}
            onChange={(event) => setSubtitleText(event.target.value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-subtitles" title="Subtitles">
          <SelectField
            id="settings-subtitles"
            label="Subtitles"
            value={draft.subtitleMode}
            onValueChange={(value) => update("subtitleMode", value)}
            options={[
              { value: "server", label: "Jellyfin default" },
              { value: "off", label: "Off" },
              { value: "forced", label: "Forced only" },
              { value: "always", label: "Always" },
              { value: "foreignAudio", label: "When audio differs from preferred languages" },
            ]}
          />
        </SettingsRow>
      </Section>
      <Section title="Browsing">
        <SettingsRow controlId="settings-text-size" title="Text size">
          <SettingsNumberField
            id="settings-text-size"
            label="Text size percent"
            min={80}
            max={150}
            value={draft.textScale}
            onValueChange={(value) => update("textScale", value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-poster-width" title="Poster width">
          <SelectField
            id="settings-poster-width"
            label="Poster width"
            value={String(draft.posterSize)}
            onValueChange={(value) => update("posterSize", Number(value))}
            options={posterWidthOptions(draft.posterSize)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-startup-destination" title="Startup destination">
          <SelectField
            id="settings-startup-destination"
            label="Startup destination"
            value={draft.startupDestination}
            onValueChange={(value) => update("startupDestination", value)}
            options={[
              { value: "home", label: "Home" },
              { value: "movies", label: "Movies" },
              { value: "series", label: "Series" },
              { value: "calendar", label: "Calendar" },
              { value: "last", label: "Last browsing page" },
            ]}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-remember-library-filters"
          title="Remember library filters"
          description="Keep separate sort and filters for Movies and Series."
        >
          <Switch
            id="settings-remember-library-filters"
            aria-describedby="settings-remember-library-filters-help"
            aria-label="Remember library filters"
            checked={draft.rememberFilters}
            onCheckedChange={(value) => update("rememberFilters", value)}
          />
        </SettingsRow>
        <SettingsRow controlId="settings-hide-watched-by-default" title="Hide watched by default">
          <Switch
            id="settings-hide-watched-by-default"
            aria-label="Hide watched by default"
            checked={draft.hideWatched}
            onCheckedChange={(value) => update("hideWatched", value)}
          />
        </SettingsRow>
      </Section>
      <SaveBar
        saveDisabled={!numbersValid}
        dirty={dirty}
        saving={save.isPending}
        onSave={saveDraft}
        onDiscard={() => {
          setDraft(query.data)
          setAudioText(query.data?.audioLanguages.join(", "))
          setSubtitleText(query.data?.subtitleLanguages.join(", "))
        }}
        onReset={() => {
          // The card preview delay is edited on the Appearance page.
          setDraft({ ...DEFAULT_VIEWING, previewDelayMs: query.data?.previewDelayMs ?? DEFAULT_VIEWING.previewDelayMs })
          setAudioText("")
          setSubtitleText("")
        }}
      />
    </div>
  )
}
