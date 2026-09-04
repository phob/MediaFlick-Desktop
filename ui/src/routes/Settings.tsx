import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  Download,
  ExternalLink,
  Film,
  FolderOpen,
  GripVertical,
  House,
  Link,
  Layers,
  Monitor,
  Palette,
  Play,
  Plug,
  RefreshCw,
  SlidersHorizontal,
  Trash2,
  type LucideIcon,
} from "lucide-react"
import {
  Fragment,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react"
import { createPortal } from "react-dom"
import { Link as RouterLink, Navigate, Route, Routes, useLocation } from "react-router-dom"
import { toast } from "sonner"
import { MediaCard } from "@/components/MediaCard"
import { PreviewProvider, type PreviewDependencies } from "@/components/PreviewCard"
import SaveBar from "@/components/SettingsSaveBar"
import SettingsDraftGuard from "@/components/SettingsDraftGuard"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { useSourceDraft } from "@/hooks/use-source-draft"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  api,
  ApiError,
  homeSettingsWrite,
  playerSettingsWrite,
  type AppearanceSettings,
  type ClientSettings,
  type CompanionService,
  type HomeConfiguration,
  type LetterboxdProfile,
  type RatingSourceDefinition,
  type Status,
} from "@/lib/api"
import { jsonNumber, jsonString } from "@/lib/json"
import { queryClient, queryKeys, removeAccountQueryData } from "@/lib/query-client"
import { RatingsContext, type RatingsContextValue } from "@/lib/rating-context"
import { collectionAccountKey, useCompanion, useHome, useHomeSettings, useItem, useNextUp, useRatingsStatus, useSeerrStatus, useSettings, useStatus } from "@/lib/queries"
import { usePrefersReducedMotion } from "@/lib/reduced-motion"
import { readShellEvent, type ShellEvent } from "@/lib/shell-events"
import type { CSSVariableProperties } from "@/lib/style"
import CollectionSettingsPage from "@/routes/CollectionSettings"

type SettingsPage = {
  to: string
  title: string
  icon: LucideIcon
  group?: string
  signedIn?: boolean
}

const NAVIGATION: SettingsPage[] = [
  { to: "/settings/client/player", title: "Player", icon: Play, group: "Client" },
  { to: "/settings/client/playback", title: "Playback", icon: SlidersHorizontal, group: "Client" },
  { to: "/settings/client/application", title: "Application", icon: Monitor, group: "Client" },
  { to: "/settings/home", title: "Home", icon: House, signedIn: true, group: "Account" },
  { to: "/settings/appearance", title: "Appearance", icon: Palette, signedIn: true, group: "Account" },
  { to: "/settings/collections", title: "Collections", icon: Layers, signedIn: true, group: "Account" },
  { to: "/settings/integrations/companion", title: "MediaFlick Companion", icon: Plug, signedIn: true, group: "Integrations" },
  { to: "/settings/integrations/letterboxd", title: "Letterboxd", icon: Link, signedIn: true, group: "Integrations" },
]

function same<T>(left: T, right: T) {
  return JSON.stringify(left) === JSON.stringify(right)
}

function saveSettings(saved: ClientSettings, message = "Settings saved") {
  queryClient.setQueryData(queryKeys.settings, saved)
  toast.success(message)
}

function SettingsRow({
  title,
  description,
  controlId,
  children,
}: {
  title: string
  description: string
  controlId?: string
  children: ReactNode
}) {
  return (
    <div className="settings-row">
      <div className="min-w-0">
        <h3 className="font-medium">{controlId ? <Label htmlFor={controlId}>{title}</Label> : title}</h3>
        <p id={controlId ? `${controlId}-help` : undefined} className="mt-1 text-sm text-muted-foreground">{description}</p>
      </div>
      <div className="settings-control">{children}</div>
    </div>
  )
}

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
          <label key={source.id} data-selected={chosen.has(source.id)}>
            <Checkbox
              aria-label={source.label}
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
          </label>
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

type SelectOption<Value extends string> = {
  value: Value
  label: string
  disabled?: boolean
}

function SelectField<const Value extends string>({
  value,
  onValueChange,
  options,
  label,
}: {
  value: Value
  onValueChange: (value: Value) => void
  options: readonly SelectOption<Value>[]
  label: string
}) {
  const selectOption = (candidate: string) => {
    const selected = options.find((option) => option.value === candidate)?.value
    if (selected !== undefined) onValueChange(selected)
  }
  return (
    <Select value={value} onValueChange={selectOption}>
      <SelectTrigger aria-label={label} className="w-52 max-w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value} disabled={option.disabled}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function Section({ title, description, children }: { title: string; description: string; children: ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">{children}</CardContent>
    </Card>
  )
}

function SettingsLoading() {
  return <div className="settings-page text-sm text-muted-foreground" role="status">Loading settings…</div>
}

function SettingsError({ title = "Settings unavailable", error, onRetry }: { title?: string; error: Error; onRetry: () => void }) {
  return (
    <div className="settings-page">
      <PageTitle title={title} detail="MediaFlick could not load the saved state for this page." />
      <Section title="Could not load settings" description={error.message}>
        <Button variant="outline" onClick={onRetry}><RefreshCw /> Try again</Button>
      </Section>
    </div>
  )
}

function PageTitle({ title, detail }: { title: string; detail: string }) {
  return <header className="settings-page-title"><h1>{title}</h1><p>{detail}</p></header>
}

function useShellEvents(listener: (event: ShellEvent) => void) {
  useEffect(() => {
    const receive = (event: Event) => {
      const shellEvent = readShellEvent(event)
      if (shellEvent) listener(shellEvent)
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [listener])
}

function requestId() {
  return crypto.randomUUID?.().replaceAll("-", "") ?? `${Date.now()}${Math.random().toString(16).slice(2)}`
}

function PlayerSettings() {
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const [draft, setDraft, updateDraft, acceptSaved] = useSourceDraft(settings?.client.player)
  const [install, setInstall] = useState<{ state: string; message?: string; downloaded?: number; total?: number | null }>({ state: "idle" })
  const pendingPickers = useRef<Partial<Record<"mpv" | "mpchc", string>>>({})
  const pendingInstall = useRef<string | null>(null)
  const [picking, setPicking] = useState<Partial<Record<"mpv" | "mpchc", boolean>>>({})
  const mutation = useMutation({
    mutationFn: (value: ClientSettings["client"]["player"]) => api.settingsPatch.player(playerSettingsWrite(value)),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.client.player, submitted)
      saveSettings(
        saved,
        submitted.playerBackend !== settings?.client.player.playerBackend &&
          (submitted.playerBackend === "libmpv" || settings?.client.player.playerBackend === "libmpv")
          ? "Player saved. Restart MediaFlick to apply the built-in player configuration."
          : "Player settings saved",
      )
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const onShellEvent = useCallback((event: ShellEvent) => {
    if (event.type === "file-picker-completed") {
      const target = jsonString(event.payload.target)
      const completedRequestId = jsonString(event.payload.requestId)
      if (
        (target !== "mpv" && target !== "mpchc") ||
        completedRequestId === null ||
        pendingPickers.current[target] !== completedRequestId
      ) {
        return
      }
      delete pendingPickers.current[target]
      setPicking((current) => ({ ...current, [target]: false }))

      const pickerError = jsonString(event.payload.error)
      if (pickerError) {
        toast.error(pickerError)
        return
      }
      // A null path is the native dialog's cancellation result. It settles the
      // request but deliberately leaves the user's existing draft untouched.
      const path = jsonString(event.payload.path)
      if (path === null) return
      if (target === "mpv") updateDraft((current) => current ? { ...current, mpvPath: path } : current)
      if (target === "mpchc") updateDraft((current) => current ? { ...current, mpchcPath: path } : current)
    }
    if (event.type === "mpv-install-progress") {
      const completedRequestId = jsonString(event.payload.requestId)
      if (
        completedRequestId === null ||
        pendingInstall.current !== completedRequestId
      ) {
        return
      }
      const state = jsonString(event.payload.state) ?? "idle"
      const message = jsonString(event.payload.message) ?? undefined
      const downloaded = jsonNumber(event.payload.downloaded) ?? 0
      const total = jsonNumber(event.payload.total)
      setInstall({ state, message, downloaded, total })
      const installedPath = jsonString(event.payload.path)
      if (state === "completed" && installedPath !== null) {
        pendingInstall.current = null
        updateDraft((current) => current ? { ...current, mpvPath: installedPath } : current)
        void queryClient.invalidateQueries({ queryKey: queryKeys.settings })
      }
      if (state === "failed") {
        pendingInstall.current = null
        if (message) toast.error(message)
      }
    }
  }, [updateDraft])
  useShellEvents(onShellEvent)
  if (settingsQuery.error && !settings) return <SettingsError title="Player settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  const dirty = !same(draft, settings.client.player)
  const backendChanged = draft.playerBackend !== settings.client.player.playerBackend
  const backendChangeNeedsRestart = backendChanged &&
    (draft.playerBackend === "libmpv" || settings.client.player.playerBackend === "libmpv")
  const restartMessage = backendChangeNeedsRestart
    ? draft.playerBackend === "libmpv"
      ? "Restart MediaFlick to enable the built-in player."
      : "Restart MediaFlick to switch player backends."
    : undefined
  const pick = (target: "mpv" | "mpchc") => {
    if (pendingPickers.current[target]) return
    const id = requestId()
    pendingPickers.current[target] = id
    setPicking((current) => ({ ...current, [target]: true }))
    void api.shell.filePicker(id, target).then((response) => {
      if (response.requestId === id) return
      if (pendingPickers.current[target] === id) {
        delete pendingPickers.current[target]
        setPicking((current) => ({ ...current, [target]: false }))
      }
      toast.error("The file picker returned an unexpected request identifier.")
    }).catch((error: Error) => {
      if (pendingPickers.current[target] === id) {
        delete pendingPickers.current[target]
        setPicking((current) => ({ ...current, [target]: false }))
      }
      toast.error(error.message)
    })
  }
  const installMpv = () => {
    if (pendingInstall.current) return
    const id = requestId()
    pendingInstall.current = id
    setInstall({ state: "queued" })
    void api.shell.installMpv(id).then((response) => {
      if (response.requestId === id) return
      if (pendingInstall.current === id) pendingInstall.current = null
      const message = "The mpv installer returned an unexpected request identifier."
      setInstall({ state: "failed", message })
      toast.error(message)
    }).catch((error: Error) => {
      if (pendingInstall.current === id) pendingInstall.current = null
      setInstall({ state: "failed", message: error.message })
      toast.error(error.message)
    })
  }
  const installDetail = install.state === "downloading" && install.total
    ? `Downloading mpv (${Math.round((install.downloaded ?? 0) / install.total * 100)}%)`
    : install.state === "extracting" ? "Extracting mpv…"
    : install.state === "completed" ? "mpv installed. Save to keep any other player changes."
    : install.state === "failed" ? install.message : undefined
  return (
    <div className="settings-page">
      <PageTitle title="Player" detail="Use MediaFlick's built-in player or hand playback to an external app." />
      <Section title="Playback backend" description="The built-in libmpv player works without a separate mpv installation.">
        <SettingsRow title="Player" description="External mpv keeps its own config, scripts, shaders, and SVP setup.">
          <SelectField label="Player backend" value={draft.playerBackend} onValueChange={(playerBackend) => setDraft({ ...draft, playerBackend })} options={[{ value: "libmpv", label: "Built-in player", disabled: !settings.capabilities.libmpv }, { value: "mpv", label: "External mpv" }, { value: "mpchc", label: "MPC-HC", disabled: !settings.capabilities.mpchc }]} />
        </SettingsRow>
        <SettingsRow title="Start fullscreen" description="Use a full-screen player window by default.">
          <SelectField label="Default fullscreen" value={draft.defaultFullscreen} onValueChange={(defaultFullscreen) => setDraft({ ...draft, defaultFullscreen })} options={[{ value: "fullscreen", label: "Fullscreen" }, { value: "windowed", label: "Windowed" }]} />
        </SettingsRow>
        {draft.playerBackend !== "mpchc" && <SettingsRow controlId="mark-watched-key" title="Mark watched key" description="The mpv key that marks the current title watched and plays the next item. Leave blank to disable it.">
          <Input id="mark-watched-key" aria-describedby="mark-watched-key-help" className="w-52" value={draft.markWatchedNext ?? ""} onChange={(event) => setDraft({ ...draft, markWatchedNext: event.target.value || null })} placeholder="w" />
        </SettingsRow>}
      </Section>
      {draft.playerBackend !== "libmpv" && <Section title="Executables" description="Paths are saved locally and are never sent to your Jellyfin server.">
        {draft.playerBackend === "mpv" && <SettingsRow controlId="mpv-path" title="mpv executable" description="Select mpv.exe or use the installer on supported Windows builds.">
          <div className="flex w-full max-w-md gap-2"><Input id="mpv-path" aria-describedby="mpv-path-help" value={draft.mpvPath ?? ""} onChange={(event) => setDraft({ ...draft, mpvPath: event.target.value || null })} placeholder="Path to mpv" /><Button variant="outline" size="icon" aria-label="Choose mpv executable" aria-busy={picking.mpv} disabled={picking.mpv} onClick={() => pick("mpv")}><FolderOpen /></Button></div>
        </SettingsRow>}
        {draft.playerBackend === "mpv" && settings.capabilities.mpvInstaller && <SettingsRow title="Install mpv" description={installDetail ?? "Download and install the supported mpv build beside MediaFlick."}>
          <div className="flex gap-2"><Button variant="outline" onClick={installMpv} disabled={["queued", "downloading", "extracting"].includes(install.state)}><Download /> {install.state === "idle" || install.state === "failed" ? "Install mpv" : "Installing…"}</Button><Button variant="ghost" onClick={() => void api.shell.mpvHelp().catch((error: Error) => toast.error(error.message))}>Installation help</Button></div>
        </SettingsRow>}
        {draft.playerBackend === "mpv" && !settings.capabilities.mpvInstaller && <SettingsRow title="Install mpv" description="See mpv’s installation guide for your operating system."><Button variant="ghost" onClick={() => void api.shell.mpvHelp().catch((error: Error) => toast.error(error.message))}>Installation help</Button></SettingsRow>}
        {draft.playerBackend === "mpchc" && settings.capabilities.mpchc && <SettingsRow controlId="mpchc-path" title="MPC-HC executable" description="Select the MPC-HC executable used for playback.">
          <div className="flex w-full max-w-md gap-2"><Input id="mpchc-path" aria-describedby="mpchc-path-help" value={draft.mpchcPath ?? ""} onChange={(event) => setDraft({ ...draft, mpchcPath: event.target.value || null })} placeholder="Path to MPC-HC" /><Button variant="outline" size="icon" aria-label="Choose MPC-HC executable" aria-busy={picking.mpchc} disabled={picking.mpchc} onClick={() => pick("mpchc")}><FolderOpen /></Button></div>
        </SettingsRow>}
      </Section>}
      <SaveBar dirty={dirty} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.client.player)} onReset={() => setDraft({ ...settings.client.player, playerBackend: settings.capabilities.libmpv ? "libmpv" : "mpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w" })} restartMessage={restartMessage} />
    </div>
  )
}

function PlaybackSettings() {
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.client.playback)
  const mutation = useMutation({ mutationFn: (value: ClientSettings["client"]["playback"]) => api.settingsPatch.playback(value), onSuccess: (saved, submitted) => { acceptSaved(saved.client.playback, submitted); saveSettings(saved) }, onError: (error: Error) => toast.error(error.message) })
  if (settingsQuery.error && !settings) return <SettingsError title="Playback settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  const update = <Key extends keyof typeof draft>(key: Key, value: (typeof draft)[Key]) => setDraft({ ...draft, [key]: value })
  const choices = [{ value: "disabled", label: "Never" }, { value: "prompt", label: "Ask me" }, { value: "always", label: "Always skip" }] as const
  return <div className="settings-page"><PageTitle title="Playback" detail="Set your default stream quality and handling for detected media segments." />
    <Section title="Streaming quality" description="Original sends the source unchanged; lower quality permits transcoding when needed.">
      <SettingsRow title="Default quality" description="You can still override this for an individual play."><SelectField label="Default streaming quality" value={draft.streamingQuality} onValueChange={(value) => update("streamingQuality", value)} options={[{ value: "original", label: "Original" }, { value: "auto", label: "Auto" }, { value: "120_mbps", label: "120 Mbps" }, { value: "80_mbps", label: "80 Mbps" }, { value: "60_mbps", label: "60 Mbps" }, { value: "40_mbps", label: "40 Mbps" }, { value: "20_mbps", label: "20 Mbps" }, { value: "10_mbps", label: "10 Mbps" }, { value: "5_mbps", label: "5 Mbps" }, { value: "3_mbps", label: "3 Mbps" }, { value: "1_5_mbps", label: "1.5 Mbps" }]} /></SettingsRow>
    </Section>
    <Section title="Segment skipping" description="MediaFlick uses Jellyfin segment markers when they are available.">
      <SettingsRow title="Introductions" description="Choose what happens when an intro marker is reached."><SelectField label="Intro skipping" value={draft.skipIntro} onValueChange={(value) => update("skipIntro", value)} options={choices} /></SettingsRow>
      <SettingsRow title="Credits" description="Choose what happens when credits begin."><SelectField label="Credits skipping" value={draft.skipCredits} onValueChange={(value) => update("skipCredits", value)} options={choices} /></SettingsRow>
      <SettingsRow title="Recaps" description="Choose what happens when a recap marker is reached."><SelectField label="Recap skipping" value={draft.skipRecap} onValueChange={(value) => update("skipRecap", value)} options={choices} /></SettingsRow>
      <SettingsRow title="Commercials" description="Choose what happens when a commercial marker is reached."><SelectField label="Commercial skipping" value={draft.skipCommercial} onValueChange={(value) => update("skipCommercial", value)} options={choices} /></SettingsRow>
    </Section>
    <SaveBar dirty={!same(draft, settings.client.playback)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.client.playback)} onReset={() => setDraft({ streamingQuality: "original", skipIntro: "prompt", skipCredits: "prompt", skipRecap: "prompt", skipCommercial: "prompt" })} />
  </div>
}

function ApplicationSettings() {
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const { data: status } = useStatus()
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.client.application)
  const [deleteConfirmation, setDeleteConfirmation] = useState("")
  const mutation = useMutation({ mutationFn: (value: ClientSettings["client"]["application"]) => api.settingsPatch.application(value), onSuccess: (saved, submitted) => { acceptSaved(saved.client.application, submitted); saveSettings(saved) }, onError: (error: Error) => toast.error(error.message) })
  const deleteAccount = useMutation({
    mutationFn: api.collections.deleteLocalAccount,
    onSuccess: (anonymousStatus) => {
      queryClient.setQueryData(queryKeys.status, anonymousStatus)
      removeAccountQueryData()
      void queryClient.resetQueries({ queryKey: queryKeys.settings })
      toast.success("Local account data deleted")
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (settingsQuery.error && !settings) return <SettingsError title="Application settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  return <div className="settings-page"><PageTitle title="Application" detail="Control window behavior and the diagnostics recorded by the desktop client." />
    {(settings.recoveries?.length ?? 0) > 0 && <Section title="Recovered local settings" description="MediaFlick preserved each damaged file before continuing.">
      {settings.recoveries?.map((recovery) => <p key={recovery.area} className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-100" role="status">
        {recovery.area}: {recovery.restoredBackup ? "the last valid backup was restored." : "defaults are in use because no valid backup was available."}
      </p>)}
    </Section>}
    <Section title="Window" description="These choices are applied immediately after saving.">
      <SettingsRow title="When the window closes" description="Minimize keeps MediaFlick and its player ready in the background."><SelectField label="Close behavior" value={draft.closeBehavior} onValueChange={(closeBehavior) => setDraft({ ...draft, closeBehavior })} options={[{ value: "exit_app", label: "Exit MediaFlick" }, { value: "minimize_window", label: "Minimize window" }]} /></SettingsRow>
      <SettingsRow title="Show scrollbars" description="Reveal native scrollbars instead of the immersive hidden treatment."><Switch aria-label="Show scrollbars" checked={draft.showScrollbars} onCheckedChange={(showScrollbars) => setDraft({ ...draft, showScrollbars })} /></SettingsRow>
    </Section>
    <Section title="Diagnostics" description="A log-level change is picked up on the next application launch.">
      <SettingsRow title="Log level" description="Use Debug only while investigating a problem."><SelectField label="Log level" value={draft.logLevel} onValueChange={(logLevel) => setDraft({ ...draft, logLevel })} options={[{ value: "trace", label: "Trace" }, { value: "debug", label: "Debug" }, { value: "info", label: "Info" }, { value: "warn", label: "Warn" }, { value: "error", label: "Error" }]} /></SettingsRow>
    </Section>
    {status?.authenticated && <Section title="Local account data" description={`Remove this device's data for ${status.userName ?? "this account"} on ${status.serverUrl ?? "this server"}. Nothing is deleted from Jellyfin.`}>
      <SettingsRow title="Delete local account data" description="This removes account settings, playback choices, collection snapshots, and custom collection posters, then signs this device out.">
        <div className="flex w-full max-w-md flex-col gap-2">
          <Input value={deleteConfirmation} onChange={(event) => setDeleteConfirmation(event.target.value)} placeholder="Type DELETE to confirm" aria-label="Type DELETE to confirm local account deletion" />
          <Button variant="destructive" disabled={deleteConfirmation !== "DELETE" || deleteAccount.isPending} onClick={() => {
            if (window.confirm(`Delete local data for ${status.userName ?? "this account"} on ${status.serverUrl ?? "this server"}?`)) deleteAccount.mutate()
          }}><Trash2 />{deleteAccount.isPending ? "Deleting…" : "Delete local account data"}</Button>
        </div>
      </SettingsRow>
    </Section>}
    <SaveBar dirty={!same(draft, settings.client.application)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.client.application)} onReset={() => setDraft({ closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" })} restartMessage={draft.logLevel !== settings.client.application.logLevel ? "Log-level changes apply after restarting MediaFlick." : undefined} />
  </div>
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
 * attributes so the panel is themed by the unsaved choices like the shelf.
 * Only one preview exists at a time, so a single module-level host is enough;
 * mounting attaches it to the body and unmounting removes it again.
 */
const PANEL_HOST = document.createElement("div")

function AppearancePreview({ appearance }: { appearance: AppearanceSettings }) {
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
    PANEL_HOST.dataset.theme = appearance.theme
    PANEL_HOST.dataset.accent = appearance.accent
    PANEL_HOST.dataset.density = appearance.density
    PANEL_HOST.dataset.reducedMotion = String(reducedMotion)
    PANEL_HOST.style.setProperty("--artwork-intensity", String(appearance.artworkIntensity / 100))
    PANEL_HOST.style.setProperty("--backdrop-intensity", String(appearance.backdropIntensity / 100))
  }, [appearance.theme, appearance.accent, appearance.density, appearance.artworkIntensity, appearance.backdropIntensity, reducedMotion])
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
      data-theme={appearance.theme}
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
  const ratingsQuery = useRatingsStatus(Boolean(status?.authenticated))
  const { data: settings } = settingsQuery
  const { data: ratings } = ratingsQuery
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.appearance, collectionAccountKey(status))
  const mutation = useMutation({ mutationFn: (value: AppearanceSettings) => api.settingsPatch.appearance(value), onSuccess: (saved, submitted) => { acceptSaved(saved.appearance, submitted); saveSettings(saved) }, onError: (error: Error) => toast.error(error.message) })
  if (statusQuery.error && !status) return <SettingsError title="Appearance unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Appearance" />
  if (settingsQuery.error && !settings) return <SettingsError title="Appearance settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  return <div className="settings-page"><PageTitle title="Appearance" detail="Tune MediaFlick for this Jellyfin account without changing library or server settings." />
    <Section title="Live preview" description="Your own shelves with your unsaved choices applied here only; the rest of MediaFlick changes after Save.">
      <AppearancePreview appearance={draft} />
    </Section>
    <Section title="Theme" description="System follows the current operating-system color preference.">
      <SettingsRow title="Color mode" description="Choose the overall surface treatment."><SelectField label="Color mode" value={draft.theme} onValueChange={(theme) => setDraft({ ...draft, theme })} options={[{ value: "system", label: "System" }, { value: "dark", label: "Dark" }, { value: "light", label: "Light" }]} /></SettingsRow>
      <SettingsRow title="Accent" description="The signal color used for active controls and focus rings."><SelectField label="Accent" value={draft.accent} onValueChange={(accent) => setDraft({ ...draft, accent })} options={[{ value: "signal", label: "Signal" }, { value: "cobalt", label: "Cobalt" }, { value: "amber", label: "Amber" }, { value: "violet", label: "Violet" }]} /></SettingsRow>
      <SettingsRow title="Density" description="Compact reduces the spacing used by browsing and settings surfaces."><SelectField label="Density" value={draft.density} onValueChange={(density) => setDraft({ ...draft, density })} options={[{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }]} /></SettingsRow>
    </Section>
    <Section title="Cards" description="Choose how library cards behave and what they show.">
      <SettingsRow title="Card previews" description="Open a larger panel after resting the pointer on a card. When off, Play, My List, and watched buttons stay on the card.">
        <Switch aria-label="Show pop-out previews on cards" checked={draft.cardPreviews} onCheckedChange={(cardPreviews) => setDraft({ ...draft, cardPreviews })} />
      </SettingsRow>
      <SettingsRow title="Media info" description="Show video resolution, dynamic range, and audio format on library cards.">
        <Switch aria-label="Show media info on cards" checked={draft.showMediaInfo} onCheckedChange={(showMediaInfo) => setDraft({ ...draft, showMediaInfo })} />
      </SettingsRow>
      <div className="border-t border-border pt-5">
        <h3 className="font-medium">Rating sources</h3>
        <p className="mt-1 mb-4 text-sm text-muted-foreground">Choose any combination of MDBList sources for compact top-left card overlays.</p>
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
            onChange={(ratingSources) => setDraft({ ...draft, ratingSources })}
          />
        )}
      </div>
    </Section>
    <Section title="Artwork and motion" description="Lower artwork intensity for a quieter browsing surface.">
      <SettingsRow controlId="artwork-intensity" title="Artwork intensity" description={`${draft.artworkIntensity}%`}><Slider id="artwork-intensity" aria-label="Artwork intensity" aria-describedby="artwork-intensity-help" aria-valuetext={`${draft.artworkIntensity} percent`} className="w-52" value={[draft.artworkIntensity]} onValueChange={([artworkIntensity]) => setDraft({ ...draft, artworkIntensity })} /></SettingsRow>
      <SettingsRow controlId="backdrop-intensity" title="Backdrop intensity" description={`${draft.backdropIntensity}%`}><Slider id="backdrop-intensity" aria-label="Backdrop intensity" aria-describedby="backdrop-intensity-help" aria-valuetext={`${draft.backdropIntensity} percent`} className="w-52" value={[draft.backdropIntensity]} onValueChange={([backdropIntensity]) => setDraft({ ...draft, backdropIntensity })} /></SettingsRow>
      <SettingsRow title="Reduce motion" description="Disable decorative transitions and automatic movement."><Switch aria-label="Reduce motion" checked={draft.reducedMotion} onCheckedChange={(reducedMotion) => setDraft({ ...draft, reducedMotion })} /></SettingsRow>
    </Section>
    <SaveBar dirty={!same(draft, settings.appearance)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.appearance)} onReset={() => setDraft({ theme: "system", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, cardPreviews: true, showMediaInfo: true, ratingSources: [] })} />
  </div>
}

type HomeElement = HomeConfiguration["elements"][number]

type HomeDrag = {
  key: string
  pointerId: number
  x: number
  y: number
  offsetX: number
  offsetY: number
  width: number
  height: number
  dropIndex: number
}

const homeElementKey = (element: HomeElement) => `${element.kind}:${element.id}`

function dropHomeElement(configuration: HomeConfiguration, key: string, dropIndex: number) {
  const visible = configuration.elements.filter((element) => element.available)
  const from = visible.findIndex((element) => homeElementKey(element) === key)
  if (from < 0) return configuration
  const [dragged] = visible.splice(from, 1)
  visible.splice(Math.max(0, Math.min(dropIndex, visible.length)), 0, dragged)
  let visibleIndex = 0
  return {
    ...configuration,
    elements: configuration.elements.map((element) => element.available ? visible[visibleIndex++] : element),
  }
}

function HomeSettingsPage() {
  const status = useStatus()
  const query = useHomeSettings(Boolean(status.data?.authenticated))
  const [draft, setDraft, updateDraft, acceptSaved] = useSourceDraft(query.data?.settings, collectionAccountKey(status.data))
  const [dragging, setDragging] = useState<HomeDrag | null>(null)
  const dragRef = useRef<HomeDrag | null>(null)
  const visible = useMemo(() => draft?.elements.filter((element) => element.available) ?? [], [draft])
  useEffect(() => {
    if (!dragging?.key) return
    const movePointer = (event: PointerEvent) => {
      const current = dragRef.current
      if (!current || event.pointerId !== current.pointerId) return
      const remainingKeys = visible
        .map(homeElementKey)
        .filter((key) => key !== current.key)
      let dropIndex = remainingKeys.length
      for (const row of document.querySelectorAll<HTMLElement>("[data-home-element-key]")) {
        const index = remainingKeys.indexOf(row.dataset.homeElementKey ?? "")
        if (index < 0) continue
        const bounds = row.getBoundingClientRect()
        if (event.clientY < bounds.top + bounds.height / 2) {
          dropIndex = index
          break
        }
      }
      const next = { ...current, x: event.clientX, y: event.clientY, dropIndex }
      dragRef.current = next
      setDragging(next)
    }
    const dropPointer = (event: PointerEvent) => {
      const current = dragRef.current
      if (!current || event.pointerId !== current.pointerId) return
      updateDraft((configuration) => configuration ? dropHomeElement(configuration, current.key, current.dropIndex) : configuration)
      dragRef.current = null
      setDragging(null)
    }
    const cancelDrag = () => {
      dragRef.current = null
      setDragging(null)
    }
    const cancelWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") cancelDrag()
    }
    window.addEventListener("pointermove", movePointer)
    window.addEventListener("pointerup", dropPointer)
    window.addEventListener("pointercancel", cancelDrag)
    window.addEventListener("keydown", cancelWithEscape)
    window.addEventListener("blur", cancelDrag)
    return () => {
      window.removeEventListener("pointermove", movePointer)
      window.removeEventListener("pointerup", dropPointer)
      window.removeEventListener("pointercancel", cancelDrag)
      window.removeEventListener("keydown", cancelWithEscape)
      window.removeEventListener("blur", cancelDrag)
    }
  }, [dragging?.key, updateDraft, visible])
  const mutation = useMutation({
    mutationFn: (value: HomeConfiguration) => api.saveHomeSettings(homeSettingsWrite(value)),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.settings, submitted)
      queryClient.setQueryData(queryKeys.homeSettings, saved)
      void queryClient.invalidateQueries({ queryKey: queryKeys.home })
      void queryClient.invalidateQueries({ queryKey: queryKeys.homeResume })
      queryClient.removeQueries({ queryKey: queryKeys.billboard })
      toast.success("Home settings saved")
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (status.isPending) return <SettingsLoading />
  if (!status.data?.authenticated) return <SignInRequired name="Home" />
  if (query.error && !query.data) return <SettingsError title="Home settings unavailable" error={query.error} onRetry={() => void query.refetch()} />
  if (!query.data || !draft) return <SettingsLoading />

  const move = (fromKey: string, toKey: string) => {
    if (fromKey === toKey) return
    updateDraft((current) => {
      if (!current) return current
      const from = current.elements.findIndex((element) => homeElementKey(element) === fromKey)
      const to = current.elements.findIndex((element) => homeElementKey(element) === toKey)
      if (from < 0 || to < 0) return current
      const elements = [...current.elements]
      const [element] = elements.splice(from, 1)
      elements.splice(to, 0, element)
      return { ...current, elements }
    })
  }
  const moveVisible = (index: number, offset: number) => {
    const target = visible[index + offset]
    if (target) move(homeElementKey(visible[index]), homeElementKey(target))
  }
  const setElementEnabled = (key: string, enabled: boolean) => updateDraft((current) => current ? ({
    ...current,
    elements: current.elements.map((element) => homeElementKey(element) === key ? { ...element, enabled } : element),
  }) : current)
  const draggedElement = dragging ? visible.find((element) => homeElementKey(element) === dragging.key) : null
  const remaining = dragging ? visible.filter((element) => homeElementKey(element) !== dragging.key) : visible
  const slots = remaining.length + (dragging ? 1 : 0)

  return <div className="settings-page">
    <PageTitle title="Home" detail="Choose and order the shelves shown for this Jellyfin account." />
    <Section title="Billboard" description="The billboard stays fixed above every shelf.">
      <SettingsRow title="Show billboard" description="Rotate a small selection of titles with landscape artwork.">
        <Checkbox checked={draft.billboard} onCheckedChange={(checked) => setDraft({ ...draft, billboard: checked === true })} aria-label="Show billboard" />
      </SettingsRow>
    </Section>
    <Section title="Shelves" description="Disabled shelves keep their positions. Drag a handle or use the arrow buttons to reorder.">
      <div className="space-y-2">
        {Array.from({ length: slots }, (_, slot) => {
          if (dragging && slot === dragging.dropIndex) return <div
            key="home-drop-placeholder"
            data-testid="home-drop-placeholder"
            aria-hidden
            className="rounded-lg border-2 border-dashed border-primary/60 bg-primary/10 shadow-inner"
            style={{ height: dragging.height }}
          />
          const index = dragging && slot > dragging.dropIndex ? slot - 1 : slot
          const element = remaining[index]
          if (!element) return null
          const key = homeElementKey(element)
          const visibleIndex = visible.findIndex((candidate) => homeElementKey(candidate) === key)
          const watching = element.kind === "builtIn" && element.id === "watching"
          return <div
            key={key}
            data-home-element-key={key}
            className="rounded-lg border bg-card p-3"
          >
            <div className="flex items-center gap-3">
              <button
                type="button"
                aria-label={`Drag ${element.label}`}
                className="shrink-0 touch-none select-none cursor-grab text-muted-foreground active:cursor-grabbing"
                onPointerDown={(event) => {
                  if (event.button !== 0) return
                  const row = event.currentTarget.closest<HTMLElement>("[data-home-element-key]")
                  if (!row) return
                  event.preventDefault()
                  const bounds = row.getBoundingClientRect()
                  const next = {
                    key,
                    pointerId: event.pointerId,
                    x: event.clientX,
                    y: event.clientY,
                    offsetX: event.clientX - bounds.left,
                    offsetY: event.clientY - bounds.top,
                    width: bounds.width,
                    height: bounds.height,
                    dropIndex: visibleIndex,
                  }
                  dragRef.current = next
                  setDragging(next)
                }}
              ><GripVertical className="size-4" aria-hidden /></button>
              <Checkbox checked={element.enabled} onCheckedChange={(checked) => setElementEnabled(key, checked === true)} aria-label={`Show ${element.label}`} />
              <div className="min-w-0 flex-1"><div className="truncate font-medium">{element.label}</div><div className="text-xs text-muted-foreground">{element.category}</div></div>
              <Button type="button" size="icon-sm" variant="ghost" disabled={visibleIndex === 0} aria-label={`Move ${element.label} up`} onClick={() => moveVisible(visibleIndex, -1)}><ArrowUp /></Button>
              <Button type="button" size="icon-sm" variant="ghost" disabled={visibleIndex === visible.length - 1} aria-label={`Move ${element.label} down`} onClick={() => moveVisible(visibleIndex, 1)}><ArrowDown /></Button>
            </div>
            {watching && <div className="mt-3 ml-7 grid gap-3 border-t pt-3 sm:grid-cols-3">
              <label className="flex items-center gap-2 text-sm"><Checkbox checked={draft.watching.continueWatching} onCheckedChange={(checked) => setDraft({ ...draft, watching: { ...draft.watching, continueWatching: checked === true } })} />Continue Watching</label>
              <label className="flex items-center gap-2 text-sm"><Checkbox checked={draft.watching.nextUp} onCheckedChange={(checked) => setDraft({ ...draft, watching: { ...draft.watching, nextUp: checked === true } })} />Next Up</label>
              <label className="flex items-center gap-2 text-sm"><Checkbox checked={draft.watching.combine} onCheckedChange={(checked) => setDraft({ ...draft, watching: { ...draft.watching, combine: checked === true } })} />Combine shelves</label>
            </div>}
          </div>
        })}
      </div>
      {query.data.collectionMode === "jellyfin" && <p className="text-xs text-muted-foreground">My Collection shelves are hidden while Jellyfin collection mode is active.</p>}
    </Section>
    <SaveBar dirty={!same(draft, query.data.settings)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(query.data.settings)} onReset={() => setDraft(query.data.defaults)} />
    {dragging && draggedElement && createPortal(<div
      aria-hidden
      data-testid="home-drag-preview"
      className="pointer-events-none fixed z-[100] rotate-[0.35deg] scale-[1.015] rounded-lg border border-primary/50 bg-card/95 p-3 opacity-95 shadow-2xl ring-1 ring-primary/30"
      style={{
        left: dragging.x - dragging.offsetX,
        top: dragging.y - dragging.offsetY,
        width: dragging.width,
        minHeight: dragging.height,
      }}
    >
      <div className="flex items-center gap-3">
        <GripVertical className="size-4 shrink-0 text-primary" />
        <Checkbox checked={draggedElement.enabled} disabled tabIndex={-1} />
        <div className="min-w-0 flex-1"><div className="truncate font-medium">{draggedElement.label}</div><div className="text-xs text-muted-foreground">{draggedElement.category}</div></div>
      </div>
      {draggedElement.kind === "builtIn" && draggedElement.id === "watching" && <div className="mt-3 ml-7 border-t pt-3 text-sm text-muted-foreground">Continue Watching · Next Up · Combine shelves</div>}
    </div>, document.body)}
  </div>
}

function SignInRequired({ name }: { name: string }) {
  return <div className="settings-page"><PageTitle title={name} detail="This configuration belongs to the signed-in Jellyfin account." /><Section title="Sign in required" description={`Sign in to your Jellyfin server to view or configure ${name}.`}><Button asChild><RouterLink to="/">Go to sign in</RouterLink></Button></Section></div>
}

function Letterboxd() {
  const cache = useQueryClient()
  const statusQuery = useStatus()
  const { data: status } = statusQuery
  const profiles = useQuery({ queryKey: ["letterboxd", "profiles"], queryFn: api.letterboxd.profiles, enabled: Boolean(status?.authenticated), retry: false })
  const [entry, setEntry] = useState("")
  const [additions, setAdditions] = useState<string[]>([])
  const [removals, setRemovals] = useState<string[]>([])
  const account = collectionAccountKey(status)
  const savedEnabled = useMemo<Record<string, boolean> | null>(() => profiles.data ? Object.fromEntries(
    profiles.data.profiles.map((profile) => [profile.id, profile.enabled]),
  ) : null, [profiles.data])
  const [enabledDraft, setEnabledDraft] = useSourceDraft(savedEnabled, account)
  const isCurrentAccount = () => collectionAccountKey(cache.getQueryData<Status>(queryKeys.status)) === account
  const checkAccount = () => {
    if (!isCurrentAccount()) throw new Error("The signed-in account changed. Remaining profile changes were not saved.")
  }
  // Profile writes also change every movie's public-review projection.
  const refresh = () => { if (isCurrentAccount()) void cache.invalidateQueries({ queryKey: ["letterboxd"] }) }
  const rememberProfile = (profile: LetterboxdProfile) => {
    checkAccount()
    cache.setQueryData<Awaited<ReturnType<typeof api.letterboxd.profiles>>>(["letterboxd", "profiles"], (current) => {
      const existing = current?.profiles ?? []
      return { profiles: existing.some((candidate) => candidate.id === profile.id)
        ? existing.map((candidate) => candidate.id === profile.id ? profile : candidate)
        : [...existing, profile] }
    })
  }
  const save = useMutation({
    mutationFn: async (submitted: { enabled: Record<string, boolean>; additions: string[]; removals: string[] }) => {
      // Acknowledge each completed write so a later failure leaves only unfinished work to retry.
      for (const profile of profiles.data?.profiles ?? []) {
        checkAccount()
        const enabled = submitted.enabled[profile.id] ?? profile.enabled
        if (enabled !== profile.enabled && !submitted.removals.includes(profile.id)) {
          const saved = await api.letterboxd.setEnabled(profile.id, enabled)
          rememberProfile(saved.profile)
        }
      }
      for (const id of submitted.removals) {
        checkAccount()
        await api.letterboxd.remove(id)
        checkAccount()
        cache.setQueryData<Awaited<ReturnType<typeof api.letterboxd.profiles>>>(["letterboxd", "profiles"], (current) => ({
          profiles: (current?.profiles ?? []).filter((profile) => profile.id !== id),
        }))
        setRemovals((current) => current.filter((removed) => removed !== id))
      }
      for (const input of submitted.additions) {
        checkAccount()
        const saved = await api.letterboxd.add(input)
        rememberProfile(saved.profile)
        setAdditions((current) => current.filter((added) => added !== input))
      }
    },
    onSuccess: () => toast.success("Letterboxd settings saved"),
    onError: (error: Error) => toast.error(error.message),
    onSettled: refresh,
  })
  const verify = useMutation({ mutationFn: api.letterboxd.refresh, onSuccess: refresh, onError: (error: Error) => toast.error(error.message) })
  const open = useMutation({ mutationFn: api.letterboxd.open, onError: (error: Error) => toast.error(error.message) })
  const queuedAdditions = () => [...new Set([...additions, ...(entry.trim() ? [entry.trim()] : [])])]
  const clearPending = () => {
    setEntry("")
    setAdditions([])
    setRemovals([])
    save.reset()
  }
  if (statusQuery.error && !status) return <SettingsError title="Letterboxd unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Letterboxd" />
  const dirty = Boolean(entry.trim() || additions.length || removals.length || enabledDraft && savedEnabled && !same(enabledDraft, savedEnabled))
  return <div className="settings-page"><PageTitle title="Letterboxd" detail="Connect public profiles using Letterboxd's public RSS feed. No credentials are stored." />
    <fieldset disabled={save.isPending} className="min-w-0 space-y-5">
      <legend className="sr-only">Letterboxd profiles</legend>
      <Section title="Add profile" description="Profiles are verified and connected when you save.">
        <form className="max-w-xl space-y-2" onSubmit={(event) => {
          event.preventDefault()
          setAdditions(queuedAdditions())
          setEntry("")
        }}>
          <Label htmlFor="letterboxd-profile">Letterboxd username or profile URL</Label>
          <p id="letterboxd-profile-help" className="text-sm text-muted-foreground">Enter a username or a profile URL such as https://letterboxd.com/username/.</p>
          <div className="flex gap-2"><Input id="letterboxd-profile" aria-describedby="letterboxd-profile-help" value={entry} onChange={(event) => setEntry(event.target.value)} placeholder="Username or profile URL" /><Button disabled={!entry.trim() || !profiles.data}>Add profile</Button></div>
        </form>
        {additions.map((input) => <div key={input} className="flex items-center justify-between gap-3 rounded-md border p-3">
          <div className="min-w-0"><p className="break-all">{input}</p><p className="text-sm text-muted-foreground">Will be added when you save</p></div>
          <Button variant="ghost" onClick={() => setAdditions((current) => current.filter((added) => added !== input))} aria-label={`Cancel adding ${input}`}>Cancel</Button>
        </div>)}
      </Section>
      <Section title="Connected profiles" description="Changes apply after Save. Discard restores the saved profiles.">
        {profiles.isPending ? <p className="text-sm text-muted-foreground">Loading profiles…</p> : profiles.error && !profiles.data ? <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-sm"><span>{profiles.error.message}</span><Button size="sm" variant="outline" onClick={() => void profiles.refetch()}>Try again</Button></div> : profiles.data?.profiles.length ? <div className="space-y-3">{profiles.data.profiles.map((profile) => removals.includes(profile.id)
          ? <div key={profile.id} className="flex items-center justify-between gap-3 rounded-md border p-3"><div><p>{profile.displayName}</p><p className="text-sm text-muted-foreground">Will be removed when you save</p></div><Button variant="outline" aria-label={`Undo removal of ${profile.displayName}`} onClick={() => setRemovals((current) => current.filter((id) => id !== profile.id))}>Undo</Button></div>
          : <ProfileCard key={profile.id} profile={{ ...profile, enabled: enabledDraft?.[profile.id] ?? profile.enabled }} onEnabled={(enabled) => { if (enabledDraft) setEnabledDraft({ ...enabledDraft, [profile.id]: enabled }) }} onRefresh={() => verify.mutate(profile.id)} onOpen={() => open.mutate(profile.id)} onRemove={() => setRemovals((current) => [...current, profile.id])} />)}</div> : <p className="text-sm text-muted-foreground">No Letterboxd profiles connected yet.</p>}
      </Section>
    </fieldset>
    {save.error && <p role="alert" className="text-sm text-destructive">Could not save all changes. Your remaining edits are kept. {save.error.message}</p>}
    <SaveBar
      dirty={dirty}
      saving={save.isPending}
      saveDisabled={!profiles.data}
      onSave={() => {
        if (!enabledDraft) return
        const pendingAdditions = queuedAdditions()
        setAdditions(pendingAdditions)
        setEntry("")
        save.mutate({ enabled: enabledDraft, additions: pendingAdditions, removals })
      }}
      onDiscard={() => { clearPending(); setEnabledDraft(savedEnabled) }}
      onReset={() => {
        clearPending()
        if (savedEnabled) setEnabledDraft(Object.fromEntries(Object.keys(savedEnabled).map((id) => [id, true])))
      }}
    />
  </div>
}

function ProfileCard({ profile, onEnabled, onRefresh, onOpen, onRemove }: { profile: LetterboxdProfile; onEnabled: (enabled: boolean) => void; onRefresh: () => void; onOpen: () => void; onRemove: () => void }) {
  return <div className="settings-profile-card"><div className="min-w-0"><div className="flex items-center gap-2"><h3 className="font-medium">{profile.displayName}</h3><span className="settings-status" data-status={profile.verificationStatus}>{profile.verificationStatus === "verified" ? <CheckCircle2 /> : <AlertTriangle />}{profile.verificationStatus}</span></div><p className="mt-1 truncate text-sm text-muted-foreground">{profile.canonicalUrl}</p></div><div className="flex flex-wrap items-center justify-end gap-1"><Switch aria-label={`Enable ${profile.displayName}`} checked={profile.enabled} onCheckedChange={onEnabled} /><Button size="icon-sm" variant="ghost" aria-label="Refresh profile" onClick={onRefresh}><RefreshCw /></Button><Button size="icon-sm" variant="ghost" aria-label="Open profile" onClick={onOpen}><ExternalLink /></Button><Button size="icon-sm" variant="ghost" aria-label="Remove profile" onClick={onRemove}><Trash2 /></Button></div></div>
}

const COMPANION_SERVICES: ReadonlyArray<{
  id: CompanionService
  name: string
  description: string
}> = [
  { id: "seerr", name: "Seerr", description: "Discovery and requests for mapped Jellyfin users." },
  { id: "sonarr", name: "Sonarr", description: "Upcoming episodes and download status." },
  { id: "radarr", name: "Radarr", description: "Upcoming films and download status." },
  { id: "mdblist", name: "MDBList", description: "Shared ratings and public list sources." },
  { id: "tmdb", name: "TMDB", description: "Movie franchises and collection metadata." },
]

const COMPANION_FEATURES: ReadonlyArray<{
  capability: string
  requires: string
  name: string
  description: string
}> = [
  {
    capability: "franchise-memberships-v1",
    requires: "collection-experience-v1",
    name: "Movie franchises",
    description: "The server plugin cannot supply the franchise membership data this Desktop build needs.",
  },
  {
    capability: "seerr-person-discovery",
    requires: "seerr",
    name: "Cast discovery",
    description: "Cast pages cannot load a person's Seerr credits with this server plugin.",
  },
  {
    capability: "seerr-discovery-v4",
    requires: "seerr",
    name: "Release-decade filters",
    description: "Discover cannot send release-decade filters to this server plugin.",
  },
  {
    capability: "seerr-request-profiles",
    requires: "seerr",
    name: "Request profile selection",
    description: "Requests cannot select a Sonarr or Radarr quality profile with this server plugin.",
  },
]

function Availability({ available }: { available: boolean }) {
  return <span className="settings-status" data-status={available ? "verified" : "unverified"}>{available ? <CheckCircle2 /> : <AlertTriangle />}{available ? "available" : "unavailable"}</span>
}

function FeatureCompatibility({ compatible }: { compatible: boolean }) {
  return <span className="settings-status" data-status={compatible ? "verified" : "unverified"}>{compatible ? <CheckCircle2 /> : <AlertTriangle />}{compatible ? "compatible" : "missing"}</span>
}

export function CompanionIntegration() {
  const statusQuery = useStatus()
  const companionQuery = useCompanion()
  const { data: status } = statusQuery
  const { data: companion } = companionQuery
  const companionSeerr = Boolean(companion?.compatible && companion.info?.services.seerr)
  const seerrQuery = useSeerrStatus(companionSeerr)
  if (statusQuery.error && !status) return <SettingsError title="Companion status unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="MediaFlick Companion" />
  if (companionQuery.error && !companion) return <SettingsError title="Companion status unavailable" error={companionQuery.error} onRetry={() => void companionQuery.refetch()} />
  if (!companion) return <SettingsLoading />

  const pluginDescription = companion.available
    ? "Companion answered this Desktop build's authenticated status check."
    : companion.error ?? "MediaFlick Companion was not found on this Jellyfin server."
  const pluginAvailable = companion.available && companion.compatible
  const capabilities = companion.info?.capabilities ?? []
  const missingFeatures = companion.available && companion.compatible
    ? COMPANION_FEATURES.filter((feature) => capabilities.includes(feature.requires) && !capabilities.includes(feature.capability))
    : []

  return <div className="settings-page"><PageTitle title="MediaFlick Companion" detail="Server-managed integrations available to this Jellyfin account." />
    <Section title="Plugin" description="Administrators configure these services in Jellyfin's MediaFlick Companion dashboard.">
      <SettingsRow title="Connection" description={pluginDescription}><Availability available={companion.available} /></SettingsRow>
    </Section>
    {companion.available && <Section title="Feature compatibility" description="Desktop checks the features advertised by Companion. Plugin version numbers are not used for this check.">
      {!companion.compatible && <SettingsRow title="Companion protocol" description="This Desktop build cannot read the plugin's status contract."><FeatureCompatibility compatible={false} /></SettingsRow>}
      {missingFeatures.map((feature) => <SettingsRow key={feature.capability} title={feature.name} description={feature.description}><FeatureCompatibility compatible={false} /></SettingsRow>)}
      {companion.compatible && missingFeatures.length === 0 && <SettingsRow title="Desktop features" description="This Companion provides every server feature used by this Desktop build."><FeatureCompatibility compatible /></SettingsRow>}
    </Section>}
    <Section title="Services" description="Desktop reads these connections through Companion and never receives their addresses or credentials.">
      {COMPANION_SERVICES.map((service) => {
        const available = Boolean(pluginAvailable && companion.info?.services[service.id])
        let description = service.description
        if (service.id === "seerr" && available) {
          if (seerrQuery.data?.mapped) {
            description += ` This account is mapped${seerrQuery.data.user?.name ? ` as ${seerrQuery.data.user.name}` : ""}.`
          } else if (seerrQuery.error instanceof ApiError && seerrQuery.error.status === 409) {
            description += " This Jellyfin account has not been imported into Seerr."
          } else if (seerrQuery.error) {
            description += " The account mapping could not be checked."
          } else {
            description += " Checking this account's mapping."
          }
        }
        return <SettingsRow key={service.id} title={service.name} description={description}><Availability available={available} /></SettingsRow>
      })}
    </Section>
  </div>
}

function SettingsNavigation() {
  const location = useLocation()
  const { data: status } = useStatus()
  return <nav className="settings-navigation" aria-label="Settings navigation"><span className="settings-nav-label">Settings</span>{NAVIGATION.map((item, index) => { const active = location.pathname.startsWith(item.to); const previousGroup = NAVIGATION[index - 1]?.group; return <Fragment key={item.to}>{item.group && item.group !== previousGroup && <span className="settings-nav-group">{item.group}</span>}<RouterLink to={item.to} data-active={active} aria-disabled={item.signedIn && !status?.authenticated}><item.icon /><span>{item.title}</span></RouterLink></Fragment> })}</nav>
}

export function AppearanceSync() {
  const { data: settings } = useSettings()
  useEffect(() => {
    const appearance = settings?.appearance
    if (!appearance) return
    const root = document.documentElement
    root.dataset.theme = appearance.theme
    root.dataset.accent = appearance.accent
    root.dataset.density = appearance.density
    root.dataset.reducedMotion = String(appearance.reducedMotion)
    root.dataset.cardPreviews = String(appearance.cardPreviews)
    root.dataset.mediaInfo = String(appearance.showMediaInfo)
    root.style.setProperty("--artwork-intensity", String(appearance.artworkIntensity / 100))
    root.style.setProperty("--backdrop-intensity", String(appearance.backdropIntensity / 100))
  }, [settings?.appearance])
  return null
}

export default function Settings() {
  const { data: status } = useStatus()
  return <SettingsDraftGuard key={collectionAccountKey(status)}><div className="settings-layout"><SettingsNavigation /><main className="settings-main"><Routes><Route index element={<Navigate to="/settings/client/player" replace />} /><Route path="client/player" element={<PlayerSettings />} /><Route path="client/playback" element={<PlaybackSettings />} /><Route path="client/application" element={<ApplicationSettings />} /><Route path="home" element={<HomeSettingsPage />} /><Route path="appearance" element={<Appearance />} /><Route path="collections" element={<CollectionSettingsPage />} /><Route path="integrations/companion" element={<CompanionIntegration />} /><Route path="integrations/letterboxd" element={<Letterboxd />} /><Route path="*" element={<Navigate to="/settings" replace />} /></Routes></main></div></SettingsDraftGuard>
}
