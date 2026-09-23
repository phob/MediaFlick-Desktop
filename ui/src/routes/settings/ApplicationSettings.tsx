import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Trash2 } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, type ClientSettings } from "@/lib/api"
import { useSettings, useStatus } from "@/lib/queries"
import { queryKeys, removeAccountQueryData } from "@/lib/query-client"
import { DEFAULT_APPLICATION_SETTINGS } from "./defaults"
import { same, saveSettings } from "./helpers"
import { PageTitle, Section, SelectField, SettingsError, SettingsLoading, SettingsRow } from "./shared"

type ApplicationDraft = ClientSettings["client"]["application"]

const DELETE_CONFIRMATION = "DELETE"
const LOCAL_DATA_SCOPE =
  "This removes account settings, playback choices, collection snapshots, and custom collection posters, then signs this device out."

export default function ApplicationSettings() {
  const cache = useQueryClient()
  const settingsQuery = useSettings()
  const { data: settings } = settingsQuery
  const { data: status } = useStatus()
  const [draft, setDraft, , acceptSaved] = useSourceDraft(settings?.client.application)
  const [deleteConfirmation, setDeleteConfirmation] = useState("")
  const mutation = useMutation({
    mutationFn: (value: ApplicationDraft) => api.settingsPatch.application(value),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.client.application, submitted)
      saveSettings(cache, saved)
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const deleteAccount = useMutation({
    mutationFn: api.collections.deleteLocalAccount,
    onSuccess: (anonymousStatus) => {
      cache.setQueryData(queryKeys.status, anonymousStatus)
      removeAccountQueryData(cache)
      void cache.resetQueries({ queryKey: queryKeys.settings })
      toast.success("Local account data deleted")
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (settingsQuery.error && !settings) {
    return (
      <SettingsError
        title="Application settings unavailable"
        error={settingsQuery.error}
        onRetry={() => void settingsQuery.refetch()}
      />
    )
  }
  if (!settings || !draft) return <SettingsLoading />
  const update = <Key extends keyof ApplicationDraft>(key: Key, value: ApplicationDraft[Key]) =>
    setDraft({ ...draft, [key]: value })
  const userName = status?.userName ?? "this account"
  const serverUrl = status?.serverUrl ?? "this server"
  const canDelete = deleteConfirmation === DELETE_CONFIRMATION && !deleteAccount.isPending
  return (
    <div className="settings-page">
      <PageTitle title="Application" />
      {(settings.recoveries?.length ?? 0) > 0 && (
        <Section title="Recovered local settings" description="MediaFlick preserved each damaged file before continuing.">
          {settings.recoveries?.map((recovery) => (
            <p
              key={recovery.area}
              className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-100"
              role="status"
            >
              {recovery.area}:{" "}
              {recovery.restoredBackup
                ? "the last valid backup was restored."
                : "defaults are in use because no valid backup was available."}
            </p>
          ))}
        </Section>
      )}
      <Section title="Window" description="These choices are applied immediately after saving.">
        <SettingsRow
          controlId="settings-when-the-window-closes"
          title="When the window closes"
          description="Minimize keeps MediaFlick and its player ready in the background."
        >
          <SelectField
            id="settings-when-the-window-closes"
            aria-describedby="settings-when-the-window-closes-help"
            label="Close behavior"
            value={draft.closeBehavior}
            onValueChange={(closeBehavior) => update("closeBehavior", closeBehavior)}
            options={[
              { value: "exit_app", label: "Exit MediaFlick" },
              { value: "minimize_window", label: "Minimize window" },
            ]}
          />
        </SettingsRow>
        <SettingsRow
          controlId="settings-show-scrollbars"
          title="Show scrollbars"
          description="Reveal native scrollbars instead of the immersive hidden treatment."
        >
          <Switch
            id="settings-show-scrollbars"
            aria-describedby="settings-show-scrollbars-help"
            aria-label="Show scrollbars"
            checked={draft.showScrollbars}
            onCheckedChange={(showScrollbars) => update("showScrollbars", showScrollbars)}
          />
        </SettingsRow>
      </Section>
      <Section title="Diagnostics" description="A log-level change is picked up on the next application launch.">
        <SettingsRow
          controlId="settings-log-level"
          title="Log level"
          description="Use Debug only while investigating a problem."
        >
          <SelectField
            id="settings-log-level"
            aria-describedby="settings-log-level-help"
            label="Log level"
            value={draft.logLevel}
            onValueChange={(logLevel) => update("logLevel", logLevel)}
            options={[
              { value: "trace", label: "Trace" },
              { value: "debug", label: "Debug" },
              { value: "info", label: "Info" },
              { value: "warn", label: "Warn" },
              { value: "error", label: "Error" },
            ]}
          />
        </SettingsRow>
      </Section>
      {status?.authenticated && (
        <Section
          title="Local account data"
          description={`Remove this device's data for ${userName} on ${serverUrl}. Nothing is deleted from Jellyfin.`}
        >
          <SettingsRow title="Delete local account data" description={LOCAL_DATA_SCOPE}>
            <div className="flex w-full max-w-md flex-col gap-2">
              <Input
                value={deleteConfirmation}
                onChange={(event) => setDeleteConfirmation(event.target.value)}
                placeholder={`Type ${DELETE_CONFIRMATION} to confirm`}
                aria-label={`Type ${DELETE_CONFIRMATION} to confirm local account deletion`}
              />
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="destructive" disabled={!canDelete}>
                    <Trash2 />
                    {deleteAccount.isPending ? "Deleting…" : "Delete local account data"}
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete local account data?</AlertDialogTitle>
                    <AlertDialogDescription>
                      Remove local data for {userName} on {serverUrl}? {LOCAL_DATA_SCOPE} Nothing is deleted from Jellyfin.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction
                      variant="destructive"
                      disabled={!canDelete}
                      onClick={() => deleteAccount.mutate()}
                    >
                      Delete local account data
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
          </SettingsRow>
        </Section>
      )}
      <SaveBar
        dirty={!same(draft, settings.client.application)}
        saving={mutation.isPending}
        onSave={() => mutation.mutate(draft)}
        onDiscard={() => setDraft(settings.client.application)}
        onReset={() => setDraft(DEFAULT_APPLICATION_SETTINGS)}
        restartMessage={draft.logLevel !== settings.client.application.logLevel
          ? "Log-level changes apply after restarting MediaFlick."
          : undefined}
      />
    </div>
  )
}
