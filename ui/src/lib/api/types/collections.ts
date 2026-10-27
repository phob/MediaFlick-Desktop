import type * as Wire from "../wire.gen.ts"

export type CollectionCategory = Wire.TemplateCategory

export type CollectionTemplatePictogram = Wire.TemplatePictogram

export type CollectionSettings = Wire.CollectionSettings

export type CollectionProfile = Wire.CollectionProfile

export type CollectionProfileDraft = Wire.CollectionProfileDraft

export type CollectionTemplate = Wire.CollectionTemplate

export type CollectionTemplates = Wire.CollectionTemplates

export type NormalizedCollectionTitle = Wire.NormalizedTitle

export type ClassifiedCollectionTitle = Wire.ClassifiedTitle

export interface PublicCollectionList {
  id: string
  name: string
  owner: string | null
}

export type FranchiseCollectionSummary = Wire.FranchiseCollectionSummary

export type JellyfinCollectionSummary = Wire.JellyfinCollectionSummary
