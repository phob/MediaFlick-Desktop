import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Film } from "lucide-react"
import { useContext, useEffect, useId, useMemo, useRef, type MouseEvent } from "react"
import { toast } from "sonner"
import { MediaCard } from "@/components/MediaCard"
import { PreviewProvider, type PreviewDependencies } from "@/components/PreviewCard"
import SaveBar from "@/components/SettingsSaveBar"
import SettingsNumberField from "@/components/SettingsNumberField"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, type AppearanceSettings, type RatingSourceDefinition, type Status } from "@/lib/api"
import { accountKey, useHome, useItem, useNextUp, useRatingsStatus, useSettings, useStatus } from "@/lib/queries"
import { queryKeys } from "@/lib/query-client"
import { RatingsContext, type RatingsContextValue } from "@/lib/rating-context"
import { usePrefersReducedMotion } from "@/lib/reduced-motion"
import { isSettingsNumberValid } from "@/lib/settings-numbers"
import type { CSSVariableProperties } from "@/lib/style"
import { DEFAULT_VIEWING, useViewing } from "@/lib/viewing"
import { DEFAULT_APPEARANCE } from "./defaults"
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

const PREVIEW_DELAY_RANGE = [200, 2000] as const

export function RatingSourceSelector({
  sources,
  selected,
  enabled,
  onChange,
  legend = "Rating sources",
}: {
  sources: RatingSourceDefinition[]
  selected: string[]
  enabled: boolean
  onChange: (sources: string[]) => void
  legend?: string
}) {
  const chosen = new Set(selected)
  const helpId = useId()
  return (
    <fieldset className="rating-source-selector" disabled={!enabled} aria-describedby={helpId}>
      <legend className="sr-only">{legend}</legend>
      <div className="rating-source-options">
        {sources.map((source) => (
          <Label key={source.id} data-selected={chosen.has(source.id)} className="leading-normal">
            <Checkbox
              aria-label={source.label}
              disabled={!enabled}
              checked={chosen.has(source.id)}
              onCheckedChange={(checked) => {
                const next = checked === true
                  ? [...selected, source.id]
                  : selected.filter((id) => id !== source.id)
                onChange([...new Set(next)])
              }}
            />
            <span>
              <strong>{source.label}</strong>
              <small>
                {source.format === "percent" ? "0–100%" : `0–${source.scaleMax}`}
                {!source.known ? " · newly observed" : ""}
              </small>
            </span>
          </Label>
        ))}
      </div>
      <p id={helpId} className="mt-3 text-xs text-muted-foreground">
        {enabled
          ? "A source that has no rating for a title is simply not shown."
          : "MDBList rating sources are unavailable for this server."}
      </p>
    </fieldset>
  )
}

/**
 * The live preview is the app itself, scaled to a shelf: real `MediaCard`s fed
 * by the same cached home feed the Home page uses, wrapped in one container
 * that carries the unsaved draft as data attributes and intensity variables.
 * The shared token rules re-skin that subtree exactly as they re-skin the root,
 * so every choice here can be judged against your own artwork before saving.
 */
const NO_RATINGS: RatingsContextValue = {
  items: new Map(),
  selected: [],
  definitions: new Map(),
  register: () => () => {},
}

/**
 * The preview shelf hovers for real — the same delay, the same panel, the same
 * hold-open behavior as Home — but it remains a picture of the app: the
 * panel's Play, My List, and watched buttons render and disable exactly as
 * they do on a shelf yet mutate nothing, so resting on a card cannot launch a
 * film or rewrite watch state from the settings page. Detail and next-up reads
 * stay live so the panel fills with the same facts it carries on Home.
 */
const PREVIEW_DEPENDENCIES: PreviewDependencies = {
  item: useItem,
  nextUp: useNextUp,
  play: () => ({ isPending: false, mutate: () => {} }),
  favorite: () => ({ isPending: false, mutate: () => {} }),
  played: () => ({ isPending: false, mutate: () => {} }),
}

/**
 * The expanded panel portals outside this page: `content-viewport` declares
 * paint containment, which would clip a fixed panel and re-anchor it to the
 * scrolling pane. One host div at the body level gives the panel the same
 * escape the real app's body portal provides, while carrying the draft
 * appearance attributes used by the shelf.
 * Only one preview exists at a time, so a single module-level host is enough;
 * mounting attaches it to the body and unmounting removes it again.
 */
const PANEL_HOST = document.createElement("div")

function AppearancePreview({ appearance, previewDelay }: { appearance: AppearanceSettings; previewDelay?: number }) {
  const systemReducedMotion = usePrefersReducedMotion()
  const reducedMotion = systemReducedMotion || appearance.reducedMotion
  const home = useHome()
  const savedRatings = useContext(RatingsContext)
  // The unsaved source selection drives which overlays render; the fetched
  // ratings and the shared request scheduler stay the provider's own, so the
  // preview never issues requests of its own.
  const draftRatings = useMemo<RatingsContextValue | null>(
    () => savedRatings && { ...savedRatings, selected: appearance.ratingSources },
    [savedRatings, appearance.ratingSources],
  )
  // A demo picture of the app must not act from its shelf: the card links are
  // marked inert one by one below — inert on the whole shelf would take the
  // pointer out of play too, and with it the hover state this preview exists
  // to show — and this capture handler is the belt to those braces for
  // everything else, such as the inline actions. The expanded panel keeps its
  // real hover behavior and details link; only its state-changing actions are
  // inerted, through PREVIEW_DEPENDENCIES above.
  const holdStill = (event: MouseEvent) => {
    event.preventDefault()
    event.stopPropagation()
  }
  useEffect(() => {
    document.body.appendChild(PANEL_HOST)
    return () => PANEL_HOST.remove()
  }, [])
  useEffect(() => {
    PANEL_HOST.dataset.accent = appearance.accent
    PANEL_HOST.dataset.density = appearance.density
    PANEL_HOST.dataset.reducedMotion = String(reducedMotion)
    PANEL_HOST.style.setProperty("--artwork-intensity", String(appearance.artworkIntensity / 100))
    PANEL_HOST.style.setProperty("--backdrop-intensity", String(appearance.backdropIntensity / 100))
  }, [appearance.accent, appearance.density, appearance.artworkIntensity, appearance.backdropIntensity, reducedMotion])
  const shelfRef = useRef<HTMLDivElement>(null)
  // Inert per link keeps it out of tab order, activation, and hit-testing, so
  // a pointer over the artwork targets the card around it and the real hover
  // rules fire exactly as they do on an actual shelf.
  useEffect(() => {
    for (const link of shelfRef.current?.querySelectorAll("a") ?? []) {
      link.setAttribute("inert", "")
    }
  })
  const style: CSSVariableProperties = {
    "--artwork-intensity": String(appearance.artworkIntensity / 100),
    "--backdrop-intensity": String(appearance.backdropIntensity / 100),
  }
  const resume = home.data?.continueWatching[0]
  const recent = home.data?.rows.find((row) => row.id === "recentlyAdded")?.items.slice(0, 4) ?? []

  return (
    <figure
      className="appearance-preview"
      data-accent={appearance.accent}
      data-density={appearance.density}
      data-reduced-motion={reducedMotion}
      data-card-previews={appearance.cardPreviews}
      data-media-info={appearance.showMediaInfo}
      style={style}
      aria-labelledby="appearance-preview-title"
      aria-describedby="appearance-preview-description appearance-preview-motion-status"
    >
      <figcaption className="sr-only">
        <span id="appearance-preview-title">Live appearance preview</span>
        <span id="appearance-preview-description">
          Your own library rendered with the unsaved appearance choices; the rest
          of MediaFlick changes after Save.
        </span>
      </figcaption>
      <PreviewProvider
        enabled={appearance.cardPreviews}
        delay={previewDelay}
        container={PANEL_HOST}
        dependencies={PREVIEW_DEPENDENCIES}
      >
        <div className="appearance-preview-stage" aria-hidden>
          <div className="appearance-preview-backdrop" />
          <header className="appearance-preview-chrome">
            <span className="appearance-preview-brand"><Film /></span>
            <span className="appearance-preview-wordmark">Media<span>Flick</span></span>
            <span>Home</span>
            <span>Movies</span>
            <span>Series</span>
            <span className="appearance-preview-online">Online</span>
          </header>
          <div ref={shelfRef} className="appearance-preview-shelf" onClickCapture={holdStill}>
            <RatingsContext.Provider value={draftRatings ?? NO_RATINGS}>
              <div className="flex items-center gap-3">
                <span className="rail-marker" />
                <h3 className="text-base font-semibold tracking-tight">Recently added</h3>
                <span className="rail-rule min-w-6 flex-1" />
              </div>
              {home.isPending ? (
                <div className="flex gap-[var(--card-gap)] overflow-hidden pt-1">
                  {Array.from({ length: 4 }, (_, index) => (
                    <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
                  ))}
                </div>
              ) : recent.length === 0 ? (
                <p className="appearance-preview-empty">
                  Your shelves appear here once your library has loaded.
                </p>
              ) : (
                <div className="flex gap-[var(--card-gap)] overflow-hidden pt-1 pb-1">
                  {resume && <MediaCard item={resume} landscape className="home-media-card shrink-0" />}
                  {recent.map((item) => (
                    <MediaCard key={item.id} item={item} className="home-media-card shrink-0" />
                  ))}
                </div>
              )}
            </RatingsContext.Provider>
          </div>
        </div>
      </PreviewProvider>
      <div className="appearance-preview-motion">
        <span className="appearance-preview-motion-label">Motion</span>
        <span
          key={String(reducedMotion)}
          className="appearance-preview-motion-track"
          aria-hidden
        >
          <span />
        </span>
        <span id="appearance-preview-motion-status">
          {reducedMotion
            ? `Reduced${systemReducedMotion ? " by your operating system" : ""}`
            : "One gentle transition"}
        </span>
      </div>
    </figure>
  )
}

export function Appearance() {
  const statusQuery = useStatus()
  const { data: status } = statusQuery
  const settingsQuery = useSettings()
  const viewing = useViewing()
  const cache = useQueryClient()
  const account = accountKey(status)
  const [previewDelay, setPreviewDelay, , acceptDelay] = useSourceDraft(viewing.data?.previewDelayMs, account)
  const ratingsQuery = useRatingsStatus(Boolean(status?.authenticated))
  const { data: settings } = settingsQuery
  const { data: ratings } = ratingsQuery
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.appearance, account)
  const mutation = useMutation({
    mutationFn: async (submitted: { appearance: AppearanceSettings; previewDelay: number | undefined }) => {
      const checkAccount = () => {
        if (accountKey(cache.getQueryData<Status>(queryKeys.status)) !== account) {
          throw new Error("The signed-in account changed. Save these settings again.")
        }
      }
      checkAccount()
      if (!same(submitted.appearance, settings?.appearance)) {
        const saved = await api.settingsPatch.appearance(submitted.appearance)
        checkAccount()
        cache.setQueryData(queryKeys.settings, saved)
        acceptSaved(saved.appearance, submitted.appearance)
      }
      if (submitted.previewDelay !== undefined && submitted.previewDelay !== viewing.data?.previewDelayMs) {
        const current = await api.viewing()
        checkAccount()
        const saved = await api.saveViewing({ ...current, previewDelayMs: submitted.previewDelay })
        checkAccount()
        cache.setQueryData(queryKeys.viewing(account), saved)
        acceptDelay(saved.previewDelayMs, submitted.previewDelay)
      }
    },
    onSuccess: () => toast.success("Appearance settings saved"),
    onError: (error: Error) => toast.error(error.message),
  })
  if (statusQuery.error && !status) {
    return (
      <SettingsError
        title="Appearance unavailable"
        error={statusQuery.error}
        onRetry={() => void statusQuery.refetch()}
      />
    )
  }
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Appearance" />
  if (settingsQuery.error && !settings) {
    return (
      <SettingsError
        title="Appearance settings unavailable"
        error={settingsQuery.error}
        onRetry={() => void settingsQuery.refetch()}
      />
    )
  }
  if (!settings || !draft) return <SettingsLoading />

  const update = <Key extends keyof AppearanceSettings>(key: Key, value: AppearanceSettings[Key]) =>
    setDraft({ ...draft, [key]: value })
  const validIntensity = (value: number) => isSettingsNumberValid(value, 0, 100)
  const validDelay = previewDelay === undefined || isSettingsNumberValid(previewDelay, ...PREVIEW_DELAY_RANGE)
  // The preview keeps the last saved value for a number that is mid-edit.
  const previewAppearance = {
    ...draft,
    artworkIntensity: validIntensity(draft.artworkIntensity)
      ? draft.artworkIntensity
      : settings.appearance.artworkIntensity,
    backdropIntensity: validIntensity(draft.backdropIntensity)
      ? draft.backdropIntensity
      : settings.appearance.backdropIntensity,
  }
  const effectiveDelay = previewDelay !== undefined && validDelay ? previewDelay : viewing.data?.previewDelayMs

  return (
    <div className="settings-page">
      <PageTitle title="Appearance" />
      <Section
        title="Live preview"
        description="Your own shelves with your unsaved choices applied here only; the rest of MediaFlick changes after Save."
      >
        <AppearancePreview appearance={previewAppearance} previewDelay={effectiveDelay} />
      </Section>
      <Section title="Style" description="Customize MediaFlick's dark appearance.">
        <SettingsRow
          controlId="settings-accent"
          title="Accent"
          description="The signal color used for active controls and focus rings."
        >
          <SelectField
            id="settings-accent"
            aria-describedby="settings-accent-help"
            label="Accent"
            value={draft.accent}
            onValueChange={(accent) => update("accent", accent)}
            options={[
              { value: "signal", label: "Signal" },
              { value: "cobalt", label: "Cobalt" },
              { value: "amber", label: "Amber" },
              { value: "violet", label: "Violet" },
            ]}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-density"
          title="Density"
          description="Compact reduces the spacing used by browsing and settings surfaces."
        >
          <SelectField
            id="settings-density"
            aria-describedby="settings-density-help"
            label="Density"
            value={draft.density}
            onValueChange={(density) => update("density", density)}
            options={[
              { value: "comfortable", label: "Comfortable" },
              { value: "compact", label: "Compact" },
            ]}
          />
        </SettingsRow>
      </Section>
      <Section title="Cards" description="Choose how library cards behave and what they show.">
        <SettingsRow
          controlId="settings-card-previews"
          title="Card previews"
          description="Open a larger panel after resting the pointer on a card. When off, Play, My List, and watched buttons stay on the card."
        >
          <Switch
            id="settings-card-previews"
            aria-describedby="settings-card-previews-help"
            aria-label="Show pop-out previews on cards"
            checked={draft.cardPreviews}
            onCheckedChange={(cardPreviews) => update("cardPreviews", cardPreviews)}
          />
        </SettingsRow>
        <SettingsRow
          controlId="card-preview-delay"
          title="Card preview delay"
          description="Milliseconds before a card preview opens."
        >
          <SettingsNumberField
            id="card-preview-delay"
            aria-describedby="card-preview-delay-help"
            label="Card preview delay"
            min={PREVIEW_DELAY_RANGE[0]}
            max={PREVIEW_DELAY_RANGE[1]}
            sliderStep={50}
            disabled={!draft.cardPreviews || previewDelay === undefined}
            validate={previewDelay !== undefined}
            value={previewDelay ?? NaN}
            onValueChange={setPreviewDelay}
          />
          {viewing.error && (
            <Button variant="ghost" onClick={() => void viewing.refetch()}>Retry loading delay</Button>
          )}
        </SettingsRow>
        <SettingsRow
          controlId="settings-media-info"
          title="Media info"
          description="Show video resolution, dynamic range, and audio format on library cards."
        >
          <Switch
            id="settings-media-info"
            aria-describedby="settings-media-info-help"
            aria-label="Show media info on cards"
            checked={draft.showMediaInfo}
            onCheckedChange={(showMediaInfo) => update("showMediaInfo", showMediaInfo)}
          />
        </SettingsRow>
        <div className="border-t border-border pt-5">
          <h3 className="font-medium">Rating sources</h3>
          <p className="mt-1 mb-4 text-sm text-muted-foreground">
            Choose any combination of MDBList sources for compact top-left card overlays.
          </p>
          {ratingsQuery.error && !ratings ? (
            <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-sm">
              <span>Rating source status could not be loaded.</span>
              <Button size="sm" variant="outline" onClick={() => void ratingsQuery.refetch()}>Try again</Button>
            </div>
          ) : (
            <RatingSourceSelector
              sources={ratings?.sources ?? []}
              selected={draft.ratingSources}
              enabled={Boolean(ratings?.selectionEnabled)}
              onChange={(ratingSources) => update("ratingSources", ratingSources)}
            />
          )}
        </div>
      </Section>
      <Section title="Artwork and motion" description="Lower artwork intensity for a quieter browsing surface.">
        <SettingsRow
          controlId="artwork-intensity"
          title="Artwork intensity"
          description="Percent of the original artwork intensity."
        >
          <SettingsNumberField
            id="artwork-intensity"
            label="Artwork intensity"
            unit="percent"
            aria-describedby="artwork-intensity-help"
            min={0}
            max={100}
            value={draft.artworkIntensity}
            onValueChange={(artworkIntensity) => update("artworkIntensity", artworkIntensity)}
          />
        </SettingsRow>
        <SettingsRow
          controlId="backdrop-intensity"
          title="Backdrop intensity"
          description="Percent of the original artwork intensity."
        >
          <SettingsNumberField
            id="backdrop-intensity"
            label="Backdrop intensity"
            unit="percent"
            aria-describedby="backdrop-intensity-help"
            min={0}
            max={100}
            value={draft.backdropIntensity}
            onValueChange={(backdropIntensity) => update("backdropIntensity", backdropIntensity)}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-reduce-motion"
          title="Reduce motion"
          description="Disable decorative transitions and automatic movement."
        >
          <Switch
            id="settings-reduce-motion"
            aria-describedby="settings-reduce-motion-help"
            aria-label="Reduce motion"
            checked={draft.reducedMotion}
            onCheckedChange={(reducedMotion) => update("reducedMotion", reducedMotion)}
          />
        </SettingsRow>
      </Section>
      <SaveBar
        saveDisabled={!validIntensity(draft.artworkIntensity) || !validIntensity(draft.backdropIntensity) || !validDelay}
        dirty={!same(draft, settings.appearance) || previewDelay !== viewing.data?.previewDelayMs}
        saving={mutation.isPending}
        onSave={() => mutation.mutate({ appearance: draft, previewDelay })}
        onDiscard={() => {
          setDraft(settings.appearance)
          setPreviewDelay(viewing.data?.previewDelayMs)
        }}
        onReset={() => {
          if (previewDelay !== undefined) setPreviewDelay(DEFAULT_VIEWING.previewDelayMs)
          setDraft(DEFAULT_APPEARANCE)
        }}
      />
    </div>
  )
}
