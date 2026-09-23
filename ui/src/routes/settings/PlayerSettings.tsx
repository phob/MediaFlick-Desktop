import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Download, FolderOpen } from "lucide-react"
import { useCallback, useRef, useState } from "react"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import SettingsNumberField from "@/components/SettingsNumberField"
import { ShortcutRecorder } from "@/components/ShortcutRecorder"
import { SubtitlePreview } from "@/components/SubtitlePreview"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, playerSettingsWrite, type ClientSettings, type PlayerComfort } from "@/lib/api"
import { jsonNumber, jsonString } from "@/lib/json"
import { PLAYER_SHORTCUTS, shortcutError } from "@/lib/player-shortcuts"
import { useSettings } from "@/lib/queries"
import { queryKeys } from "@/lib/query-client"
import { isSettingsNumberValid } from "@/lib/settings-numbers"
import type { ShellEvent } from "@/lib/shell-events"
import { DEFAULT_COMFORT } from "@/lib/viewing"
import { defaultPlayerSettings } from "./defaults"
import { requestId, same, saveSettings, useShellEvents } from "./helpers"
import { PageTitle, Section, SelectField, SettingsError, SettingsLoading, SettingsRow } from "./shared"

type PlayerDraft = ClientSettings["client"]["player"]

const COMFORT_NUMBERS = [
  ["subtitleSize", "Subtitle size (%)", 50, 200],
  ["subtitleOutline", "Subtitle outline", 0, 8],
  ["subtitleBackground", "Subtitle background (%)", 0, 100],
  ["subtitlePosition", "Subtitle vertical position", 0, 100],
  ["seekBackSeconds", "Seek backward seconds", 1, 120],
  ["seekForwardSeconds", "Seek forward seconds", 1, 120],
] as const

type InstallState = { state: string; message?: string; downloaded?: number; total?: number | null }

/** Out-of-range numbers keep their last valid value in the subtitle preview. */
function previewComfort(comfort: PlayerComfort): PlayerComfort {
  const valid = COMFORT_NUMBERS.map(([key, , min, max]) => [
    key,
    isSettingsNumberValid(comfort[key], min, max) ? comfort[key] : DEFAULT_COMFORT[key],
  ])
  return { ...comfort, ...Object.fromEntries(valid) }
}

function installDetail(install: InstallState) {
  if (install.state === "downloading" && install.total) {
    return `Downloading mpv (${Math.round((install.downloaded ?? 0) / install.total * 100)}%)`
  }
  if (install.state === "extracting") return "Extracting mpv…"
  if (install.state === "completed") return "mpv installed. Save to keep any other player changes."
  if (install.state === "failed") return install.message
  return undefined
}

function openMpvHelp() {
  void api.shell.mpvHelp().catch((error: Error) => toast.error(error.message))
}

export default function PlayerSettings() {
  const cache = useQueryClient()
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const [draft, setDraft, updateDraft, acceptSaved] = useSourceDraft(settings?.client.player)
  const [install, setInstall] = useState<InstallState>({ state: "idle" })
  const pendingPicker = useRef<string | null>(null)
  const pendingInstall = useRef<string | null>(null)
  const [picking, setPicking] = useState(false)
  const mutation = useMutation({
    mutationFn: (value: PlayerDraft) => api.settingsPatch.player(playerSettingsWrite(value)),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.client.player, submitted)
      const previousBackend = settings?.client.player.playerBackend
      const needsRestart = submitted.playerBackend !== previousBackend &&
        (submitted.playerBackend === "libmpv" || previousBackend === "libmpv")
      saveSettings(
        cache,
        saved,
        needsRestart
          ? "Player saved. Restart MediaFlick to apply the built-in player configuration."
          : "Player settings saved",
      )
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const onShellEvent = useCallback((event: ShellEvent) => {
    if (event.type === "file-picker-completed") {
      const completedRequestId = jsonString(event.payload.requestId)
      if (completedRequestId === null || pendingPicker.current !== completedRequestId) return
      pendingPicker.current = null
      setPicking(false)

      const pickerError = jsonString(event.payload.error)
      if (pickerError) {
        toast.error(pickerError)
        return
      }
      // A null path is the native dialog's cancellation result. It settles the
      // request but deliberately leaves the user's existing draft untouched.
      const path = jsonString(event.payload.path)
      if (path === null) return
      updateDraft((current) => current ? { ...current, mpvPath: path } : current)
    }
    if (event.type === "mpv-install-progress") {
      const completedRequestId = jsonString(event.payload.requestId)
      if (completedRequestId === null || pendingInstall.current !== completedRequestId) return
      const state = jsonString(event.payload.state) ?? "idle"
      const message = jsonString(event.payload.message) ?? undefined
      const downloaded = jsonNumber(event.payload.downloaded) ?? 0
      const total = jsonNumber(event.payload.total)
      setInstall({ state, message, downloaded, total })
      const installedPath = jsonString(event.payload.path)
      if (state === "completed" && installedPath !== null) {
        pendingInstall.current = null
        updateDraft((current) => current ? { ...current, mpvPath: installedPath } : current)
        void cache.invalidateQueries({ queryKey: queryKeys.settings })
      }
      if (state === "failed") {
        pendingInstall.current = null
        if (message) toast.error(message)
      }
    }
  }, [cache, updateDraft])
  useShellEvents(onShellEvent)
  if (settingsQuery.error && !settings) {
    return (
      <SettingsError
        title="Player settings unavailable"
        error={settingsQuery.error}
        onRetry={() => void settingsQuery.refetch()}
      />
    )
  }
  if (!settings || !draft) return <SettingsLoading />

  const saved = settings.client.player
  const dirty = !same(draft, saved)
  const comfort = draft.comfort ?? DEFAULT_COMFORT
  const keysError = draft.playerBackend === "libmpv" ? shortcutError(comfort, draft.markWatchedNext) : null
  const validNumbers = COMFORT_NUMBERS.every(([key, , min, max]) => isSettingsNumberValid(comfort[key], min, max))
  const backendChanged = draft.playerBackend !== saved.playerBackend
  const backendChangeNeedsRestart = backendChanged &&
    (draft.playerBackend === "libmpv" || saved.playerBackend === "libmpv")
  const restartMessage = backendChangeNeedsRestart
    ? draft.playerBackend === "libmpv"
      ? "Restart MediaFlick to enable the built-in player."
      : "Restart MediaFlick to switch player backends."
    : undefined
  const installing = ["queued", "downloading", "extracting"].includes(install.state)
  const update = <Key extends keyof PlayerDraft>(key: Key, value: PlayerDraft[Key]) =>
    setDraft({ ...draft, [key]: value })
  const updateComfort = <Key extends keyof PlayerComfort>(key: Key, value: PlayerComfort[Key]) =>
    setDraft({ ...draft, comfort: { ...comfort, [key]: value } })

  const pickMpv = () => {
    if (pendingPicker.current) return
    const id = requestId()
    pendingPicker.current = id
    setPicking(true)
    const settle = () => {
      if (pendingPicker.current !== id) return
      pendingPicker.current = null
      setPicking(false)
    }
    void api.shell.filePicker(id).then((response) => {
      if (response.requestId === id) return
      settle()
      toast.error("The file picker returned an unexpected request identifier.")
    }).catch((error: Error) => {
      settle()
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

  return (
    <div className="settings-page">
      <PageTitle title="Player" />
      <Section
        title="Playback backend"
        description="The built-in player uses bundled libmpv on Windows and system libmpv on Linux."
      >
        <SettingsRow
          controlId="settings-player"
          title="Player"
          description="External mpv keeps its own config, scripts, shaders, and SVP setup."
        >
          <SelectField
            id="settings-player"
            aria-describedby="settings-player-help"
            label="Player backend"
            value={draft.playerBackend}
            onValueChange={(playerBackend) => update("playerBackend", playerBackend)}
            options={[
              { value: "libmpv", label: "Built-in player", disabled: !settings.capabilities.libmpv },
              { value: "mpv", label: "External mpv" },
            ]}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-start-fullscreen"
          title="Start fullscreen"
          description="Use a full-screen player window by default."
        >
          <SelectField
            id="settings-start-fullscreen"
            aria-describedby="settings-start-fullscreen-help"
            label="Default fullscreen"
            value={draft.defaultFullscreen}
            onValueChange={(defaultFullscreen) => update("defaultFullscreen", defaultFullscreen)}
            options={[
              { value: "fullscreen", label: "Fullscreen" },
              { value: "windowed", label: "Windowed" },
            ]}
          />
        </SettingsRow>
      </Section>
      {draft.playerBackend === "mpv" && (
        <Section title="Executables" description="Paths are saved locally and are never sent to your Jellyfin server.">
          <SettingsRow
            controlId="mpv-path"
            title="mpv executable"
            description="Select mpv.exe or use the installer on supported Windows builds."
          >
            <div className="flex w-full max-w-md gap-2">
              <Input
                id="mpv-path"
                aria-describedby="mpv-path-help"
                value={draft.mpvPath ?? ""}
                onChange={(event) => update("mpvPath", event.target.value || null)}
                placeholder="Path to mpv"
              />
              <Button
                variant="outline"
                size="icon"
                aria-label="Choose mpv executable"
                aria-busy={picking}
                disabled={picking}
                onClick={pickMpv}
              >
                <FolderOpen />
              </Button>
            </div>
          </SettingsRow>
          {settings.capabilities.mpvInstaller ? (
            <SettingsRow
              title="Install mpv"
              description={installDetail(install) ?? "Download and install the supported mpv build beside MediaFlick."}
            >
              <div className="flex gap-2">
                <Button variant="outline" onClick={installMpv} disabled={installing}>
                  <Download /> {install.state === "idle" || install.state === "failed" ? "Install mpv" : "Installing…"}
                </Button>
                <Button variant="ghost" onClick={openMpvHelp}>Installation help</Button>
              </div>
            </SettingsRow>
          ) : (
            <SettingsRow title="Install mpv" description="See mpv’s installation guide for your operating system.">
              <Button variant="ghost" onClick={openMpvHelp}>Installation help</Button>
            </SettingsRow>
          )}
        </Section>
      )}
      {draft.playerBackend === "libmpv" && (
        <Section
          title="Built-in player comfort"
          description="Subtitle changes apply to the next playback. Styled bitmap subtitles may keep their own appearance."
        >
          <SubtitlePreview comfort={previewComfort(comfort)} />
          {COMFORT_NUMBERS.map(([key, label, min, max]) => (
            <SettingsRow key={key} controlId={`comfort-${key}`} title={label}>
              <SettingsNumberField
                id={`comfort-${key}`}
                label={label}
                min={min}
                max={max}
                value={comfort[key]}
                onValueChange={(value) => updateComfort(key, value)}
              />
            </SettingsRow>
          ))}
        </Section>
      )}
      <Section
        title="Keyboard shortcuts"
        description={draft.playerBackend === "libmpv"
          ? "Record a key combination, or clear it to disable the binding. Space always pauses; Left and Right use the seek intervals above."
          : "MediaFlick provides the watched/next shortcut. Configure other shortcuts in mpv. Command and Option combinations work on macOS."}
      >
        <SettingsRow
          controlId="mark-watched-key"
          title="Mark watched key"
          description="Marks a movie watched, or marks an episode watched and plays the next available item. Works in the built-in player and external mpv. Record a key combination, or clear it to disable."
        >
          <ShortcutRecorder
            id="mark-watched-key"
            label="Mark watched key"
            value={draft.markWatchedNext ?? ""}
            onChange={(value) => update("markWatchedNext", value || null)}
          />
        </SettingsRow>
        {draft.playerBackend === "libmpv" && PLAYER_SHORTCUTS.map(([key, label]) => (
          <SettingsRow key={key} controlId={`shortcut-${key}`} title={label}>
            <ShortcutRecorder
              id={`shortcut-${key}`}
              label={label}
              value={comfort[key]}
              onChange={(value) => updateComfort(key, value)}
            />
          </SettingsRow>
        ))}
      </Section>
      {keysError && <p role="alert" className="text-sm text-destructive">{keysError}</p>}
      <SaveBar
        saveDisabled={!validNumbers || Boolean(keysError)}
        dirty={dirty}
        saving={mutation.isPending}
        onSave={() => mutation.mutate(draft)}
        onDiscard={() => setDraft(saved)}
        onReset={() => setDraft(defaultPlayerSettings(saved, settings.capabilities.libmpv))}
        restartMessage={restartMessage}
      />
    </div>
  )
}
