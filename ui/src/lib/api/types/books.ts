import type * as Wire from "../wire.gen.ts"

export type BookItemDetail = Extract<Wire.DetailView, { layout: "book" }>

export type ReadingState = Wire.ReadingState
