import { Layers, MoreHorizontal, Pencil, RefreshCw } from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import { Link, Navigate, Outlet } from "react-router-dom"
import { toast } from "sonner"
import { PageEmptyState, PageErrorState, PageHeader } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Skeleton } from "@/components/ui/skeleton"
import {
  api,
  imageUrl,
  type CollectionProfile,
  type FranchiseCollectionSummary,
  type JellyfinCollectionSummary,
} from "@/lib/api"
import { queryClient } from "@/lib/query-client"
import { useLocalDate } from "@/lib/local-date"
import {
  collectionAccountKey,
  franchiseQueryOptions,
  myCollectionQueryOptions,
  useCollectionSettings,
  useFranchises,
  useJellyfinCollections,
  useMyCollections,
  useStatus,
} from "@/lib/queries"

function titleCardPalette(name: string) {
  let hash = 0
  for (const character of name) hash = (hash * 31 + character.charCodeAt(0)) >>> 0
  const hue = hash % 360
  return {
    backgroundImage: `linear-gradient(155deg, hsl(${hue} 45% 22%), hsl(${(hue + 40) % 360} 40% 14%) 70%)`,
  }
}

function CollectionArtwork({ name, poster }: { name: string; poster: string | null }) {
  return (
    <div
      className="relative flex h-poster-h w-poster-w flex-col justify-end overflow-hidden rounded-lg bg-card shadow-lg ring-1 ring-white/10 transition group-hover:ring-white/25"
      style={poster ? undefined : titleCardPalette(name)}
    >
      {poster ? (
        <img
          src={poster}
          alt=""
          loading="lazy"
          decoding="async"
          className="media-artwork-image h-full w-full object-cover transition group-hover:scale-[1.03]"
        />
      ) : (
        <>
          <Layers aria-hidden className="absolute -right-2 -top-2 size-20 opacity-10" />
          <span className="p-3 text-sm font-semibold leading-snug text-white/90">{name}</span>
        </>
      )}
    </div>
  )
}

function CardFrame({
  to,
  name,
  poster,
  detail,
  menu,
  onPrefetch,
}: {
  to: string
  name: string
  poster: string | null
  detail: string | null
  menu?: React.ReactNode
  onPrefetch?: () => void
}) {
  return (
    <article className="catalog-card group relative flex w-poster-w flex-col rounded-lg">
      <Link
        to={to}
        aria-label={`Open ${name}`}
        onPointerEnter={onPrefetch}
        onFocus={onPrefetch}
      >
        <CollectionArtwork name={name} poster={poster} />
        <div className="min-w-0 pt-2">
          <div className="truncate text-sm font-medium">{name}</div>
          {detail && <div className="data-value truncate text-muted-foreground">{detail}</div>}
        </div>
      </Link>
      {menu && <div className="absolute right-1 top-1">{menu}</div>}
    </article>
  )
}

function LoadingGrid() {
  return (
    <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
      {Array.from({ length: 8 }, (_, index) => (
        <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
      ))}
    </div>
  )
}

export function CollectionModeRoute({ mode }: { mode: "mediaFlick" | "jellyfin" }) {
  const settings = useCollectionSettings()
  if (!settings.data) return <LoadingGrid />
  if (settings.data.effectiveMode !== mode) return <Navigate to="/collections" replace />
  return <Outlet />
}

function CollectionPage({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-6 pb-16">
      <PageHeader eyebrow={eyebrow} title={title} description={description} />
      {children}
    </div>
  )
}

export default function Collections() {
  const settings = useCollectionSettings()
  if (settings.error) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState title="Could not open collections" description={settings.error.message} />
      </div>
    )
  }
  if (!settings.data) return <LoadingGrid />
  return (
    <Navigate
      to={settings.data.effectiveMode === "mediaFlick" ? "/collections/franchises" : "/collections/jellyfin"}
      replace
    />
  )
}

export function FranchiseCollections() {
  const date = useLocalDate()
  const query = useFranchises(date)
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = collectionAccountKey(status)
  const rows = query.data?.franchises
  return (
    <CollectionPage
      eyebrow="Collections"
      title="Movie Franchises"
      description="Movie series found from the exact TMDB collection identities in your library."
    >
      {query.error && !rows ? (
        <PageErrorState title="Could not find movie franchises" description={query.error.message} />
      ) : query.isPending || query.data?.status === "updating" ? (
        <>
          <p className="px-6 text-sm text-muted-foreground sm:px-10 lg:px-14">Finding movie franchises...</p>
          <LoadingGrid />
        </>
      ) : query.data?.status === "resultsUnavailable" ? (
        <PageErrorState
          title="Results unavailable"
          description="Movie franchises will retry when provider results are available."
        />
      ) : !rows?.length ? (
        <PageEmptyState
          icon={<Layers className="size-6" />}
          title="No movie franchises found."
          description="A franchise appears after at least one owned movie and one other visible movie are matched."
        />
      ) : (
        <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
          {rows.map((collection: FranchiseCollectionSummary) => (
            <CardFrame
              key={collection.collectionId}
              to={`/collections/franchises/${collection.collectionId}`}
              name={collection.name}
              poster={api.collections.providerArtworkUrl(collection.posterPath, "w342")}
              detail={collection.ownershipAvailable === false
                ? "Ownership unavailable"
                : `${collection.ownedCount} owned${collection.missingCount ? ` · ${collection.missingCount} missing` : ""}`}
              onPrefetch={() => {
                if (!status?.authenticated) return
                void cache.prefetchQuery(
                  franchiseQueryOptions(account, collection.collectionId, date),
                )
              }}
            />
          ))}
        </div>
      )}
    </CollectionPage>
  )
}

function ProfileMenu({
  profile,
  error,
  providerAvailable,
}: {
  profile: CollectionProfile
  error?: string
  providerAvailable: boolean
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button size="icon" variant="secondary" aria-label={`Actions for ${profile.title}`}>
          <MoreHorizontal />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {error ? (
          <DropdownMenuItem disabled><Pencil /> Edit</DropdownMenuItem>
        ) : (
          <DropdownMenuItem asChild>
            <Link to={`/settings/collections?edit=${encodeURIComponent(profile.id)}`}>
              <Pencil /> Edit
            </Link>
          </DropdownMenuItem>
        )}
        <DropdownMenuItem
          disabled={Boolean(error) || !providerAvailable}
          onSelect={() => {
            void api.collections.refreshProfile(profile.id).then(
              () => {
                void queryClient.invalidateQueries({ queryKey: ["collections"] })
                toast.success(`${profile.title} updated`)
              },
              (error: Error) => toast.error(error.message),
            )
          }}
        >
          <RefreshCw /> Check for updates
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function MyCollections() {
  const query = useMyCollections()
  const settings = useCollectionSettings()
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = collectionAccountKey(status)
  const profiles = query.data?.profiles
  return (
    <CollectionPage
      eyebrow="Collections"
      title="My Collections"
      description="Collections chosen and ordered for this account."
    >
      {query.error && !profiles ? (
        <PageErrorState title="Could not load your collections" description={query.error.message} />
      ) : query.isPending ? (
        <LoadingGrid />
      ) : !profiles?.length ? (
        <PageEmptyState
          icon={<Layers className="size-6" />}
          title="No collections yet"
          description="Choose a template, preview its titles, and save it for this account."
          action={<Button asChild><Link to="/settings/collections">Choose templates</Link></Button>}
        />
      ) : (
        <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
          {profiles.map((profile) => (
            <CardFrame
              key={profile.id}
              to={`/collections/mine/${profile.id}`}
              name={profile.title}
              poster={profile.customPosterId ? api.collections.artworkUrl(profile.customPosterId) : null}
              detail={profile.description || null}
              menu={<ProfileMenu
                profile={profile}
                error={query.data?.errors?.[profile.id]}
                providerAvailable={profile.source.kind === "mdbListPublicList"
                  ? Boolean(settings.data?.readiness.mdblist)
                  : Boolean(settings.data?.readiness.tmdb)}
              />}
              onPrefetch={() => {
                if (!status?.authenticated) return
                void cache.prefetchQuery(myCollectionQueryOptions(account, profile.id))
              }}
            />
          ))}
        </div>
      )}
    </CollectionPage>
  )
}

export function JellyfinCollections() {
  const query = useJellyfinCollections()
  const rows = query.data?.collections
  return (
    <CollectionPage
      eyebrow="Collections"
      title="Jellyfin Collections"
      description="BoxSets from your Jellyfin server, shown without importing or changing them."
    >
      {query.error && !rows ? (
        <PageErrorState title="Could not load Jellyfin collections" description={query.error.message} />
      ) : query.isPending ? (
        <LoadingGrid />
      ) : !rows?.length ? (
        <PageEmptyState
          icon={<Layers className="size-6" />}
          title="No Jellyfin collections found."
          description="BoxSets created on your server will appear here."
        />
      ) : (
        <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
          {rows.map((collection: JellyfinCollectionSummary) => (
            <CardFrame
              key={collection.id}
              to={`/collections/jellyfin/${encodeURIComponent(collection.id)}`}
              name={collection.name}
              poster={collection.primaryImageTag ? imageUrl({ id: collection.id, primaryImageTag: collection.primaryImageTag }, "Primary", 342) : null}
              detail={collection.itemCount == null ? null : `${collection.itemCount} ${collection.itemCount === 1 ? "item" : "items"}`}
            />
          ))}
        </div>
      )}
    </CollectionPage>
  )
}
