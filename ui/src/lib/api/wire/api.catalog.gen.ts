// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemAbout, ItemSynopsis } from "./api.catalog.detail.gen.ts"
import type { Billboard, HomeResponse, HomeResumeResponse } from "./api.catalog.home.gen.ts"
import type { HomeSettingsResponse } from "./api.catalog.home_settings.gen.ts"
import type { PersonResolution } from "./api.catalog.person.gen.ts"
import type { ItemPage } from "./library.gen.ts"
import type { DetailView } from "./library.model.gen.ts"
import type { HomeSettings } from "./preferences.accounts.gen.ts"

export type Genres = {
  genres: Array<string>,
};

export type ItemLetters = {
  letters: { [key in string]: number },
};

export type OfficialRatings = {
  officialRatings: Array<string>,
};

export type ApiCatalogRoutes = {
  "GET /api/calendar": { params: null, body: null, kind: "relay" },
  "GET /api/settings/home": { params: null, body: null, kind: "json", response: HomeSettingsResponse },
  "PATCH /api/settings/home": { params: null, body: HomeSettings, kind: "json", response: HomeSettingsResponse },
  "GET /api/home/resume": { params: null, body: null, kind: "json", response: HomeResumeResponse },
  "GET /api/home": { params: null, body: null, kind: "json", response: HomeResponse },
  "GET /api/billboard": { params: null, body: null, kind: "json", response: Billboard },
  "GET /api/items": { params: null, body: null, kind: "json", response: ItemPage },
  "GET /api/items/letters": { params: null, body: null, kind: "json", response: ItemLetters },
  "GET /api/genres": { params: null, body: null, kind: "json", response: Genres },
  "GET /api/official-ratings": { params: null, body: null, kind: "json", response: OfficialRatings },
  "GET /api/person/resolve": { params: null, body: null, kind: "json", response: PersonResolution },
  "GET /api/item/{id}": { params: { id: string | number }, body: null, kind: "json", response: DetailView },
  "GET /api/item/{id}/synopsis": { params: { id: string | number }, body: null, kind: "json", response: ItemSynopsis },
  "GET /api/item/{id}/about": { params: { id: string | number }, body: null, kind: "json", response: ItemAbout },
  "GET /api/item/{id}/children": { params: { id: string | number }, body: null, kind: "json", response: ItemPage },
};
