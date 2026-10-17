// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type * from "./wire/api.gen.ts"
export type * from "./wire/api.auth.gen.ts"
export type * from "./wire/api.catalog.gen.ts"
export type * from "./wire/api.catalog.detail.gen.ts"
export type * from "./wire/api.catalog.home.gen.ts"
export type * from "./wire/api.catalog.home_settings.gen.ts"
export type * from "./wire/api.catalog.person.gen.ts"
export type * from "./wire/api.collections.gen.ts"
export type * from "./wire/api.collections.browse.gen.ts"
export type * from "./wire/api.collections.profiles.gen.ts"
export type * from "./wire/api.images.gen.ts"
export type * from "./wire/api.letterboxd.gen.ts"
export type * from "./wire/api.libraries.gen.ts"
export type * from "./wire/api.media.gen.ts"
export type * from "./wire/api.playback.gen.ts"
export type * from "./wire/api.ratings.gen.ts"
export type * from "./wire/api.seerr.gen.ts"
export type * from "./wire/api.settings.gen.ts"
export type * from "./wire/api.shell.gen.ts"
export type * from "./wire/api.wire.gen.ts"
export type * from "./wire/collections.model.gen.ts"
export type * from "./wire/collections.snapshots.gen.ts"
export type * from "./wire/collections.templates.gen.ts"
export type * from "./wire/companion.gen.ts"
export type * from "./wire/integrations.letterboxd.gen.ts"
export type * from "./wire/integrations.mdblist.gen.ts"
export type * from "./wire/integrations.mdblist.cache.gen.ts"
export type * from "./wire/integrations.mdblist.schema.gen.ts"
export type * from "./wire/jellyfin.session.gen.ts"
export type * from "./wire/library.gen.ts"
export type * from "./wire/library.browse.gen.ts"
export type * from "./wire/library.libraries.gen.ts"
export type * from "./wire/library.model.gen.ts"
export type * from "./wire/library.sync.gen.ts"
export type * from "./wire/maintenance.player_setup.gen.ts"
export type * from "./wire/maintenance.updater.gen.ts"
export type * from "./wire/playback.model.gen.ts"
export type * from "./wire/playback.segments.gen.ts"
export type * from "./wire/preferences.accounts.gen.ts"
export type * from "./wire/preferences.collections.gen.ts"
export type * from "./wire/preferences.model.gen.ts"
export type * from "./wire/preferences.service.gen.ts"
export type * from "./wire/preferences.viewing.gen.ts"
export type * from "./wire/ui_events.gen.ts"

import type { ApiRoutes } from "./wire/api.gen.ts"
import type { ApiSettingsRoutes } from "./wire/api.settings.gen.ts"
import type { ApiRatingsRoutes } from "./wire/api.ratings.gen.ts"
import type { ApiCollectionsRoutes } from "./wire/api.collections.gen.ts"
import type { ApiShellRoutes } from "./wire/api.shell.gen.ts"
import type { ApiLetterboxdRoutes } from "./wire/api.letterboxd.gen.ts"
import type { ApiAuthRoutes } from "./wire/api.auth.gen.ts"
import type { ApiSeerrRoutes } from "./wire/api.seerr.gen.ts"
import type { ApiCatalogRoutes } from "./wire/api.catalog.gen.ts"
import type { ApiLibrariesRoutes } from "./wire/api.libraries.gen.ts"
import type { ApiMediaRoutes } from "./wire/api.media.gen.ts"
import type { ApiImagesRoutes } from "./wire/api.images.gen.ts"
import type { ApiPlaybackRoutes } from "./wire/api.playback.gen.ts"

/** Every route the native app serves, by method and path. */
export type Routes =
  & ApiRoutes
  & ApiSettingsRoutes
  & ApiRatingsRoutes
  & ApiCollectionsRoutes
  & ApiShellRoutes
  & ApiLetterboxdRoutes
  & ApiAuthRoutes
  & ApiSeerrRoutes
  & ApiCatalogRoutes
  & ApiLibrariesRoutes
  & ApiMediaRoutes
  & ApiImagesRoutes
  & ApiPlaybackRoutes;
