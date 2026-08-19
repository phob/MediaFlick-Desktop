import { useMutation, useQuery } from "@tanstack/react-query"
import {
  AlertTriangle,
  AudioLines,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  FolderOpen,
  KeyRound,
  Link,
  Monitor,
  Palette,
  Play,
  Plus,
  RefreshCw,
  Save,
  ScanLine,
  SlidersHorizontal,
  Trash2,
  ThumbsUp,
  Undo2,
  type LucideIcon,
} from "lucide-react"
import {
  Fragment,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { Link as RouterLink, Navigate, Route, Routes, useLocation } from "react-router-dom"
import { toast } from "sonner"
import { RatingSourceIcon } from "@/components/RatingSourceIcon"
import { SeerrSetupDialog } from "@/components/seerr/SeerrSetupDialog"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  api,
  playerSettingsWrite,
  type AppearanceSettings,
  type ClientSettings,
  type LetterboxdProfile,
  type RatingCredentialStatus,
  type RatingSourceDefinition,
  type RatingsIntegrationStatus,
} from "@/lib/api"
import { jsonNumber, jsonString } from "@/lib/json"
import { queryClient, queryKeys } from "@/lib/query-client"
import { useRatingsStatus, useSeerrStatus, useSettings, useStatus } from "@/lib/queries"
import { usePrefersReducedMotion } from "@/lib/reduced-motion"
import { readShellEvent, type ShellEvent } from "@/lib/shell-events"
import { cssVariables } from "@/lib/style"

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
  { to: "/settings/appearance", title: "Appearance", icon: Palette },
  { to: "/settings/integrations/ratings", title: "MDBList Ratings", icon: KeyRound, group: "Integrations" },
  { to: "/settings/integrations/letterboxd", title: "Letterboxd", icon: Link, signedIn: true, group: "Integrations" },
  { to: "/settings/integrations/seerr", title: "Seerr", icon: Link, signedIn: true, group: "Integrations" },
]

function same<T>(left: T, right: T) {
  return JSON.stringify(left) === JSON.stringify(right)
}

function useDraft<T>(value: T | undefined) {
  const [draft, setDraft] = useState<T | undefined>(value)
  useEffect(() => setDraft(value), [value])
  return [draft, setDraft] as const
}

function saveSettings(saved: ClientSettings, message = "Settings saved") {
  queryClient.setQueryData(queryKeys.settings, saved)
  toast.success(message)
}

function SettingsRow({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <div className="settings-row">
      <div className="min-w-0">
        <h3 className="font-medium">{title}</h3>
        <p className="mt-1 text-sm text-muted-foreground">{description}</p>
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
          : "Add an MDBList API key under Integrations to choose sources."}
      </p>
    </fieldset>
  )
}

function credentialStatusText(status: RatingCredentialStatus) {
  const quota = status.quota
  const quotaText = quota.limit != null && quota.remaining != null
    ? ` · ${quota.remaining.toLocaleString()} of ${quota.limit.toLocaleString()} requests remaining`
    : ""
  const retry = status.retryAt && status.retryAt * 1000 > Date.now()
    ? ` · retry after ${new Date(status.retryAt * 1000).toLocaleString()}`
    : ""
  return `${status.detail ?? (status.configured ? "Credential saved securely." : "No credential saved.")}${quotaText}${retry}`
}

export function SecureCredentialField({
  provider,
  label,
  status,
  onStatus,
}: {
  provider: "mdblist" | "tmdb"
  label: string
  status: RatingCredentialStatus
  onStatus: (status: RatingsIntegrationStatus) => void
}) {
  const [key, setKey] = useState("")
  const [revealed, setRevealed] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const configuredPlaceholder = status.configured ? "••••••••••••••••" : `Enter ${label}`
  const act = async (name: string, work: () => Promise<void>) => {
    setBusy(name)
    try {
      await work()
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Credential operation failed")
    } finally {
      setBusy(null)
    }
  }
  const reveal = () => void act("reveal", async () => {
    if (!key) setKey((await api.ratings.revealCredential(provider)).key)
    setRevealed((current) => !current || !key)
  })
  const copy = () => void act("copy", async () => {
    const value = key || (await api.ratings.revealCredential(provider)).key
    if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable")
    await navigator.clipboard.writeText(value)
    toast.success(`${label} copied`)
  })
  return (
    <div className="secure-credential-field">
      <form
        className="flex min-w-0 flex-1 gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (!key.trim()) return
          void act("save", async () => {
            const saved = await api.ratings.saveCredential(provider, key)
            onStatus(saved)
            if (saved.local[provider].configured) {
              setKey("")
              setRevealed(false)
            }
          })
        }}
      >
        <Input
          aria-label={label}
          type={revealed ? "text" : "password"}
          value={key}
          onChange={(event) => setKey(event.target.value)}
          placeholder={configuredPlaceholder}
          autoComplete="off"
          spellCheck={false}
        />
        <Button type="submit" variant={status.configured ? "outline" : "default"} disabled={!key.trim() || busy !== null}>
          {busy === "save" ? "Checking…" : status.configured ? "Replace" : "Save"}
        </Button>
      </form>
      {status.configured && (
        <div className="flex shrink-0 gap-1">
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`${revealed ? "Hide" : "Reveal"} ${label}`} aria-pressed={revealed} onClick={reveal} disabled={busy !== null}>
            {revealed ? <EyeOff /> : <Eye />}
          </Button>
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`Copy ${label}`} onClick={copy} disabled={busy !== null}><Copy /></Button>
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`Remove ${label}`} onClick={() => void act("remove", async () => {
            onStatus(await api.ratings.removeCredential(provider))
            setKey("")
            setRevealed(false)
          })} disabled={busy !== null}><Trash2 /></Button>
        </div>
      )}
      {status.configured && provider === "mdblist" && (
        <Button type="button" variant="ghost" size="sm" disabled={busy !== null} onClick={() => void act("validate", async () => onStatus(await api.ratings.validateCredential(provider)))}>
          {busy === "validate" ? "Checking…" : "Validate"}
        </Button>
      )}
      <p className="secure-credential-status" data-status={status.validation} aria-live="polite">
        {credentialStatusText(status)}
      </p>
    </div>
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

function SaveBar({ dirty, saving, onSave, onDiscard, onReset, restartRequired = false }: {
  dirty: boolean
  saving: boolean
  onSave: () => void
  onDiscard: () => void
  onReset: () => void
  restartRequired?: boolean
}) {
  if (!dirty) return null
  return (
    <div className="settings-save-bar">
      <div className="flex min-w-0 items-center gap-2 text-sm">
        {restartRequired && <AlertTriangle className="size-4 shrink-0 text-primary" />}
        <span>{restartRequired ? "Log-level changes apply after restart." : "You have unsaved changes."}</span>
      </div>
      <div className="flex shrink-0 gap-2">
        <Button variant="ghost" size="sm" onClick={onReset} disabled={saving}><Undo2 /> Reset</Button>
        <Button variant="outline" size="sm" onClick={onDiscard} disabled={saving}>Discard</Button>
        <Button size="sm" onClick={onSave} disabled={saving}><Save /> {saving ? "Saving…" : "Save"}</Button>
      </div>
    </div>
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
  const [draft, setDraft] = useDraft(settings?.client.player)
  const [install, setInstall] = useState<{ state: string; message?: string; downloaded?: number; total?: number | null }>({ state: "idle" })
  const pendingPickers = useRef<Partial<Record<"mpv" | "mpchc", string>>>({})
  const pendingInstall = useRef<string | null>(null)
  const [picking, setPicking] = useState<Partial<Record<"mpv" | "mpchc", boolean>>>({})
  const mutation = useMutation({
    mutationFn: api.settingsPatch.player,
    onSuccess: (saved) => saveSettings(saved),
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
      if (target === "mpv") setDraft((current) => current ? { ...current, mpvPath: path } : current)
      if (target === "mpchc") setDraft((current) => current ? { ...current, mpchcPath: path } : current)
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
        setDraft((current) => current ? { ...current, mpvPath: installedPath } : current)
        void queryClient.invalidateQueries({ queryKey: queryKeys.settings })
      }
      if (state === "failed") {
        pendingInstall.current = null
        if (message) toast.error(message)
      }
    }
  }, [setDraft])
  useShellEvents(onShellEvent)
  if (settingsQuery.error && !settings) return <SettingsError title="Player settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  const dirty = !same(draft, settings.client.player)
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
      <PageTitle title="Player" detail="Choose the native player MediaFlick launches for playback." />
      <Section title="Playback backend" description="MPC-HC is available only on Windows; mpv is the portable default.">
        <SettingsRow title="Player" description="The active backend determines which executable is required.">
          <SelectField label="Player backend" value={draft.playerBackend} onValueChange={(playerBackend) => setDraft({ ...draft, playerBackend })} options={[{ value: "mpv", label: "mpv" }, { value: "mpchc", label: "MPC-HC", disabled: !settings.capabilities.mpchc }]} />
        </SettingsRow>
        <SettingsRow title="Start fullscreen" description="Use a full-screen player window by default.">
          <SelectField label="Default fullscreen" value={draft.defaultFullscreen} onValueChange={(defaultFullscreen) => setDraft({ ...draft, defaultFullscreen })} options={[{ value: "fullscreen", label: "Fullscreen" }, { value: "windowed", label: "Windowed" }]} />
        </SettingsRow>
        <SettingsRow title="Mark watched key" description="The mpv key that marks the current title watched and plays the next item. Leave blank to disable it.">
          <Input className="w-52" value={draft.markWatchedNext ?? ""} onChange={(event) => setDraft({ ...draft, markWatchedNext: event.target.value || null })} placeholder="w" />
        </SettingsRow>
      </Section>
      <Section title="Executables" description="Paths are saved locally and are never sent to your Jellyfin server.">
        <SettingsRow title="mpv executable" description="Select mpv.exe or use the installer on supported Windows builds.">
          <div className="flex w-full max-w-md gap-2"><Input value={draft.mpvPath ?? ""} onChange={(event) => setDraft({ ...draft, mpvPath: event.target.value || null })} placeholder="Path to mpv" /><Button variant="outline" size="icon" aria-label="Choose mpv executable" aria-busy={picking.mpv} disabled={picking.mpv} onClick={() => pick("mpv")}><FolderOpen /></Button></div>
        </SettingsRow>
        {settings.capabilities.mpvInstaller && <SettingsRow title="Install mpv" description={installDetail ?? "Download and install the supported mpv build beside MediaFlick."}>
          <div className="flex gap-2"><Button variant="outline" onClick={installMpv} disabled={["queued", "downloading", "extracting"].includes(install.state)}><Download /> {install.state === "idle" || install.state === "failed" ? "Install mpv" : "Installing…"}</Button><Button variant="ghost" onClick={() => void api.shell.mpvHelp().catch((error: Error) => toast.error(error.message))}>Installation help</Button></div>
        </SettingsRow>}
        {!settings.capabilities.mpvInstaller && <SettingsRow title="Install mpv" description="See mpv’s installation guide for your operating system."><Button variant="ghost" onClick={() => void api.shell.mpvHelp().catch((error: Error) => toast.error(error.message))}>Installation help</Button></SettingsRow>}
        {settings.capabilities.mpchc && <SettingsRow title="MPC-HC executable" description="Only required when MPC-HC is the selected backend.">
          <div className="flex w-full max-w-md gap-2"><Input value={draft.mpchcPath ?? ""} onChange={(event) => setDraft({ ...draft, mpchcPath: event.target.value || null })} placeholder="Path to MPC-HC" /><Button variant="outline" size="icon" aria-label="Choose MPC-HC executable" aria-busy={picking.mpchc} disabled={picking.mpchc} onClick={() => pick("mpchc")}><FolderOpen /></Button></div>
        </SettingsRow>}
      </Section>
      <SaveBar dirty={dirty} saving={mutation.isPending} onSave={() => mutation.mutate(playerSettingsWrite(draft))} onDiscard={() => setDraft(settings.client.player)} onReset={() => setDraft({ ...settings.client.player, playerBackend: "mpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w" })} />
    </div>
  )
}

function PlaybackSettings() {
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const [draft, setDraft] = useDraft(settings?.client.playback)
  const mutation = useMutation({ mutationFn: (value: ClientSettings["client"]["playback"]) => api.settingsPatch.playback(value), onSuccess: (saved) => saveSettings(saved), onError: (error: Error) => toast.error(error.message) })
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
  const [draft, setDraft] = useDraft(settings?.client.application)
  const mutation = useMutation({ mutationFn: (value: ClientSettings["client"]["application"]) => api.settingsPatch.application(value), onSuccess: (saved) => saveSettings(saved), onError: (error: Error) => toast.error(error.message) })
  if (settingsQuery.error && !settings) return <SettingsError title="Application settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  return <div className="settings-page"><PageTitle title="Application" detail="Control window behavior and the diagnostics recorded by the desktop client." />
    <Section title="Window" description="These choices are applied immediately after saving.">
      <SettingsRow title="When the window closes" description="Minimize keeps MediaFlick and its player ready in the background."><SelectField label="Close behavior" value={draft.closeBehavior} onValueChange={(closeBehavior) => setDraft({ ...draft, closeBehavior })} options={[{ value: "exit_app", label: "Exit MediaFlick" }, { value: "minimize_window", label: "Minimize window" }]} /></SettingsRow>
      <SettingsRow title="Show scrollbars" description="Reveal native scrollbars instead of the immersive hidden treatment."><Switch aria-label="Show scrollbars" checked={draft.showScrollbars} onCheckedChange={(showScrollbars) => setDraft({ ...draft, showScrollbars })} /></SettingsRow>
    </Section>
    <Section title="Diagnostics" description="A log-level change is picked up on the next application launch.">
      <SettingsRow title="Log level" description="Use Debug only while investigating a problem."><SelectField label="Log level" value={draft.logLevel} onValueChange={(logLevel) => setDraft({ ...draft, logLevel })} options={[{ value: "trace", label: "Trace" }, { value: "debug", label: "Debug" }, { value: "info", label: "Info" }, { value: "warn", label: "Warn" }, { value: "error", label: "Error" }]} /></SettingsRow>
    </Section>
    <SaveBar dirty={!same(draft, settings.client.application)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.client.application)} onReset={() => setDraft({ closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" })} restartRequired={draft.logLevel !== settings.client.application.logLevel} />
  </div>
}

function previewRatingValue(source: RatingSourceDefinition) {
  if (source.format === "percent") return "88%"
  if (source.format === "stars") return "★4.2"
  if (source.format === "integer") return "84"
  return source.scaleMax <= 5 ? "4.2" : "8.4"
}

function AppearancePreview({
  appearance,
  ratingSources,
}: {
  appearance: AppearanceSettings
  ratingSources: RatingSourceDefinition[]
}) {
  const systemReducedMotion = usePrefersReducedMotion()
  const reducedMotion = systemReducedMotion || appearance.reducedMotion
  const previewRatings = appearance.ratingSources
    .map((id) => ratingSources.find((source) => source.id === id))
    .filter((source): source is RatingSourceDefinition => Boolean(source))
    .slice(0, 2)
  const style = cssVariables({
    "--preview-artwork-intensity": String(appearance.artworkIntensity / 100),
    "--preview-backdrop-intensity": String(appearance.backdropIntensity / 100),
  })

  return (
    <figure
      className="appearance-preview"
      data-theme={appearance.theme}
      data-accent={appearance.accent}
      data-density={appearance.density}
      data-reduced-motion={reducedMotion}
      data-card-previews={appearance.cardPreviews}
      data-show-media-info={appearance.showMediaInfo}
      style={style}
      aria-labelledby="appearance-preview-title"
      aria-describedby="appearance-preview-description appearance-preview-motion-status"
    >
      <figcaption className="sr-only">
        <span id="appearance-preview-title">Live appearance preview</span>
        <span id="appearance-preview-description">
          A contained browsing shelf using the unsaved appearance controls.
        </span>
      </figcaption>
      <div className="appearance-preview-backdrop" aria-hidden />
      <div className="appearance-preview-scrim" aria-hidden />
      <div className="appearance-preview-shell">
        <header className="appearance-preview-chrome">
          <span className="appearance-preview-brand">MF</span>
          <span>Home</span>
          <span>Movies</span>
          <span className="appearance-preview-active">My List</span>
          <span className="appearance-preview-online">Online</span>
        </header>
        <div className="appearance-preview-content">
          <div className="appearance-preview-heading">
            <span className="appearance-preview-kicker">Continue watching</span>
            <h3>Tonight’s signal</h3>
          </div>
          <div className="appearance-preview-cards" aria-hidden>
            <div className="appearance-preview-card">
              <div className="appearance-preview-poster">
                <div className="appearance-preview-artwork" />
                {previewRatings.length > 0 && (
                  <dl className="appearance-preview-ratings">
                    {previewRatings.map((source) => (
                      <div key={source.id}>
                        <dt><RatingSourceIcon sourceId={source.id} /></dt>
                        <dd>{previewRatingValue(source)}</dd>
                      </div>
                    ))}
                  </dl>
                )}
                <div className="appearance-preview-media-info">
                  <span><ScanLine /> 4K <i>·</i> DV</span>
                  <span><AudioLines /> TrueHD <i>·</i> Atmos</span>
                </div>
                <div className="appearance-preview-card-actions">
                  <span><Play /></span>
                  <span><Plus /></span>
                  <span><ThumbsUp /></span>
                </div>
                <span className="appearance-preview-progress"><i /></span>
              </div>
              <strong>Midnight Frequency</strong>
              <small>42 min left</small>
            </div>
            <div className="appearance-preview-card appearance-preview-card-secondary">
              <div className="appearance-preview-poster">
                <div className="appearance-preview-artwork" />
              </div>
              <strong>Green Horizon</strong>
              <small>2026</small>
            </div>
          </div>
        </div>
      </div>
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
  const settingsQuery = useSettings()
  const ratingsQuery = useRatingsStatus()
  const { data: settings } = settingsQuery
  const { data: ratings } = ratingsQuery
  const [draft, setDraft] = useDraft(settings?.appearance)
  const mutation = useMutation({ mutationFn: (value: AppearanceSettings) => api.settingsPatch.appearance(value), onSuccess: (saved) => saveSettings(saved), onError: (error: Error) => toast.error(error.message) })
  if (settingsQuery.error && !settings) return <SettingsError title="Appearance settings unavailable" error={settingsQuery.error} onRetry={() => void settingsQuery.refetch()} />
  if (!settings || !draft) return <SettingsLoading />
  return <div className="settings-page"><PageTitle title="Appearance" detail="Tune the MediaFlick shell without changing your library or server settings." />
    <Section title="Live preview" description="Uses your unsaved choices here only; the rest of MediaFlick changes after Save.">
      <AppearancePreview appearance={draft} ratingSources={ratings?.sources ?? []} />
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
      <SettingsRow title="Artwork intensity" description={`${draft.artworkIntensity}%`}><Slider aria-label="Artwork intensity" className="w-52" value={[draft.artworkIntensity]} onValueChange={([artworkIntensity]) => setDraft({ ...draft, artworkIntensity })} /></SettingsRow>
      <SettingsRow title="Backdrop intensity" description={`${draft.backdropIntensity}%`}><Slider aria-label="Backdrop intensity" className="w-52" value={[draft.backdropIntensity]} onValueChange={([backdropIntensity]) => setDraft({ ...draft, backdropIntensity })} /></SettingsRow>
      <SettingsRow title="Reduce motion" description="Disable decorative transitions and automatic movement."><Switch aria-label="Reduce motion" checked={draft.reducedMotion} onCheckedChange={(reducedMotion) => setDraft({ ...draft, reducedMotion })} /></SettingsRow>
    </Section>
    <SaveBar dirty={!same(draft, settings.appearance)} saving={mutation.isPending} onSave={() => mutation.mutate(draft)} onDiscard={() => setDraft(settings.appearance)} onReset={() => setDraft({ theme: "system", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, cardPreviews: true, showMediaInfo: true, ratingSources: [] })} />
  </div>
}

export function RatingsIntegration() {
  const statusQuery = useRatingsStatus()
  const { data: status } = statusQuery
  const onStatus = (next: RatingsIntegrationStatus) => queryClient.setQueryData(queryKeys.ratingsStatus, next)
  if (statusQuery.error && !status) return <SettingsError title="Ratings status unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (!status) return <SettingsLoading />
  const origin = status.effectiveOrigin === "local_mdblist"
    ? "Your local MDBList key"
    : status.effectiveOrigin === "plugin"
      ? "Your server's MediaFlick plugin"
      : "Disabled — add an MDBList key or enable ratings on your server's MediaFlick plugin"
  return <div className="settings-page">
    <PageTitle title="MDBList Ratings" detail="Show ratings from MDBList on artwork and title details." />
    <Section title="Credential" description="The key is stored in the operating-system credential vault and never sent to your Jellyfin server.">
      <SettingsRow title="MDBList API key" description="Your personal key from mdblist.com. A local key takes precedence over the server's plugin.">
        <SecureCredentialField provider="mdblist" label="MDBList API key" status={status.local.mdblist} onStatus={onStatus} />
      </SettingsRow>
    </Section>
    <Section title="Status" description="Which rating sources appear on cards is chosen in Appearance.">
      <SettingsRow title="Ratings come from" description={origin}><span className="settings-status" data-status={status.available ? "verified" : "unverified"}>{status.available ? <CheckCircle2 /> : <AlertTriangle />}{status.available ? "available" : "disabled"}</span></SettingsRow>
      <SettingsRow title="Server plugin" description={status.plugin.detail}><span className="data-value">{status.plugin.available ? "available" : "not available"}</span></SettingsRow>
    </Section>
  </div>
}

function SignInRequired({ name }: { name: string }) {
  return <div className="settings-page"><PageTitle title={name} detail="This integration belongs to the signed-in Jellyfin account." /><Section title="Sign in required" description="Sign in to your Jellyfin server to configure this integration."><Button asChild><RouterLink to="/">Go to sign in</RouterLink></Button></Section></div>
}

function Letterboxd() {
  const statusQuery = useStatus()
  const { data: status } = statusQuery
  const profiles = useQuery({ queryKey: ["letterboxd", "profiles"], queryFn: api.letterboxd.profiles, enabled: Boolean(status?.authenticated), retry: false })
  const [entry, setEntry] = useState("")
  // Profile writes also change every movie's public-review projection.
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ["letterboxd"] })
  const add = useMutation({ mutationFn: api.letterboxd.add, onSuccess: () => { setEntry(""); refresh(); toast.success("Letterboxd profile added") }, onError: (error: Error) => toast.error(error.message) })
  const setEnabled = useMutation({ mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => api.letterboxd.setEnabled(id, enabled), onSuccess: refresh, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: api.letterboxd.remove, onSuccess: refresh, onError: (error: Error) => toast.error(error.message) })
  const verify = useMutation({ mutationFn: api.letterboxd.refresh, onSuccess: refresh, onError: (error: Error) => toast.error(error.message) })
  const open = useMutation({ mutationFn: api.letterboxd.open, onError: (error: Error) => toast.error(error.message) })
  if (statusQuery.error && !status) return <SettingsError title="Letterboxd unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Letterboxd" />
  return <div className="settings-page"><PageTitle title="Letterboxd" detail="Connect public profiles using Letterboxd’s public RSS feed—no credentials are stored." />
    <Section title="Add profile" description="Enter a Letterboxd username or a canonical profile URL.">
      <form className="flex max-w-xl gap-2" onSubmit={(event) => { event.preventDefault(); if (entry.trim()) add.mutate(entry) }}><Input value={entry} onChange={(event) => setEntry(event.target.value)} placeholder="letterboxd username or URL" /><Button disabled={add.isPending}>{add.isPending ? "Verifying…" : "Add profile"}</Button></form>
    </Section>
    <Section title="Connected profiles" description="Profiles are visible only to this Jellyfin account on this server.">
      {profiles.isPending ? <p className="text-sm text-muted-foreground">Loading profiles…</p> : profiles.error && !profiles.data ? <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-sm"><span>{profiles.error.message}</span><Button size="sm" variant="outline" onClick={() => void profiles.refetch()}>Try again</Button></div> : profiles.data?.profiles.length ? <div className="space-y-3">{profiles.data.profiles.map((profile) => <ProfileCard key={profile.id} profile={profile} onEnabled={(enabled) => setEnabled.mutate({ id: profile.id, enabled })} onRefresh={() => verify.mutate(profile.id)} onOpen={() => open.mutate(profile.id)} onRemove={() => remove.mutate(profile.id)} />)}</div> : <p className="text-sm text-muted-foreground">No Letterboxd profiles connected yet.</p>}
    </Section>
  </div>
}

function ProfileCard({ profile, onEnabled, onRefresh, onOpen, onRemove }: { profile: LetterboxdProfile; onEnabled: (enabled: boolean) => void; onRefresh: () => void; onOpen: () => void; onRemove: () => void }) {
  return <div className="settings-profile-card"><div className="min-w-0"><div className="flex items-center gap-2"><h3 className="font-medium">{profile.displayName}</h3><span className="settings-status" data-status={profile.verificationStatus}>{profile.verificationStatus === "verified" ? <CheckCircle2 /> : <AlertTriangle />}{profile.verificationStatus}</span></div><p className="mt-1 truncate text-sm text-muted-foreground">{profile.canonicalUrl}</p></div><div className="flex flex-wrap items-center justify-end gap-1"><Switch aria-label={`Enable ${profile.displayName}`} checked={profile.enabled} onCheckedChange={onEnabled} /><Button size="icon-sm" variant="ghost" aria-label="Refresh profile" onClick={onRefresh}><RefreshCw /></Button><Button size="icon-sm" variant="ghost" aria-label="Open profile" onClick={onOpen}><ExternalLink /></Button><Button size="icon-sm" variant="ghost" aria-label="Remove profile" onClick={onRemove}><Trash2 /></Button></div></div>
}

function Seerr() {
  const statusQuery = useStatus()
  const seerrQuery = useSeerrStatus()
  const { data: status } = statusQuery
  const { data: seerr } = seerrQuery
  const [setup, setSetup] = useState(false)
  if (statusQuery.error && !status) return <SettingsError title="Seerr unavailable" error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Seerr" />
  if (seerrQuery.error && !seerr) return <SettingsError title="Seerr status unavailable" error={seerrQuery.error} onRetry={() => void seerrQuery.refetch()} />
  if (seerrQuery.isPending) return <SettingsLoading />
  return <div className="settings-page"><PageTitle title="Seerr" detail="Connect Seerr to discover titles and submit requests from MediaFlick." />
    <Section title="Connection" description="Seerr signs in with your Jellyfin account; no server password is retained by MediaFlick.">
      <SettingsRow title="Status" description={seerr?.linked ? "This Jellyfin account can request through the connected Seerr instance." : "No Seerr instance is linked for this Jellyfin account."}><Button variant={seerr?.linked ? "outline" : "default"} onClick={() => setSetup(true)}>{seerr?.linked ? "Manage connection" : "Set up Seerr"}</Button></SettingsRow>
    </Section>
    {setup && <SeerrSetupDialog onClose={() => setSetup(false)} />}
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
  return <div className="settings-layout"><SettingsNavigation /><main className="settings-main"><Routes><Route index element={<Navigate to="/settings/client/player" replace />} /><Route path="client/player" element={<PlayerSettings />} /><Route path="client/playback" element={<PlaybackSettings />} /><Route path="client/application" element={<ApplicationSettings />} /><Route path="appearance" element={<Appearance />} /><Route path="integrations/ratings" element={<RatingsIntegration />} /><Route path="integrations/letterboxd" element={<Letterboxd />} /><Route path="integrations/seerr" element={<Seerr />} /><Route path="*" element={<Navigate to="/settings" replace />} /></Routes></main></div>
}
