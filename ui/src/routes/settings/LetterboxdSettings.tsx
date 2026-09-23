import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { AlertTriangle, CheckCircle2, ExternalLink, RefreshCw, Trash2 } from "lucide-react"
import { useMemo, useState, type ReactNode } from "react"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, type LetterboxdProfile, type Status } from "@/lib/api"
import { accountKey, letterboxdProfilesQueryOptions, useStatus } from "@/lib/queries"
import { queryKeys } from "@/lib/query-client"
import { same } from "./helpers"
import { PageTitle, Section, SettingsError, SettingsLoading, SignInRequired } from "./shared"

type PendingChanges = { enabled: Record<string, boolean>; additions: string[]; removals: string[] }

function ProfileCard({
  profile,
  onEnabled,
  onRefresh,
  onOpen,
  onRemove,
}: {
  profile: LetterboxdProfile
  onEnabled: (enabled: boolean) => void
  onRefresh: () => void
  onOpen: () => void
  onRemove: () => void
}) {
  const switchId = `letterboxd-enabled-${profile.id}`
  return (
    <div className="settings-profile-card">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <h3 className="font-medium"><Label htmlFor={switchId}>{profile.displayName}</Label></h3>
          <span className="settings-status" data-status={profile.verificationStatus}>
            {profile.verificationStatus === "verified" ? <CheckCircle2 /> : <AlertTriangle />}
            {profile.verificationStatus}
          </span>
        </div>
        <p className="mt-1 truncate text-sm text-muted-foreground">{profile.canonicalUrl}</p>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-1">
        <Switch
          id={switchId}
          aria-label={`Enable ${profile.displayName}`}
          checked={profile.enabled}
          onCheckedChange={onEnabled}
        />
        <Button size="icon-sm" variant="ghost" aria-label="Refresh profile" onClick={onRefresh}><RefreshCw /></Button>
        <Button size="icon-sm" variant="ghost" aria-label="Open profile" onClick={onOpen}><ExternalLink /></Button>
        <Button size="icon-sm" variant="ghost" aria-label="Remove profile" onClick={onRemove}><Trash2 /></Button>
      </div>
    </div>
  )
}

function PendingRow({ title, note, action }: { title: string; note: string; action: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border p-3">
      <div className="min-w-0">
        <p className="break-all">{title}</p>
        <p className="text-sm text-muted-foreground">{note}</p>
      </div>
      {action}
    </div>
  )
}

export default function LetterboxdSettings() {
  const cache = useQueryClient()
  const statusQuery = useStatus()
  const { data: status } = statusQuery
  const profiles = useQuery({ ...letterboxdProfilesQueryOptions(), enabled: Boolean(status?.authenticated) })
  const [entry, setEntry] = useState("")
  const [additions, setAdditions] = useState<string[]>([])
  const [removals, setRemovals] = useState<string[]>([])
  const account = accountKey(status)
  const savedEnabled = useMemo<Record<string, boolean> | null>(() => profiles.data
    ? Object.fromEntries(profiles.data.profiles.map((profile) => [profile.id, profile.enabled]))
    : null, [profiles.data])
  const [enabledDraft, setEnabledDraft] = useSourceDraft(savedEnabled, account)
  const profilesKey = letterboxdProfilesQueryOptions().queryKey
  const isCurrentAccount = () => accountKey(cache.getQueryData<Status>(queryKeys.status)) === account
  const checkAccount = () => {
    if (!isCurrentAccount()) throw new Error("The signed-in account changed. Remaining profile changes were not saved.")
  }
  // Profile writes also change every movie's public-review projection.
  const refresh = () => {
    if (isCurrentAccount()) void cache.invalidateQueries({ queryKey: ["letterboxd"] })
  }
  const rememberProfile = (profile: LetterboxdProfile) => {
    checkAccount()
    cache.setQueryData(profilesKey, (current) => {
      const existing = current?.profiles ?? []
      return {
        profiles: existing.some((candidate) => candidate.id === profile.id)
          ? existing.map((candidate) => candidate.id === profile.id ? profile : candidate)
          : [...existing, profile],
      }
    })
  }
  const save = useMutation({
    mutationFn: async (submitted: PendingChanges) => {
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
        cache.setQueryData(profilesKey, (current) => ({
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
  const verify = useMutation({
    mutationFn: api.letterboxd.refresh,
    onSuccess: refresh,
    onError: (error: Error) => toast.error(error.message),
  })
  const open = useMutation({ mutationFn: api.letterboxd.open, onError: (error: Error) => toast.error(error.message) })
  const queuedAdditions = () => [...new Set([...additions, ...(entry.trim() ? [entry.trim()] : [])])]
  const clearPending = () => {
    setEntry("")
    setAdditions([])
    setRemovals([])
    save.reset()
  }
  if (statusQuery.error && !status) {
    return (
      <SettingsError
        title="Letterboxd unavailable"
        error={statusQuery.error}
        onRetry={() => void statusQuery.refetch()}
      />
    )
  }
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Letterboxd" />
  const dirty = Boolean(
    entry.trim() || additions.length || removals.length ||
    enabledDraft && savedEnabled && !same(enabledDraft, savedEnabled),
  )

  let connected
  if (profiles.isPending) {
    connected = <p className="text-sm text-muted-foreground">Loading profiles…</p>
  } else if (profiles.error && !profiles.data) {
    connected = (
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-sm">
        <span>{profiles.error.message}</span>
        <Button size="sm" variant="outline" onClick={() => void profiles.refetch()}>Try again</Button>
      </div>
    )
  } else if (profiles.data?.profiles.length) {
    connected = (
      <div className="space-y-3">
        {profiles.data.profiles.map((profile) => removals.includes(profile.id) ? (
          <PendingRow
            key={profile.id}
            title={profile.displayName}
            note="Will be removed when you save"
            action={(
              <Button
                variant="outline"
                aria-label={`Undo removal of ${profile.displayName}`}
                onClick={() => setRemovals((current) => current.filter((id) => id !== profile.id))}
              >
                Undo
              </Button>
            )}
          />
        ) : (
          <ProfileCard
            key={profile.id}
            profile={{ ...profile, enabled: enabledDraft?.[profile.id] ?? profile.enabled }}
            onEnabled={(enabled) => {
              if (enabledDraft) setEnabledDraft({ ...enabledDraft, [profile.id]: enabled })
            }}
            onRefresh={() => verify.mutate(profile.id)}
            onOpen={() => open.mutate(profile.id)}
            onRemove={() => setRemovals((current) => [...current, profile.id])}
          />
        ))}
      </div>
    )
  } else {
    connected = <p className="text-sm text-muted-foreground">No Letterboxd profiles connected yet.</p>
  }

  return (
    <div className="settings-page">
      <PageTitle title="Letterboxd" />
      <fieldset disabled={save.isPending} className="min-w-0 space-y-5">
        <legend className="sr-only">Letterboxd profiles</legend>
        <Section title="Add profile" description="Profiles are verified and connected when you save.">
          <form
            className="max-w-xl space-y-2"
            onSubmit={(event) => {
              event.preventDefault()
              setAdditions(queuedAdditions())
              setEntry("")
            }}
          >
            <Label htmlFor="letterboxd-profile">Letterboxd username or profile URL</Label>
            <p id="letterboxd-profile-help" className="text-sm text-muted-foreground">
              Enter a username or a profile URL such as https://letterboxd.com/username/.
            </p>
            <div className="flex gap-2">
              <Input
                id="letterboxd-profile"
                aria-describedby="letterboxd-profile-help"
                value={entry}
                onChange={(event) => setEntry(event.target.value)}
                placeholder="Username or profile URL"
              />
              <Button disabled={!entry.trim() || !profiles.data}>Add profile</Button>
            </div>
          </form>
          {additions.map((input) => (
            <PendingRow
              key={input}
              title={input}
              note="Will be added when you save"
              action={(
                <Button
                  variant="ghost"
                  onClick={() => setAdditions((current) => current.filter((added) => added !== input))}
                  aria-label={`Cancel adding ${input}`}
                >
                  Cancel
                </Button>
              )}
            />
          ))}
        </Section>
        <Section title="Connected profiles" description="Changes apply after Save. Discard restores the saved profiles.">
          {connected}
        </Section>
      </fieldset>
      {save.error && (
        <p role="alert" className="text-sm text-destructive">
          Could not save all changes. Your remaining edits are kept. {save.error.message}
        </p>
      )}
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
        onDiscard={() => {
          clearPending()
          setEnabledDraft(savedEnabled)
        }}
        onReset={() => {
          clearPending()
          if (savedEnabled) setEnabledDraft(Object.fromEntries(Object.keys(savedEnabled).map((id) => [id, true])))
        }}
      />
    </div>
  )
}
