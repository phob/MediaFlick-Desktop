import { ArrowDown, ArrowUp, ImagePlus, Layers, Pencil, Search, Trash2 } from "lucide-react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useEffect, useMemo, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { toast } from "sonner"
import collectionTemplateArt from "@/assets/collection-template.svg"
import CollectionTemplatePictogram from "@/components/CollectionTemplatePictogram"
import SaveBar from "@/components/SettingsSaveBar"
import SettingsNumberField from "@/components/SettingsNumberField"
import PublicListCombobox from "@/components/PublicListCombobox"
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { useSourceDraft } from "@/hooks/use-source-draft"
import {
  api,
  type CollectionCategory,
  type CollectionProfile,
  type CollectionProfileDraft,
  type CollectionSettings,
  type CollectionTemplate,
  type CollectionTemplates,
} from "@/lib/api"
import { jsonString } from "@/lib/json"
import { queryKeys } from "@/lib/query-client"
import {
  accountKey,
  useCollectionProfiles,
  useCollectionSettings,
  useCollectionTemplates,
  useStatus,
} from "@/lib/queries"
import {
  PageTitle,
  RecoveryNotice,
  Section,
  SelectField,
  SettingsError,
  SettingsLoading,
  SettingsRow,
  SignInRequired,
} from "./shared"

const CATEGORY_LABELS = {
  trending: "Trending",
  popular: "Popular",
  streamingServices: "Streaming services",
  topRated: "Top rated",
  inTheaters: "In theaters",
  upcoming: "Upcoming",
  onAir: "On air",
  editorial: "Editorial",
  custom: "Custom",
} satisfies Record<CollectionCategory, string>

function providerLabel(profile: Pick<CollectionProfileDraft, "source">) {
  if (profile.source.kind === "mdbListPublicList") return "MDBList"
  if (profile.source.kind === "tmdbCollection" || profile.source.kind === "tmdbDiscover") return "TMDB"
  return "Unavailable"
}

function sourceLabel(profile: Pick<CollectionProfileDraft, "source">) {
  if (profile.source.kind === "tmdbCollection") return "Exact collection"
  if (profile.source.kind === "mdbListPublicList") return "Public list"
  if (profile.source.kind === "tmdbDiscover") return "Discover"
  return "Unsupported source"
}

function draftFromTemplate(template: CollectionTemplate): CollectionProfileDraft {
  const { category: _, pictogram: __, id, ...draft } = template
  return structuredClone({
    ...draft,
    template: { id },
    customPosterId: null,
    availableOnHome: false,
  })
}

function draftFromProfile(profile: CollectionProfile): CollectionProfileDraft {
  const { id: _, revision: __, ...draft } = profile
  return structuredClone(draft)
}

function resultSignature(profile: CollectionProfileDraft) {
  return JSON.stringify({
    source: profile.source,
    mediaType: profile.mediaType,
    limit: profile.limit,
  })
}

type CollectionSettingsDraft = {
  modeSelection: CollectionSettings["effectiveMode"]
  includeUnreleased: boolean
  profileIds: string[]
}

const GENERAL_DESCRIPTION = "Choose which collection experience this account sees."

function GeneralSettings({ draft, onChange }: {
  draft: CollectionSettingsDraft | null
  onChange: (draft: CollectionSettingsDraft) => void
}) {
  const query = useCollectionSettings()
  const settings = query.data
  if (query.isError && !settings) {
    return (
      <Section title="General" description={GENERAL_DESCRIPTION}>
        <p className="text-sm text-destructive" role="alert">Collection settings could not be loaded.</p>
        <Button variant="outline" onClick={() => void query.refetch()}>Try again</Button>
      </Section>
    )
  }
  const loading = !settings || !draft
  return (
    <Section title="General" description={GENERAL_DESCRIPTION}>
      {settings?.recovery && (
        <RecoveryNotice>
          Damaged collection settings were moved aside
          {settings.recovery.restoredBackup ? " and the backup was restored." : ". Defaults are in use."}
        </RecoveryNotice>
      )}
      <SettingsRow
        controlId="collection-mode"
        title="Mode"
        description="MediaFlick adds Movie Franchises and My Collections. Jellyfin shows server BoxSets directly."
      >
        <SelectField
          id="collection-mode"
          aria-describedby="collection-mode-help"
          label="Collection mode"
          placeholder="Loading…"
          disabled={loading}
          value={draft?.modeSelection ?? ""}
          onValueChange={(modeSelection) => { if (draft) onChange({ ...draft, modeSelection }) }}
          options={[
            { value: "mediaFlick", label: "MediaFlick", disabled: !settings?.mediaFlickAvailable },
            { value: "jellyfin", label: "Jellyfin" },
          ]}
        />
      </SettingsRow>
      <SettingsRow
        controlId="collection-general-unreleased"
        title="Include unreleased titles"
        description="Show future and undated missing movies in Movie Franchises."
      >
        <Switch
          id="collection-general-unreleased"
          aria-describedby="collection-general-unreleased-help"
          aria-label="Include unreleased titles"
          checked={draft?.includeUnreleased ?? false}
          disabled={loading}
          onCheckedChange={(includeUnreleased) => { if (draft) onChange({ ...draft, includeUnreleased }) }}
        />
      </SettingsRow>
    </Section>
  )
}

function ConfiguredProfiles({ profileIds, onProfileIdsChange, onEdit }: {
  profileIds: string[]
  onProfileIdsChange: (profileIds: string[]) => void
  onEdit: (profile: CollectionProfile) => void
}) {
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = accountKey(status)
  const query = useCollectionProfiles()
  const settings = useCollectionSettings()
  const byId = new Map(query.data?.profiles.map((profile) => [profile.id, profile]))
  const profiles = profileIds.map((id) => byId.get(id)).filter((profile): profile is CollectionProfile => Boolean(profile))
  const disabled = !settings.data
  const reorder = (from: number, to: number) => {
    if (to < 0 || to >= profiles.length) return
    const next = [...profileIds]
    ;[next[from], next[to]] = [next[to], next[from]]
    onProfileIdsChange(next)
  }
  const remove = useMutation({
    mutationFn: (profile: CollectionProfile) => api.collections.deleteProfile(profile.id),
    onSuccess: (_, profile) => {
      void cache.invalidateQueries({ queryKey: ["collections", account] })
      void cache.invalidateQueries({ queryKey: queryKeys.homeSettings })
      void cache.invalidateQueries({ queryKey: queryKeys.home })
      toast.success(`${profile.title} deleted`)
    },
    onError: (error: Error) => toast.error(error.message),
  })
  return (
    <Section title="Configured My Collections" description="Order, edit, or remove the collections saved for this account.">
      {query.isError ? (
        <div className="flex flex-col items-start gap-2">
          <p className="text-sm text-destructive" role="alert">Configured collections could not be loaded.</p>
          <Button variant="outline" onClick={() => void query.refetch()}>Try again</Button>
        </div>
      ) : !profiles.length ? (
        <p className="text-sm text-muted-foreground">No collections are configured.</p>
      ) : profiles.map((profile, index) => (
        <div key={profile.id} className="flex flex-wrap items-center gap-3 rounded-lg border p-3">
          <div className="min-w-0 flex-1">
            <div className="truncate font-medium">{profile.title}</div>
            <div className="mt-1 flex flex-wrap gap-1">
              <Badge variant="outline">{providerLabel(profile)}</Badge>
              <Badge variant="outline">{sourceLabel(profile)}</Badge>
              <span className="text-xs text-muted-foreground">Template {profile.template.id}</span>
            </div>
            {query.data?.errors?.[profile.id] && <p className="mt-1 text-xs text-destructive">{query.data.errors[profile.id]}</p>}
          </div>
          <div className="flex gap-1">
            <Button size="icon" variant="ghost" disabled={disabled || index === 0} aria-label={`Move ${profile.title} up`} onClick={() => reorder(index, index - 1)}><ArrowUp /></Button>
            <Button size="icon" variant="ghost" disabled={disabled || index === profiles.length - 1} aria-label={`Move ${profile.title} down`} onClick={() => reorder(index, index + 1)}><ArrowDown /></Button>
            <Button size="icon" variant="ghost" disabled={disabled || Boolean(query.data?.errors?.[profile.id])} aria-label={`Edit ${profile.title}`} onClick={() => onEdit(profile)}><Pencil /></Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button size="icon" variant="ghost" disabled={disabled || remove.isPending} aria-label={`Delete ${profile.title}`}><Trash2 /></Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete “{profile.title}”?</AlertDialogTitle>
                  <AlertDialogDescription>Its saved results will be removed from this device.</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction variant="destructive" disabled={disabled || remove.isPending} onClick={() => remove.mutate(profile)}>Delete collection</AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </div>
      ))}
    </Section>
  )
}

function TemplateCatalog({
  catalog,
  disabled,
  onAdd,
}: {
  catalog: CollectionTemplates
  disabled: boolean
  onAdd: (template: CollectionTemplate) => void
}) {
  const [search, setSearch] = useState("")
  const normalized = search.trim().toLocaleLowerCase()
  const matching = useMemo(
    () => catalog.templates.filter(({ template }) => !normalized || `${template.title} ${template.description}`.toLocaleLowerCase().includes(normalized)),
    [catalog.templates, normalized],
  )
  return (
    <Section title="Template catalog" description="Templates are starting points. Saving one copies its current values into your account.">
      <div className="relative max-w-md">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input className="pl-9" value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search templates" aria-label="Search templates" />
      </div>
      {catalog.categories.map((category) => {
        const rows = matching.filter(({ template }) => template.category === category)
        if (!rows.length) return null
        return (
          <section key={category} className="flex flex-col gap-2">
            <h3 className="font-medium">{CATEGORY_LABELS[category]}</h3>
            <div className="grid gap-2 md:grid-cols-2">
              {rows.map(({ template, available }) => (
                <Button
                  key={template.id}
                  type="button"
                  variant="outline"
                  disabled={disabled || !available}
                  onClick={() => onAdd(template)}
                  className="h-auto min-w-0 justify-start rounded-lg p-3 text-left font-normal whitespace-normal"
                >
                  <div className="flex w-full min-w-0 gap-3">
                    <CollectionTemplatePictogram category={template.category} pictogram={template.pictogram} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-start justify-between gap-2">
                        <span className="font-medium">{template.title}</span>
                        <Badge variant={available ? "outline" : "secondary"}>{available ? providerLabel(template) : `${providerLabel(template)} unavailable`}</Badge>
                      </div>
                      {template.description && <p className="mt-1 text-sm text-muted-foreground">{template.description}</p>}
                    </div>
                  </div>
                </Button>
              ))}
            </div>
          </section>
        )
      })}
    </Section>
  )
}

function CollectionWizard({
  initial,
  profileId,
  open,
  onOpenChange,
}: {
  initial: CollectionProfileDraft | null
  profileId: string | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = accountKey(status)
  const [draft, setDraft] = useState<CollectionProfileDraft | null>(initial)
  const [preview, setPreview] = useState<Awaited<ReturnType<typeof api.collections.preview>> | null>(null)
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)
  const [posterError, setPosterError] = useState<string | null>(null)
  const [posterName, setPosterName] = useState("")
  const posterInput = useRef<HTMLInputElement>(null)
  const posterRequest = useRef<AbortController | null>(null)
  const [previewing, setPreviewing] = useState(false)
  const previewRequest = useRef<{ controller: AbortController | null; generation: number }>({
    controller: null,
    generation: 0,
  })
  const settings = useCollectionSettings()
  const saveProfile = useMutation({
    mutationFn: (value: CollectionProfileDraft) =>
      profileId ? api.collections.updateProfile(profileId, value) : api.collections.createProfile(value),
    onSuccess: () => {
      void cache.invalidateQueries({ queryKey: ["collections", account] })
      void cache.invalidateQueries({ queryKey: queryKeys.homeSettings })
      void cache.invalidateQueries({ queryKey: queryKeys.home })
      toast.success(profileId ? "Collection saved" : "Collection created")
      onOpenChange(false)
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const busy = previewing || saveProfile.isPending

  useEffect(() => () => {
    previewRequest.current.generation += 1
    previewRequest.current.controller?.abort()
    posterRequest.current?.abort()
  }, [])

  if (!draft) return null

  const mediaTypeChoices = draft.source.kind === "mdbListPublicList"
    ? [
        { value: "movie" as const, label: "Movie" },
        { value: "series" as const, label: "Series" },
        { value: "mixed" as const, label: "Mixed" },
      ]
    : draft.source.kind === "tmdbDiscover"
      ? [
          { value: "movie" as const, label: "Movie" },
          { value: "series" as const, label: "Series" },
        ]
      : [{ value: "movie" as const, label: "Movie" }]

  const refreshesResults = !profileId || resultSignature(draft) !== resultSignature(initial ?? draft)
  const readOnly = !settings.data
  const providerAvailable = draft.source.kind === "mdbListPublicList"
    ? Boolean(settings.data?.readiness.mdblist)
    : Boolean(settings.data?.readiness.tmdb)
  const validLimit = draft.limit.kind === "all"
    || Number.isInteger(draft.limit.count) && draft.limit.count >= 1 && draft.limit.count <= 500
  const validMediaType = mediaTypeChoices.some((choice) => choice.value === draft.mediaType)
  const validDraft = Boolean(draft.title.trim()) && validLimit && validMediaType

  const invalidatePreview = () => {
    previewRequest.current.generation += 1
    previewRequest.current.controller?.abort()
    previewRequest.current.controller = null
    setPreview(null)
    setPreviewError(null)
    setPreviewing(false)
  }

  const patch = (change: Partial<CollectionProfileDraft>, resultAffecting = false) => {
    setDraft((current) => current ? { ...current, ...change } : current)
    if (resultAffecting) invalidatePreview()
  }
  const patchSource = (source: CollectionProfileDraft["source"]) => {
    setDraft((current) => {
      if (!current) return current
      const supported = source.kind === "mdbListPublicList"
        || current.mediaType !== "mixed" && (source.kind !== "tmdbCollection" || current.mediaType === "movie")
      return {
        ...current,
        source,
        mediaType: supported ? current.mediaType : "movie",
      }
    })
    invalidatePreview()
  }
  const patchDiscoverParameter = (key: "language" | "region", value: string) => {
    if (draft.source.kind !== "tmdbDiscover") return
    const parameters = { ...draft.source.parameters }
    const normalized = value.trim()
    if (normalized) parameters[key] = normalized
    else delete parameters[key]
    patchSource({ ...draft.source, parameters })
  }
  const runPreview = async () => {
    const generation = previewRequest.current.generation + 1
    const controller = new AbortController()
    previewRequest.current.controller?.abort()
    previewRequest.current = { controller, generation }
    setPreview(null)
    setPreviewError(null)
    setPreviewing(true)
    try {
      const result = await api.collections.preview(structuredClone(draft), controller.signal)
      if (previewRequest.current.generation !== generation) return null
      const previewedDraft = result.sourceIdentity && draft.source.kind === "mdbListPublicList"
        ? { ...draft, source: { ...draft.source, listId: result.sourceIdentity } }
        : draft
      if (previewedDraft !== draft) setDraft(previewedDraft)
      setPreview(result)
      return previewedDraft
    } catch (error) {
      const failure = error instanceof Error ? error : new Error("Preview failed")
      if (previewRequest.current.generation !== generation || failure.name === "AbortError") return null
      setPreviewError(failure.message)
      toast.error(failure.message)
      return null
    } finally {
      if (previewRequest.current.generation === generation) {
        previewRequest.current.controller = null
        setPreviewing(false)
      }
    }
  }
  const cancelPoster = () => {
    posterRequest.current?.abort()
    posterRequest.current = null
    setUploading(false)
    setPosterError(null)
    setPosterName("")
    if (posterInput.current) posterInput.current.value = ""
  }
  const uploadPoster = async (file: File) => {
    cancelPoster()
    setPosterName(file.name)
    if (file.size === 0 || file.size > 10 * 1024 * 1024) {
      setPosterError("Choose a PNG, JPEG, or WebP image between 1 byte and 10 MB.")
      return
    }
    const controller = new AbortController()
    posterRequest.current = controller
    setUploading(true)
    try {
      const bytes = await file.arrayBuffer()
      if (controller.signal.aborted) return
      const result = await api.collections.uploadArtwork(bytes, controller.signal)
      if (!controller.signal.aborted) patch({ customPosterId: result.id })
    } catch (error) {
      if (!controller.signal.aborted) setPosterError(error instanceof Error ? error.message : "Could not upload poster.")
    } finally {
      if (posterRequest.current === controller) {
        posterRequest.current = null
        setUploading(false)
      }
    }
  }
  const reset = () => {
    cancelPoster()
    invalidatePreview()
    setDraft(structuredClone(initial))
    setPreview(null)
    setPreviewError(null)
  }
  const save = () => {
    if (uploading || posterError || readOnly || !validDraft || refreshesResults && !providerAvailable) return
    if (!profileId && !preview) {
      void runPreview()
      return
    }
    saveProfile.mutate(draft)
  }
  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) cancelPoster(); onOpenChange(next) }}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{profileId ? "Edit collection" : "Add collection"}</DialogTitle>
          <DialogDescription>Saving a new collection previews it first. Changing a result option clears the current preview.</DialogDescription>
        </DialogHeader>
        {!providerAvailable && <p className="rounded-md border p-3 text-sm text-muted-foreground">The provider is unavailable. Presentation and cadence changes can still be saved; result options are read-only.</p>}
        <div className="grid gap-4 md:grid-cols-2">
          <div className="flex flex-col gap-2">
            <Label htmlFor="collection-title">Title</Label>
            <Input id="collection-title" disabled={readOnly} value={draft.title} maxLength={120} onChange={(event) => patch({ title: event.target.value })} />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="collection-media">Media type</Label>
            <Select
              value={draft.mediaType}
              disabled={readOnly || !providerAvailable || draft.source.kind === "tmdbCollection"}
              onValueChange={(mediaType) => {
                if (mediaType === "movie" || mediaType === "series" || mediaType === "mixed") {
                  patch({ mediaType }, true)
                }
              }}
            >
              <SelectTrigger id="collection-media"><SelectValue /></SelectTrigger>
              <SelectContent>
                {mediaTypeChoices.map((choice) => (
                  <SelectItem key={choice.value} value={choice.value}>{choice.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-2 md:col-span-2">
            <Label htmlFor="collection-description">Description</Label>
            <Textarea id="collection-description" disabled={readOnly} className="min-h-20" value={draft.description} maxLength={2000} onChange={(event) => patch({ description: event.target.value })} />
          </div>
          {settings.data?.effectiveMode === "mediaFlick" && <div className="flex items-center justify-between gap-4 md:col-span-2">
            <div><Label htmlFor="collection-home">Available on Home</Label><p id="collection-home-help" className="mt-1 text-sm text-muted-foreground">Allow this collection to be selected as a Home shelf.</p></div>
            <Switch id="collection-home" aria-describedby="collection-home-help" checked={draft.availableOnHome} onCheckedChange={(availableOnHome) => patch({ availableOnHome })} disabled={readOnly} />
          </div>}
          {draft.source.kind === "tmdbDiscover" && draft.template.id.endsWith(".custom-discover") && (
            <>
              <div className="flex flex-col gap-2">
                <Label htmlFor="collection-language">Metadata language (optional)</Label>
                <Input
                  id="collection-language"
                  disabled={readOnly || !providerAvailable}
                  value={jsonString(draft.source.parameters.language) ?? ""}
                  onChange={(event) => patchDiscoverParameter("language", event.target.value)}
                  placeholder="en-US"
                />
              </div>
              <div className="flex flex-col gap-2">
                <Label htmlFor="collection-region">Region (optional)</Label>
                <Input
                  id="collection-region"
                  disabled={readOnly || !providerAvailable}
                  value={jsonString(draft.source.parameters.region) ?? ""}
                  onChange={(event) => patchDiscoverParameter("region", event.target.value)}
                  placeholder="US"
                />
              </div>
            </>
          )}
          {draft.source.kind === "mdbListPublicList" && (
            <div className="flex flex-col gap-2 md:col-span-2">
              <Label htmlFor="collection-list">MDBList public list ID or canonical URL</Label>
              <Input id="collection-list" disabled={readOnly || !providerAvailable} value={draft.source.listId} onChange={(event) => patchSource({ kind: "mdbListPublicList", listId: event.target.value })} />
              <PublicListCombobox account={account} value={draft.source.listId} disabled={readOnly || !providerAvailable} onValueChange={(listId) => patchSource({ kind: "mdbListPublicList", listId })} />
            </div>
          )}
          {draft.source.kind === "tmdbCollection" && (
            <div className="flex items-center justify-between gap-4 md:col-span-2">
              <div><Label htmlFor="collection-unreleased">Include unreleased titles</Label><p id="collection-unreleased-help" className="mt-1 text-sm text-muted-foreground">Applies only to this exact collection.</p></div>
              <Switch id="collection-unreleased" aria-describedby="collection-unreleased-help" checked={draft.source.includeUnreleased} onCheckedChange={(includeUnreleased) => {
                if (draft.source.kind !== "tmdbCollection") return
                patchSource({ kind: "tmdbCollection", collectionId: draft.source.collectionId, includeUnreleased })
              }} disabled={readOnly || !providerAvailable} />
            </div>
          )}
          <div className="flex flex-col gap-2">
            <Label htmlFor="collection-limit">Result limit</Label>
            <Select value={draft.limit.kind} disabled={readOnly || !providerAvailable} onValueChange={(kind) => patch({ limit: kind === "all" ? { kind: "all" } : { kind: "maximum", count: 100 } }, true)}>
              <SelectTrigger id="collection-limit"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="all">All</SelectItem><SelectItem value="maximum">Maximum</SelectItem></SelectContent>
            </Select>
            {draft.limit.kind === "maximum" && <div className="space-y-2"><Label htmlFor="collection-maximum">Maximum results</Label><SettingsNumberField id="collection-maximum" label="Maximum results" disabled={readOnly || !providerAvailable} min={1} max={500} value={draft.limit.count} onValueChange={(count) => patch({ limit: { kind: "maximum", count } }, true)} /></div>}
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="collection-cadence">Refresh cadence</Label>
            <Select value={draft.cadence} disabled={readOnly} onValueChange={(cadence) => {
              if (cadence === "manual" || cadence === "daily" || cadence === "weekly" || cadence === "monthly") {
                patch({ cadence })
              }
            }}>
              <SelectTrigger id="collection-cadence"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="manual">Manual</SelectItem><SelectItem value="daily">Daily</SelectItem><SelectItem value="weekly">Weekly</SelectItem><SelectItem value="monthly">Monthly</SelectItem></SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-2 md:col-span-2">
            <Label htmlFor="collection-poster-button">Custom poster (optional)</Label>
            <p id="collection-poster-help" className="text-sm text-muted-foreground">PNG, JPEG, or WebP, up to 10 MB. The poster is applied when you save.</p>
            <div className="flex flex-wrap items-center gap-2">
              <Input ref={posterInput} id="collection-poster" className="hidden" disabled={readOnly || busy} type="file" accept="image/png,image/jpeg,image/webp" aria-label="Custom poster file" onChange={(event) => {
                const file = event.target.files?.[0]
                if (file) void uploadPoster(file)
              }} />
              <Button id="collection-poster-button" type="button" variant="outline" aria-describedby="collection-poster-help collection-poster-status" disabled={readOnly || busy} onClick={() => posterInput.current?.click()}><ImagePlus />{draft.customPosterId ? "Replace poster" : "Choose poster"}</Button>
              {(uploading || posterError) && <Button type="button" variant="outline" onClick={cancelPoster}>{uploading ? "Cancel upload" : "Discard failed upload"}</Button>}
              {draft.customPosterId && <Button type="button" variant="outline" disabled={readOnly || busy} onClick={() => { cancelPoster(); patch({ customPosterId: null }) }}>Remove poster</Button>}
            </div>
            <p id="collection-poster-status" className="text-sm text-muted-foreground" role="status">{uploading ? `Uploading ${posterName}…` : posterError ? "Poster upload failed." : posterName || (draft.customPosterId ? "Custom poster selected" : "No custom poster selected")}</p>
            {posterError && <p className="text-sm text-destructive" role="alert">{posterError}</p>}
            {draft.customPosterId && <img className="h-32 w-22 rounded object-cover" src={api.collections.artworkUrl(draft.customPosterId)} alt="Selected collection poster" />}
          </div>
        </div>
        <div className="rounded-lg border p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div><strong>Preview</strong>{preview && <p className="text-sm text-muted-foreground">{preview.total} total · {preview.movies} movies · {preview.series} series</p>}</div>
            <Button variant="outline" disabled={readOnly || !providerAvailable || busy || !validDraft} onClick={() => void runPreview()}>{previewing ? "Previewing…" : preview ? "Preview again" : "Preview"}</Button>
          </div>
          {previewError && <p className="mt-3 text-sm text-destructive" role="alert">Preview failed: {previewError}</p>}
          {preview && (
            <ol className="mt-3 grid gap-2 text-sm sm:grid-cols-2">
              {preview.items.slice(0, 24).map((item) => {
                const poster = api.collections.providerArtworkUrl(item.posterPath, "w342")
                return (
                  <li key={`${item.mediaType}:${item.tmdbId}`} className="flex min-w-0 items-center gap-3 rounded-md bg-muted/40 p-2">
                    <img
                      src={poster ?? collectionTemplateArt}
                      alt=""
                      loading="lazy"
                      className="h-16 w-11 shrink-0 rounded object-cover"
                      onError={(event) => {
                        if (!poster || event.currentTarget.dataset.fallback === "true") return
                        event.currentTarget.dataset.fallback = "true"
                        event.currentTarget.src = collectionTemplateArt
                      }}
                    />
                    <span className="min-w-0 truncate text-muted-foreground">{item.title}{item.year ? ` (${item.year})` : ""}</span>
                  </li>
                )
              })}
            </ol>
          )}
        </div>
        <SaveBar
          dirty={uploading || Boolean(posterError) || !profileId || JSON.stringify(draft) !== JSON.stringify(initial)}
          saving={busy}
          saveDisabled={uploading || Boolean(posterError) || readOnly || !validDraft || (refreshesResults && !providerAvailable)}
          onSave={save}
          onDiscard={() => { cancelPoster(); onOpenChange(false) }}
          onReset={reset}
        />
      </DialogContent>
    </Dialog>
  )
}

export default function CollectionSettingsPage() {
  const cache = useQueryClient()
  const statusQuery = useStatus()
  const { data: status } = statusQuery
  const account = accountKey(status)
  const settings = useCollectionSettings(Boolean(status?.authenticated))
  const templates = useCollectionTemplates(Boolean(status?.authenticated))
  const profiles = useCollectionProfiles(Boolean(status?.authenticated))
  const [params, setParams] = useSearchParams()
  const [selection, setSelection] = useState<{
    draft: CollectionProfileDraft
    profileId: string | null
  } | null>(null)
  const savedDraft = useMemo<CollectionSettingsDraft | null>(() => settings.data && profiles.data ? ({
    modeSelection: settings.data.modeSelection ?? settings.data.effectiveMode,
    includeUnreleased: settings.data.franchises.includeUnreleased,
    profileIds: profiles.data.profiles.map((profile) => profile.id),
  }) : null, [profiles.data, settings.data])
  const [draft, setDraft, , acceptSaved] = useSourceDraft(savedDraft, account)
  const save = useMutation({
    mutationFn: async (value: CollectionSettingsDraft) => {
      const nextSettings = await api.collections.patchSettings({
        modeSelection: value.modeSelection,
        includeUnreleased: value.includeUnreleased,
      })
      const nextProfiles = savedDraft && JSON.stringify(value.profileIds) !== JSON.stringify(savedDraft.profileIds)
        ? await api.collections.reorderProfiles(value.profileIds)
        : profiles.data
      return { settings: nextSettings, profiles: nextProfiles }
    },
    onSuccess: ({ settings: nextSettings, profiles: nextProfiles }, submitted) => {
      acceptSaved({
        modeSelection: nextSettings.modeSelection ?? nextSettings.effectiveMode,
        includeUnreleased: nextSettings.franchises.includeUnreleased,
        profileIds: nextProfiles?.profiles.map((profile) => profile.id) ?? submitted.profileIds,
      }, submitted)
      cache.setQueryData(queryKeys.collectionSettings(account), nextSettings)
      if (nextProfiles) cache.setQueryData(queryKeys.collectionProfiles(account), nextProfiles)
      void cache.invalidateQueries({ queryKey: queryKeys.homeSettings })
      void cache.invalidateQueries({ queryKey: queryKeys.home })
      toast.success("Collection settings saved")
    },
    onError: (error: Error) => toast.error(error.message),
  })

  useEffect(() => {
    if (!status?.authenticated) return
    let cancelled = false
    void api.collections.settings(true).then(
      (next) => {
        if (cancelled) return
        cache.setQueryData(queryKeys.collectionSettings(account), next)
        void cache.invalidateQueries({ queryKey: queryKeys.companion })
        void cache.invalidateQueries({ queryKey: queryKeys.collectionTemplates(account) })
      },
      () => undefined,
    )
    return () => {
      cancelled = true
    }
  }, [account, cache, status?.authenticated])

  const requestedEdit = params.get("edit")
  const requestedProfile = requestedEdit
    ? profiles.data?.profiles.find((candidate) => candidate.id === requestedEdit)
    : null
  const active = selection ?? (requestedProfile ? {
    draft: draftFromProfile(requestedProfile),
    profileId: requestedProfile.id,
  } : null)

  if (statusQuery.error && !status) {
    return (
      <SettingsError
        title="Collections unavailable"
        error={statusQuery.error}
        onRetry={() => void statusQuery.refetch()}
      />
    )
  }
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="Collections" />
  const edit = (profile: CollectionProfile) => {
    setSelection({ draft: draftFromProfile(profile), profileId: profile.id })
  }
  const add = (template: CollectionTemplate) => {
    setSelection({ draft: draftFromTemplate(template), profileId: null })
  }
  const setWizardOpen = (next: boolean) => {
    if (next) return
    setSelection(null)
    if (requestedEdit) setParams({}, { replace: true })
  }
  return (
    <div className="settings-page">
      <PageTitle title="Collections" />
      <GeneralSettings draft={draft} onChange={setDraft} />
      <ConfiguredProfiles
        profileIds={draft?.profileIds ?? []}
        onProfileIdsChange={(profileIds) => { if (draft) setDraft({ ...draft, profileIds }) }}
        onEdit={edit}
      />
      {templates.data ? (
        <TemplateCatalog catalog={templates.data} disabled={!settings.data} onAdd={add} />
      ) : templates.isError ? (
        <Section title="Template catalog" description="Templates are starting points for My Collections.">
          <p className="text-sm text-destructive" role="alert">The template catalog could not be loaded.</p>
          <Button variant="outline" onClick={() => void templates.refetch()}>Try again</Button>
        </Section>
      ) : (
        <Section title="Template catalog" description="Loading available templates…">
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Layers className="size-4" /> Loading templates…
          </div>
        </Section>
      )}
      <SaveBar
        dirty={Boolean(draft && savedDraft && JSON.stringify(draft) !== JSON.stringify(savedDraft))}
        saving={save.isPending}
        onSave={() => { if (draft) save.mutate(draft) }}
        onDiscard={() => setDraft(savedDraft)}
        onReset={() => { if (draft && savedDraft && settings.data) setDraft({
          ...savedDraft,
          modeSelection: settings.data.mediaFlickAvailable ? "mediaFlick" : "jellyfin",
          includeUnreleased: false,
        }) }}
      />
      {active && (
        <CollectionWizard
          key={active.profileId ?? active.draft.template.id}
          initial={active.draft}
          profileId={active.profileId}
          open
          onOpenChange={setWizardOpen}
        />
      )}
    </div>
  )
}
