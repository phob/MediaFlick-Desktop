import type * as Wire from "../wire.gen.ts"

export type BookItemDetail = Extract<Wire.DetailView, { layout: "book" }>

export type ReadingState = Wire.ReadingState

export type BookManifest = Wire.BookManifest

export type BookFormat = Wire.BookFormat

export type ReadingLocation = Wire.ReadingLocation

export type BookPositionWrite = Wire.BookPositionWrite

export type ReaderAppearance = Wire.ReaderAppearance
