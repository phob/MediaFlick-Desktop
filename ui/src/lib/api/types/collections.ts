import type * as Wire from "../wire.gen.ts"

export type CollectionMode = Wire.CollectionMode

export type CollectionCategory = Wire.TemplateCategory

export type CollectionTemplatePictogram = Wire.TemplatePictogram

export type CollectionSettings = Wire.CollectionSettings

export type CollectionProfile = Wire.CollectionProfile

export type CollectionProfileDraft = Omit<CollectionProfile, "id" | "revision">

export type CollectionTemplate = Wire.CollectionTemplate

export type CollectionTemplates = Wire.CollectionTemplates

export type NormalizedCollectionTitle = Wire.NormalizedTitle

export type ClassifiedCollectionTitle = Wire.ClassifiedTitle

export type CollectionPreview = Wire.ProviderResult

export interface PublicCollectionList {
  id: string
  name: string
  owner: string | null
}

export type CollectionProfilesIndex = Wire.CollectionProfilesIndex

export type CollectionProfileDetail = Wire.CollectionProfileDetail

export type FranchiseCollection = Wire.FranchiseCollection

export type FranchiseCollectionSummary = Wire.FranchiseCollectionSummary

export type FranchiseCollectionsIndex = Wire.FranchiseCollectionsIndex

export type JellyfinCollectionSummary = Wire.JellyfinCollectionSummary

export type JellyfinCollectionDetail = Wire.JellyfinCollectionDetail

export type MovieCollection = Wire.MovieCollection
