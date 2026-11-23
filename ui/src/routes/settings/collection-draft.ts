import { ApiError } from "../../lib/api/client.ts"
import type {
  CollectionProfile,
  CollectionProfileDraft,
  CollectionProfilesIndex,
  CollectionSettings,
  CollectionSettingsPatch,
  ProfileDeleted,
} from "../../lib/api/types/collections.ts"
import { sameJson } from "../../lib/json.ts"

export type CollectionSettingsDraft = {
  modeSelection: CollectionSettings["effectiveMode"]
  includeUnreleased: boolean
  profileIds: string[]
  deletedProfileIds: string[]
}

export type CollectionSavePlan = {
  reorderIds: string[] | null
  deletions: string[]
  settingsPatch: CollectionSettingsPatch | null
}

export type CollectionSaveActions = {
  reorderProfiles: (profileIds: string[]) => Promise<CollectionProfilesIndex>
  deleteProfile: (profileId: string) => Promise<ProfileDeleted>
  patchSettings: (patch: CollectionSettingsPatch) => Promise<CollectionSettings>
  fetchSettings: () => Promise<CollectionSettings>
  fetchProfiles: () => Promise<CollectionProfilesIndex>
  isCurrentAccount?: () => boolean
}

type CollectionSaveProgress = {
  settings: CollectionSettings | undefined
  profiles: CollectionProfilesIndex | undefined
  committedDeletions: string[]
  committedReorder: boolean
  committedSettings: boolean
}

export type CollectionSaveResult =
  | (CollectionSaveProgress & { ok: true })
  | (CollectionSaveProgress & { ok: false; error: Error })

export function savedCollectionDraft(
  settings: CollectionSettings,
  profiles: CollectionProfilesIndex,
): CollectionSettingsDraft {
  return {
    modeSelection: settings.modeSelection ?? settings.effectiveMode,
    includeUnreleased: settings.franchises.includeUnreleased,
    profileIds: profiles.profiles.map((profile) => profile.id),
    deletedProfileIds: [],
  }
}

export function buildCollectionSettingsPatch(
  value: CollectionSettingsDraft,
  baseline: CollectionSettingsDraft,
): CollectionSettingsPatch | null {
  const patch: CollectionSettingsPatch = {}
  if (value.modeSelection !== baseline.modeSelection) patch.modeSelection = value.modeSelection
  if (value.includeUnreleased !== baseline.includeUnreleased) patch.includeUnreleased = value.includeUnreleased
  return Object.keys(patch).length ? patch : null
}

export function reconcileProfileIds(
  previous: readonly string[],
  draft: readonly string[],
  server: readonly string[],
): string[] {
  if (sameJson(draft, previous)) return [...server]
  const serverSet = new Set(server)
  const seen = new Set<string>()
  const rebased: string[] = []
  for (const id of draft) {
    if (serverSet.has(id) && !seen.has(id)) {
      seen.add(id)
      rebased.push(id)
    }
  }
  for (const id of server) {
    if (!seen.has(id)) {
      seen.add(id)
      rebased.push(id)
    }
  }
  return rebased
}

export function reconcileDeletedProfileIds(
  draft: readonly string[],
  server: readonly string[],
): string[] {
  const serverSet = new Set(server)
  const seen = new Set<string>()
  const kept: string[] = []
  for (const id of draft) {
    if (serverSet.has(id) && !seen.has(id)) {
      seen.add(id)
      kept.push(id)
    }
  }
  return kept
}

export function reconcileCollectionDraft(
  previous: CollectionSettingsDraft | null,
  draft: CollectionSettingsDraft | null,
  source: CollectionSettingsDraft | null,
): CollectionSettingsDraft | null {
  if (previous == null || draft == null || source == null) return source
  if (sameJson(previous, draft)) return source
  return {
    modeSelection: draft.modeSelection !== previous.modeSelection
      ? draft.modeSelection
      : source.modeSelection,
    includeUnreleased: draft.includeUnreleased !== previous.includeUnreleased
      ? draft.includeUnreleased
      : source.includeUnreleased,
    profileIds: reconcileProfileIds(previous.profileIds, draft.profileIds, source.profileIds),
    deletedProfileIds: reconcileDeletedProfileIds(draft.deletedProfileIds, source.profileIds),
  }
}

export function planCollectionSave(
  value: CollectionSettingsDraft,
  baseline: CollectionSettingsDraft,
): CollectionSavePlan {
  const serverIds = baseline.profileIds
  const deletions = reconcileDeletedProfileIds(value.deletedProfileIds, serverIds)
  const deletedSet = new Set(deletions)
  const fullOrder = reconcileProfileIds(serverIds, value.profileIds, serverIds)
  const kept = (ids: readonly string[]) => ids.filter((id) => !deletedSet.has(id))
  const reorderIds = !sameJson(kept(fullOrder), kept(serverIds)) ? fullOrder : null
  const settingsPatch = buildCollectionSettingsPatch(value, baseline)
  return { reorderIds, deletions, settingsPatch }
}

export function removeProfilesFromIndex(
  index: CollectionProfilesIndex,
  deletedIds: readonly string[],
): CollectionProfilesIndex {
  if (!deletedIds.length) return index
  const deleted = new Set(deletedIds)
  const errors = Object.fromEntries(
    Object.entries(index.errors).filter(([id]) => !deleted.has(id)),
  )
  return {
    profiles: index.profiles.filter((profile) => !deleted.has(profile.id)),
    errors,
  }
}

export function applyPreviewSourceIdentity(
  current: CollectionProfileDraft | null,
  requestedSource: CollectionProfileDraft["source"],
  sourceIdentity: string | null,
): CollectionProfileDraft | null {
  if (
    !current
    || !sourceIdentity
    || requestedSource.kind !== "mdbListPublicList"
    || current.source.kind !== "mdbListPublicList"
    || current.source.listId !== requestedSource.listId
    || current.source.listId === sourceIdentity
  ) {
    return current
  }
  return {
    ...current,
    source: { ...current.source, listId: sourceIdentity },
  }
}

export function upsertProfileInIndex(
  index: CollectionProfilesIndex,
  profile: CollectionProfile,
): CollectionProfilesIndex {
  const exists = index.profiles.some((candidate) => candidate.id === profile.id)
  return {
    ...index,
    profiles: exists
      ? index.profiles.map((candidate) => candidate.id === profile.id ? profile : candidate)
      : [...index.profiles, profile],
  }
}

function assertCurrentAccount(actions: CollectionSaveActions) {
  if (actions.isCurrentAccount && !actions.isCurrentAccount()) {
    throw new ApiError("The signed-in account changed. Save these settings again.", 409, { accountChanged: true })
  }
}

export async function executeCollectionSave(
  value: CollectionSettingsDraft,
  baseline: CollectionSettingsDraft,
  currentSettings: CollectionSettings | undefined,
  currentProfiles: CollectionProfilesIndex | undefined,
  actions: CollectionSaveActions,
): Promise<CollectionSaveResult> {
  const plan = planCollectionSave(value, baseline)
  let nextSettings = currentSettings
  let nextProfiles = currentProfiles
  const committedDeletions: string[] = []
  let committedReorder = false
  let committedSettings = false

  try {
    if (plan.reorderIds) {
      assertCurrentAccount(actions)
      nextProfiles = await actions.reorderProfiles(plan.reorderIds)
      committedReorder = true
    }
    for (const id of plan.deletions) {
      assertCurrentAccount(actions)
      await actions.deleteProfile(id)
      committedDeletions.push(id)
      if (nextProfiles) nextProfiles = removeProfilesFromIndex(nextProfiles, [id])
    }
    if (plan.settingsPatch) {
      assertCurrentAccount(actions)
      nextSettings = await actions.patchSettings(plan.settingsPatch)
      committedSettings = true
    } else if (committedDeletions.length) {
      assertCurrentAccount(actions)
      nextSettings = await actions.fetchSettings()
    }
    if (committedDeletions.length) {
      assertCurrentAccount(actions)
      nextProfiles = await actions.fetchProfiles()
    }
    assertCurrentAccount(actions)
    return {
      ok: true,
      settings: nextSettings,
      profiles: nextProfiles,
      committedDeletions,
      committedReorder,
      committedSettings,
    }
  } catch (caught) {
    const error = caught instanceof Error ? caught : new Error("Collection settings could not be saved")
    return {
      ok: false,
      error,
      settings: nextSettings,
      profiles: nextProfiles,
      committedDeletions,
      committedReorder,
      committedSettings,
    }
  }
}
