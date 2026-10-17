// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { AppStatus } from "./api.gen.ts"
import type { CollectionProfileDetail, CollectionTitle, FranchiseCollection, FranchiseCollectionsIndex, MovieCollection } from "./api.collections.browse.gen.ts"
import type { CollectionProfileDraft, CollectionProfilesIndex, CollectionSettings, CollectionSettingsPatch, CollectionTemplates, CommittedProfile, ProfileDeleted, ProfileEdit, ProfileOrderBody } from "./api.collections.profiles.gen.ts"
import type { CollectionProfile, ProviderResult } from "./collections.model.gen.ts"
import type { ItemSummary } from "./library.model.gen.ts"

export type JellyfinCollectionDetail = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  items: Array<ItemSummary>,
  totalRecordCount: number,
};

export type JellyfinCollectionSummary = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  itemCount: number | null,
};

export type JellyfinCollections = {
  collections: Array<JellyfinCollectionSummary>,
};

export type LocalAccountDeletionBody = {
  confirmed: boolean,
};

export type PublicListSearchBody = {
  query: string,
};

export type PublicListValidateBody = {
  selector: string,
};

export type UploadedArtwork = {
  id: string,
};

export type ApiCollectionsRoutes = {
  "GET /api/collections/settings": { params: null, body: null, kind: "json", response: CollectionSettings },
  "POST /api/collections/settings/reprobe": { params: null, body: null, kind: "json", response: CollectionSettings },
  "PATCH /api/collections/settings": { params: null, body: CollectionSettingsPatch, kind: "json", response: CollectionSettings },
  "GET /api/collections/templates": { params: null, body: null, kind: "json", response: CollectionTemplates },
  "POST /api/collections/mdblist/search": { params: null, body: PublicListSearchBody, kind: "relay" },
  "POST /api/collections/mdblist/validate": { params: null, body: PublicListValidateBody, kind: "relay" },
  "GET /api/collections/provider-artwork": { params: null, body: null, kind: "binary" },
  "POST /api/collections/artwork": { params: null, body: ArrayBuffer, kind: "json", response: UploadedArtwork },
  "GET /api/collections/artwork/{artworkId}": { params: { artworkId: string | number }, body: null, kind: "binary" },
  "POST /api/collections/preview": { params: null, body: CollectionProfileDraft, kind: "json", response: ProviderResult },
  "GET /api/collections/profiles": { params: null, body: null, kind: "json", response: CollectionProfilesIndex },
  "POST /api/collections/profiles": { params: null, body: CollectionProfileDraft, kind: "json", response: CommittedProfile },
  "PUT /api/collections/profiles/order": { params: null, body: ProfileOrderBody, kind: "json", response: CollectionProfilesIndex },
  "GET /api/collections/profiles/{profileId}": { params: { profileId: string | number }, body: null, kind: "json", response: CollectionProfile },
  "PATCH /api/collections/profiles/{profileId}": { params: { profileId: string | number }, body: CollectionProfileDraft, kind: "json", response: ProfileEdit },
  "DELETE /api/collections/profiles/{profileId}": { params: { profileId: string | number }, body: null, kind: "json", response: ProfileDeleted },
  "POST /api/collections/profiles/{profileId}/refresh": { params: { profileId: string | number }, body: null, kind: "json", response: CommittedProfile },
  "GET /api/collections": { params: null, body: null, kind: "json", response: CollectionSettings },
  "GET /api/collections/mine": { params: null, body: null, kind: "json", response: CollectionProfilesIndex },
  "GET /api/collections/mine/{profileId}": { params: { profileId: string | number }, body: null, kind: "json", response: CollectionProfileDetail },
  "GET /api/collections/franchises": { params: null, body: null, kind: "json", response: FranchiseCollectionsIndex },
  "GET /api/collections/franchises/{collectionId}": { params: { collectionId: string | number }, body: null, kind: "json", response: FranchiseCollection },
  "GET /api/collections/jellyfin": { params: null, body: null, kind: "json", response: JellyfinCollections },
  "GET /api/collections/jellyfin/{boxsetId}": { params: { boxsetId: string | number }, body: null, kind: "json", response: JellyfinCollectionDetail },
  "DELETE /api/collections/local-account": { params: null, body: LocalAccountDeletionBody, kind: "json", response: AppStatus },
  "GET /api/collections/movie/{tmdbId}": { params: { tmdbId: string | number }, body: null, kind: "json", response: MovieCollection },
  "GET /api/collections/title/{mediaType}/{tmdbId}": { params: { mediaType: string | number, tmdbId: string | number }, body: null, kind: "json", response: CollectionTitle },
};
