import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useGenres } from "@/lib/queries"

/** Ids accepted by `ItemSort::from_id` (`src/library/mod.rs`). */
const SORTS = [
  { id: "name", label: "Name" },
  { id: "year", label: "Year" },
  { id: "added", label: "Recently added" },
  { id: "rating", label: "Rating" },
]

/**
 * Radix Select reserves the empty string for "no selection", so an unset
 * filter needs a sentinel. It has to be something no genre could ever be.
 */
const ANY = "__any__"

const WATCHED = [
  { id: ANY, label: "All" },
  { id: "false", label: "Unwatched" },
  { id: "true", label: "Watched" },
]

export interface LibraryFilterState {
  sort: string
  genre: string
  watched: string
}

interface LibraryFiltersProps {
  value: LibraryFilterState
  onChange: (patch: Partial<LibraryFilterState>) => void
  total: number | null
}

export function LibraryFilters({ value, onChange, total }: LibraryFiltersProps) {
  const genres = useGenres()

  return (
    <div className="flex flex-wrap items-center gap-2 px-6 py-4">
      <Select value={value.sort || "name"} onValueChange={(sort) => onChange({ sort })}>
        <SelectTrigger size="sm" aria-label="Sort by">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {SORTS.map((sort) => (
            <SelectItem key={sort.id} value={sort.id}>
              Sort: {sort.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={value.watched || ANY}
        onValueChange={(watched) => onChange({ watched: watched === ANY ? "" : watched })}
      >
        <SelectTrigger size="sm" aria-label="Filter by watched state">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {WATCHED.map((option) => (
            <SelectItem key={option.id} value={option.id}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={value.genre || ANY}
        onValueChange={(genre) => onChange({ genre: genre === ANY ? "" : genre })}
      >
        <SelectTrigger size="sm" aria-label="Filter by genre">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ANY}>All genres</SelectItem>
          {(genres.data?.genres ?? []).map((genre) => (
            <SelectItem key={genre} value={genre}>
              {genre}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {total !== null && (
        <span className="ml-auto text-sm text-muted-foreground">
          {total.toLocaleString()} {total === 1 ? "item" : "items"}
        </span>
      )}
    </div>
  )
}
