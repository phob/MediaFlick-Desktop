import { MediaCard } from "@/components/MediaCard"
import { Skeleton } from "@/components/ui/skeleton"
import type { HomeRow } from "@/lib/api"
import { useHome } from "@/lib/queries"

function Row({ row }: { row: HomeRow }) {
  if (!row.items.length) return null
  return (
    <section className="flex flex-col gap-3">
      <h2 className="px-6 text-base font-medium">{row.title}</h2>
      {/* Carousel rows are the primary discovery pattern; grids are for search
          and view-all. See .planning/research/silo-server-web.md. */}
      <div className="flex gap-[var(--card-gap)] overflow-x-auto px-6 pb-2">
        {row.items.map((item) => (
          <MediaCard key={item.id} item={item} className="shrink-0" />
        ))}
      </div>
    </section>
  )
}

function RowSkeleton() {
  return (
    <section className="flex flex-col gap-3">
      <Skeleton className="mx-6 h-5 w-40" />
      <div className="flex gap-[var(--card-gap)] px-6">
        {Array.from({ length: 6 }, (_, index) => (
          <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
        ))}
      </div>
    </section>
  )
}

export default function Home() {
  const { data, isPending, error } = useHome()

  if (error) {
    return <p className="p-6 text-sm text-destructive">{error.message}</p>
  }

  if (isPending) {
    return (
      <div className="flex flex-col gap-8 py-6">
        <RowSkeleton />
        <RowSkeleton />
      </div>
    )
  }

  const rows = data.rows.filter((row) => row.items.length > 0)
  if (!rows.length) {
    return (
      <p className="p-6 text-sm text-muted-foreground">
        Nothing here yet — the library sync may still be running.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-8 py-6">
      {rows.map((row) => (
        <Row key={row.id} row={row} />
      ))}
    </div>
  )
}
