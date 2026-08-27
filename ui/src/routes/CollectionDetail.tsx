import { ArrowLeft, Layers } from "lucide-react"
import { useMemo, useState } from "react"
import { Link, useParams } from "react-router-dom"
import { toast } from "sonner"
import { CollectionTitleGrid } from "@/components/CollectionTitleGrid"
import { MediaCard } from "@/components/MediaCard"
import { PageErrorState } from "@/components/PageHeader"
import { DetailBackdrop } from "@/components/detail/DetailPrimitives"
import { SeerrCard } from "@/components/seerr/SeerrCard"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  api,
  imageUrl,
  type ClassifiedCollectionTitle,
  type ItemSummary,
  type NormalizedCollectionTitle,
  type SeerrCapabilities,
  type SeerrResult,
} from "@/lib/api"
import {
  useCollectionSettings,
  useFranchise,
  useJellyfinCollection,
  useMyCollection,
  useSeerrStatus,
} from "@/lib/queries"
import { useLocalDate } from "@/lib/local-date"

function CollectionShell({
  backTo,
  backHref,
  name,
  countLine,
  overview,
  backdrop,
  poster,
  status,
  onRetry,
  children,
}: {
  backTo: string
  backHref: string
  name: string
  countLine: string | null
  overview: string | null
  backdrop: string | null
  poster: string | null
  status?: string | null
  onRetry?: () => void
  children: React.ReactNode
}) {
  return (
    <div className="detail-page relative isolate flex min-h-full min-w-0 flex-col gap-8 pb-16">
      {backdrop && <DetailBackdrop src={backdrop} />}
      <header className="relative">
        <div className="relative z-10 flex items-end gap-6 px-6 pt-10 sm:px-10 lg:px-14">
          <div className="hidden h-40 w-27 shrink-0 overflow-hidden rounded-lg bg-card shadow-xl ring-1 ring-white/10 sm:block">
            {poster ? (
              <img src={poster} alt="" decoding="async" className="media-artwork-image h-full w-full object-cover" />
            ) : (
              <div className="flex h-full w-full items-center justify-center text-muted-foreground">
                <Layers className="size-7" aria-hidden />
              </div>
            )}
          </div>
          <div className="min-w-0 flex-1 pb-1">
            <Link
              to={backHref}
              className="inline-flex items-center gap-1 text-sm text-muted-foreground transition hover:text-foreground"
            >
              <ArrowLeft className="size-3.5" /> {backTo}
            </Link>
            <h1 className="mt-1 truncate text-2xl font-semibold tracking-tight">{name}</h1>
            {countLine && <p className="data-value mt-1 text-muted-foreground">{countLine}</p>}
            {status && <div className="mt-2 flex items-center gap-2 text-sm text-amber-300" role="status"><span>{status}</span>{onRetry && <Button size="sm" variant="outline" onClick={onRetry}>Retry</Button>}</div>}
          </div>
        </div>
        {overview && (
          <p className="relative z-10 mt-4 max-w-3xl px-6 text-sm leading-relaxed text-foreground/85 sm:px-10 lg:px-14">
            {overview}
          </p>
        )}
      </header>
      {children}
    </div>
  )
}

function TitleArtwork({ title }: { title: NormalizedCollectionTitle }) {
  const poster = api.collections.providerArtworkUrl(title.posterPath, "w342")
  return (
    <div className="relative h-poster-h w-poster-w overflow-hidden rounded-lg bg-card ring-1 ring-white/10">
      {poster ? (
        <img src={poster} alt="" loading="lazy" className="media-artwork-image h-full w-full object-cover" />
      ) : (
        <div className="flex h-full items-end bg-gradient-to-br from-slate-700 to-slate-950 p-3 text-sm font-medium">
          {title.title}
        </div>
      )}
      {title.mediaType === "series" && (
        <Badge className="absolute left-2 top-2" variant="secondary">Series</Badge>
      )}
    </div>
  )
}

function OwnedCard({
  title,
  item,
}: {
  title: ClassifiedCollectionTitle
  item: ItemSummary | undefined
}) {
  const primary = title.localItems[0]
  return (
    <div className="flex w-poster-w flex-col" data-collection-owned-card>
      {item ? (
        <MediaCard item={item} className="catalog-card" />
      ) : (
        <article className="catalog-card flex w-poster-w flex-col">
          {primary ? (
            <Link to={`/item/${encodeURIComponent(primary.id)}`}>
              <TitleArtwork title={title} />
            </Link>
          ) : (
            <TitleArtwork title={title} />
          )}
          <div className="min-w-0 pt-2">
            <div className="truncate text-sm font-medium">{title.title}</div>
            <div className="data-value text-muted-foreground">
              {title.year ?? "Year unknown"}
            </div>
          </div>
        </article>
      )}
      {title.localItems.length > 1 && (
        <details className="mt-1 text-xs">
          <summary className="cursor-pointer text-muted-foreground">Choose edition</summary>
          <div className="mt-1 flex flex-col gap-1">
            {title.localItems.map((edition) => (
              <Link
                key={edition.id}
                className="hover:underline"
                to={`/item/${encodeURIComponent(edition.id)}`}
              >
                {edition.name}
              </Link>
            ))}
          </div>
        </details>
      )}
    </div>
  )
}

function discoveryResult(title: NormalizedCollectionTitle): SeerrResult {
  return {
    mediaType: title.mediaType === "series" ? "tv" : "movie",
    tmdbId: title.tmdbId,
    title: title.title,
    year: title.year ?? null,
    overview: title.overview,
    posterPath: title.posterPath ?? null,
    backdropPath: title.backdropPath ?? null,
    voteAverage: null,
    status: "unknown",
    status4k: "unknown",
    libraryItemId: null,
  }
}

function MissingCard({
  title,
  requestsEnabled = true,
  capabilities,
}: {
  title: NormalizedCollectionTitle
  requestsEnabled?: boolean
  capabilities?: SeerrCapabilities | null
}) {
  if (requestsEnabled) {
    return <SeerrCard result={discoveryResult(title)} capabilities={capabilities} />
  }

  const artwork = <TitleArtwork title={title} />
  return (
    <article className="catalog-card flex w-poster-w flex-col">
      {artwork}
      <div className="min-w-0 pt-2">
        <div className="truncate text-sm font-medium">{title.title}</div>
        <div className="data-value text-muted-foreground">{title.year ?? "Year unknown"}</div>
      </div>
    </article>
  )
}

function TitleGrid({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-wrap gap-[var(--card-gap)]">{children}</div>
}

function SnapshotSections({
  owned,
  missing,
  items,
  libraryItems,
  ownershipAvailable,
}: {
  owned: ClassifiedCollectionTitle[]
  missing: NormalizedCollectionTitle[]
  items: NormalizedCollectionTitle[]
  libraryItems: ItemSummary[]
  ownershipAvailable: boolean
}) {
  const [expanded, setExpanded] = useState(false)
  const seerr = useSeerrStatus(ownershipAvailable && missing.length > 0)
  const visibleMissing = expanded ? missing : missing.slice(0, 24)
  const libraryById = useMemo(
    () => new Map(libraryItems.map((item) => [item.id, item])),
    [libraryItems],
  )
  if (!ownershipAvailable) {
    return (
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <div>
          <h2 className="section-title">Titles</h2>
          <p className="mt-1 text-sm text-muted-foreground">Ownership unavailable</p>
        </div>
        <CollectionTitleGrid
          items={items}
          itemKey={(title) => `${title.mediaType}:${title.tmdbId}`}
          renderItem={(title) => <MissingCard title={title} requestsEnabled={false} />}
        />
      </section>
    )
  }
  return (
    <>
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <h2 className="section-title">Owned</h2>
        {owned.length ? (
          <CollectionTitleGrid
            items={owned}
            itemKey={(title) => `${title.mediaType}:${title.tmdbId}`}
            renderItem={(title) => (
              <OwnedCard title={title} item={libraryById.get(title.localItems[0]?.id ?? "")} />
            )}
          />
        ) : <p className="text-sm text-muted-foreground">No matching titles are in your library.</p>}
      </section>
      {missing.length > 0 && (
        <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
          <h2 className="section-title">Missing</h2>
          <CollectionTitleGrid
            items={visibleMissing}
            itemKey={(title) => `${title.mediaType}:${title.tmdbId}`}
            renderItem={(title) => (
              <MissingCard title={title} capabilities={seerr.data?.capabilities} />
            )}
          />
          {missing.length >= 25 && (
            <Button className="self-start" variant="outline" onClick={() => setExpanded((value) => !value)}>
              {expanded ? "Show fewer" : `Show all ${missing.length}`}
            </Button>
          )}
        </section>
      )}
    </>
  )
}

function ErrorPage({ title, error, retry }: { title: string; error: Error; retry: () => void }) {
  return (
    <div className="p-6 sm:p-10 lg:p-14">
      <PageErrorState
        title={title}
        description={error.message}
        action={<Button variant="outline" onClick={() => void retry()}>Try again</Button>}
      />
    </div>
  )
}

function lastUpdated(timestamp: number | null | undefined) {
  return timestamp ? new Date(timestamp * 1000).toLocaleString() : null
}

export function MyCollectionDetail() {
  const { profileId } = useParams<{ profileId: string }>()
  const query = useMyCollection(profileId ?? null)
  const settings = useCollectionSettings()
  const [refreshing, setRefreshing] = useState(false)
  if (query.error && !query.data) return <ErrorPage title="Could not load collection" error={query.error} retry={() => { void query.refetch() }} />
  const data = query.data
  const profile = data?.profile
  const updated = lastUpdated(data?.refresh?.lastSuccess)
  const stale = data?.refresh?.latestFailure
    ? updated ? `Update failed. Last updated ${updated}.` : "Results unavailable. Update failed."
    : data?.overdue ? updated ? `Last updated ${updated}.` : "Results unavailable." : null
  const providerAvailable = profile?.source.kind === "mdbListPublicList"
    ? Boolean(settings.data?.readiness.mdblist)
    : Boolean(settings.data?.readiness.tmdb)
  const retry = profile && stale && providerAvailable ? () => {
    setRefreshing(true)
    void api.collections.refreshProfile(profile.id).then(
      () => void query.refetch(),
      (error: Error) => toast.error(error.message),
    ).finally(() => setRefreshing(false))
  } : undefined
  return (
    <CollectionShell
      backTo="My Collections"
      backHref="/collections/mine"
      name={profile?.title ?? "…"}
      countLine={data?.status === "updating" ? "Updating results" : data?.status === "resultsUnavailable" ? "Results unavailable" : data ? `${data.owned.length} owned${data.missing.length ? ` · ${data.missing.length} missing` : ""}` : null}
      overview={profile?.description ?? null}
      backdrop={null}
      poster={profile?.customPosterId ? `/api/collections/artwork/${profile.customPosterId}` : null}
      status={stale}
      onRetry={refreshing ? undefined : retry}
    >
      {data && <SnapshotSections owned={data.owned} missing={data.missing} items={data.items} libraryItems={data.libraryItems} ownershipAvailable={data.ownershipAvailable !== false} />}
    </CollectionShell>
  )
}

export function FranchiseCollectionDetail() {
  const { tmdbCollectionId } = useParams<{ tmdbCollectionId: string }>()
  const numericId = Number(tmdbCollectionId)
  const id = Number.isSafeInteger(numericId) && numericId > 0 ? numericId : null
  const date = useLocalDate()
  const query = useFranchise(id, date)
  if (query.error && !query.data) return <ErrorPage title="Could not load franchise" error={query.error} retry={() => { void query.refetch() }} />
  const data = query.data
  return (
    <CollectionShell
      backTo="Movie Franchises"
      backHref="/collections/franchises"
      name={data?.name ?? "…"}
      countLine={data?.ownershipAvailable === false ? "Ownership unavailable" : data ? `${data.owned.length} owned${data.missing.length ? ` · ${data.missing.length} missing` : ""}` : null}
      overview={null}
      backdrop={api.collections.providerArtworkUrl(data?.backdropPath, "w1280")}
      poster={api.collections.providerArtworkUrl(data?.posterPath, "w342")}
    >
      {data && <SnapshotSections owned={data.owned} missing={data.missing} items={data.items ?? [...data.owned, ...data.missing]} libraryItems={data.libraryItems} ownershipAvailable={data.ownershipAvailable !== false} />}
    </CollectionShell>
  )
}

export function JellyfinCollectionDetail() {
  const { boxSetId } = useParams<{ boxSetId: string }>()
  const query = useJellyfinCollection(boxSetId ?? null)
  if (query.error && !query.data) return <ErrorPage title="Could not load Jellyfin collection" error={query.error} retry={() => { void query.refetch() }} />
  const data = query.data
  return (
    <CollectionShell
      backTo="Jellyfin Collections"
      backHref="/collections/jellyfin"
      name={data?.name ?? "…"}
      countLine={data ? `${data.items.length} ${data.items.length === 1 ? "item" : "items"}` : null}
      overview={null}
      backdrop={data?.backdropImageTag ? imageUrl({ id: data.id, primaryImageTag: null }, "Backdrop", 1280, data.backdropImageTag) : null}
      poster={data?.primaryImageTag ? imageUrl({ id: data.id, primaryImageTag: data.primaryImageTag }, "Primary", 342) : null}
    >
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <h2 className="section-title">Items</h2>
        {data && (
          <TitleGrid>{data.items.map((item) => <MediaCard key={item.id} item={item} className="catalog-card" />)}</TitleGrid>
        )}
      </section>
    </CollectionShell>
  )
}

export default MyCollectionDetail
