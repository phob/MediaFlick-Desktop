import { ArrowDownWideNarrow, ArrowUpNarrowWide, Heart, ListFilter, Shuffle, X } from "lucide-react"
import { useId, type ComponentProps, type ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet"
import { useTouchInput } from "@/hooks/use-touch-input"
import type { BrowseCapabilities, LibraryFilterId } from "@/lib/api"
import type { ListingSource } from "@/lib/libraries"
import {
  activeLibraryFilterCount,
  CLEARED_LIBRARY_FILTERS,
  librarySortLabel,
  newShuffleSeed,
  offersFilter,
  RELEASE_DECADES,
  SERIES_STATUSES,
  sortsFor,
  WATCH_FILTERS,
  type LibraryFilterState,
} from "@/lib/library-filters"
import { useGenres, useLibraryOptions, useOfficialRatings } from "@/lib/queries"
import { cn } from "@/lib/utils"

const ANY = "__any__"

const WATCHED = [{ id: ANY, label: "Any watch status" }, ...WATCH_FILTERS]

interface FilterOptions {
  genres: string[]
  genresLoading: boolean
  ratings: string[]
  ratingsLoading: boolean
  offered: (filter: LibraryFilterId) => boolean
}

function watchFilter(value: string) {
  return WATCH_FILTERS.find((option) => option.id === value)?.id ?? ""
}

function seriesStatus(value: string) {
  return SERIES_STATUSES.find((status) => status === value) ?? ""
}

interface LibraryFiltersProps {
  value: LibraryFilterState
  onChange: (patch: Partial<LibraryFilterState>) => void
  total: number | null
  capabilities: BrowseCapabilities
  source: ListingSource
  tvShowsLibrary?: boolean
}

function currentValue(value: string, fallback: string) {
  return value || fallback
}

function FiltersButton({
  count,
  touch = false,
  className,
  ...props
}: { count: number; touch?: boolean } & Omit<ComponentProps<typeof Button>, "children">) {
  return (
    <Button
      {...props}
      type="button"
      variant="outline"
      size={touch ? "default" : "sm"}
      aria-label={`Filters${count ? `, ${count} active` : ""}`}
      className={cn(
        "border-edge bg-field shadow-none hover:bg-field-hover",
        touch && "min-h-11",
        className,
      )}
    >
      <ListFilter aria-hidden="true" />
      Filters
      {count > 0 && (
        <span
          aria-hidden="true"
          className="ml-0.5 inline-flex min-w-5 items-center justify-center rounded-full bg-primary px-1.5 text-[0.6875rem] leading-5 font-semibold text-primary-foreground"
        >
          {count}
        </span>
      )}
    </Button>
  )
}

function DesktopFilters({
  value,
  onChange,
  options: { genres, genresLoading, ratings, ratingsLoading, offered },
}: {
  value: LibraryFilterState
  onChange: LibraryFiltersProps["onChange"]
  options: FilterOptions
}) {
  const count = activeLibraryFilterCount(value)
  const watchedLabel = WATCHED.find((option) => option.id === (value.watched || ANY))?.label

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <FiltersButton count={count} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64" aria-label="Library filters">
        <DropdownMenuLabel>Filter library</DropdownMenuLabel>
        <DropdownMenuSeparator />

        {offered("genre") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Genre</span>
              <span className="ml-auto max-w-28 truncate text-xs text-muted-foreground">
                {currentValue(value.genre, "Any")}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="max-h-80 w-56 overflow-y-auto">
              <DropdownMenuRadioGroup
                value={value.genre || ANY}
                onValueChange={(genre) => onChange({ genre: genre === ANY ? "" : genre })}
              >
                <DropdownMenuRadioItem value={ANY} className="min-h-9">
                  Any genre
                </DropdownMenuRadioItem>
                {genres.map((genre) => (
                  <DropdownMenuRadioItem key={genre} value={genre} className="min-h-9">
                    {genre}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              {genresLoading && (
                <DropdownMenuItem disabled className="min-h-9">
                  Loading genres…
                </DropdownMenuItem>
              )}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        {offered("decade") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Release decade</span>
              <span className="ml-auto text-xs text-muted-foreground">
                {currentValue(value.decade, "Any")}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="max-h-80 w-44 overflow-y-auto">
              <DropdownMenuRadioGroup
                value={value.decade || ANY}
                onValueChange={(decade) => onChange({ decade: decade === ANY ? "" : decade })}
              >
                <DropdownMenuRadioItem value={ANY} className="min-h-9">
                  Any decade
                </DropdownMenuRadioItem>
                {RELEASE_DECADES.map((decade) => (
                  <DropdownMenuRadioItem
                    key={decade.value}
                    value={String(decade.value)}
                    className="min-h-9"
                  >
                    {decade.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        {offered("watched") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Watch status</span>
              <span className="ml-auto max-w-28 truncate text-xs text-muted-foreground">
                {watchedLabel}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-52">
              <DropdownMenuRadioGroup
                value={value.watched || ANY}
                onValueChange={(watched) => onChange({ watched: watchFilter(watched) })}
              >
                {WATCHED.map((option) => (
                  <DropdownMenuRadioItem key={option.id} value={option.id} className="min-h-9">
                    {option.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        {offered("parental") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Parental rating</span>
              <span className="ml-auto max-w-28 truncate text-xs text-muted-foreground">
                {currentValue(value.parental, "Any")}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="max-h-80 w-48 overflow-y-auto">
              <DropdownMenuRadioGroup
                value={value.parental || ANY}
                onValueChange={(parental) => onChange({ parental: parental === ANY ? "" : parental })}
              >
                <DropdownMenuRadioItem value={ANY} className="min-h-9">
                  Any rating
                </DropdownMenuRadioItem>
                {ratings.map((rating) => (
                  <DropdownMenuRadioItem key={rating} value={rating} className="min-h-9">
                    {rating}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              {ratingsLoading && (
                <DropdownMenuItem disabled className="min-h-9">
                  Loading ratings…
                </DropdownMenuItem>
              )}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        {offered("seriesStatus") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Series status</span>
              <span className="ml-auto text-xs text-muted-foreground">
                {currentValue(value.seriesStatus, "Any")}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-48">
              <DropdownMenuRadioGroup
                value={value.seriesStatus || ANY}
                onValueChange={(status) => onChange({ seriesStatus: seriesStatus(status) })}
              >
                <DropdownMenuRadioItem value={ANY} className="min-h-9">
                  Any status
                </DropdownMenuRadioItem>
                {SERIES_STATUSES.map((status) => (
                  <DropdownMenuRadioItem key={status} value={status} className="min-h-9">
                    {status}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        {offered("favorite") && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className="min-h-9">
              <span>Favorites</span>
              <span className="ml-auto text-xs text-muted-foreground">
                {value.favorite ? "Only" : "Any"}
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-52">
              <DropdownMenuRadioGroup
                value={value.favorite ? "favorite" : ANY}
                onValueChange={(favorite) => onChange({ favorite: favorite === "favorite" })}
              >
                <DropdownMenuRadioItem value={ANY} className="min-h-9">
                  All titles
                </DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="favorite" className="min-h-9">
                  <Heart aria-hidden="true" /> Favorites only
                </DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}

        <DropdownMenuSeparator />
        <DropdownMenuItem
          disabled={count === 0}
          className="min-h-9"
          onSelect={() => onChange(CLEARED_LIBRARY_FILTERS)}
        >
          Clear all filters
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function TouchChoice({ value, children }: { value: string; children: ReactNode }) {
  return (
    <Label
      className={cn(
        "flex min-h-11 cursor-pointer items-center gap-3 rounded-md border border-edge bg-white/4 px-3 py-2 text-left text-sm leading-normal font-normal text-muted-foreground",
        "has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring",
        "has-[[data-state=checked]]:border-primary/60 has-[[data-state=checked]]:bg-primary/12 has-[[data-state=checked]]:text-foreground",
      )}
    >
      <RadioGroupItem value={value} />
      {children}
    </Label>
  )
}

function TouchGroup({
  label,
  value,
  onValueChange,
  children,
}: {
  label: string
  value: string
  onValueChange: (value: string) => void
  children: ReactNode
}) {
  const labelId = useId()
  return (
    <div className="space-y-2">
      <p id={labelId} className="mb-2 text-sm font-medium text-foreground">{label}</p>
      <RadioGroup
        aria-labelledby={labelId}
        value={value}
        onValueChange={onValueChange}
        className="grid gap-2 sm:grid-cols-2"
      >
        {children}
      </RadioGroup>
    </div>
  )
}

function TouchFilters({
  value,
  onChange,
  options: { genres, genresLoading, ratings, ratingsLoading, offered },
}: {
  value: LibraryFilterState
  onChange: LibraryFiltersProps["onChange"]
  options: FilterOptions
}) {
  const count = activeLibraryFilterCount(value)

  return (
    <Sheet>
      <SheetTrigger asChild>
        <FiltersButton count={count} touch />
      </SheetTrigger>
      <SheetContent side="bottom" className="max-h-[88dvh] gap-0 rounded-t-xl">
        <SheetHeader className="border-b">
          <SheetTitle>Filter library</SheetTitle>
          <SheetDescription>
            Choose one value in each category. Selections apply to the complete library.
          </SheetDescription>
        </SheetHeader>
        <div className="space-y-6 overflow-y-auto px-4 py-5">
          {offered("genre") && (
            <TouchGroup
              label="Genre"
              value={value.genre || ANY}
              onValueChange={(genre) => onChange({ genre: genre === ANY ? "" : genre })}
            >
              <TouchChoice value={ANY}>Any genre</TouchChoice>
              {genres.map((genre) => (
                <TouchChoice key={genre} value={genre}>{genre}</TouchChoice>
              ))}
              {genresLoading && (
                <p className="px-3 py-2 text-sm text-muted-foreground">Loading genres…</p>
              )}
            </TouchGroup>
          )}

          {offered("decade") && (
            <TouchGroup
              label="Release decade"
              value={value.decade || ANY}
              onValueChange={(decade) => onChange({ decade: decade === ANY ? "" : decade })}
            >
              <TouchChoice value={ANY}>Any decade</TouchChoice>
              {RELEASE_DECADES.map((decade) => (
                <TouchChoice key={decade.value} value={String(decade.value)}>{decade.label}</TouchChoice>
              ))}
            </TouchGroup>
          )}

          {offered("watched") && (
            <TouchGroup
              label="Watch status"
              value={value.watched || ANY}
              onValueChange={(watched) => onChange({ watched: watchFilter(watched) })}
            >
              {WATCHED.map((option) => (
                <TouchChoice key={option.id} value={option.id}>{option.label}</TouchChoice>
              ))}
            </TouchGroup>
          )}

          {offered("parental") && (
            <TouchGroup
              label="Parental rating"
              value={value.parental || ANY}
              onValueChange={(parental) => onChange({ parental: parental === ANY ? "" : parental })}
            >
              <TouchChoice value={ANY}>Any rating</TouchChoice>
              {ratings.map((rating) => (
                <TouchChoice key={rating} value={rating}>{rating}</TouchChoice>
              ))}
              {ratingsLoading && (
                <p className="px-3 py-2 text-sm text-muted-foreground">Loading ratings…</p>
              )}
            </TouchGroup>
          )}

          {offered("seriesStatus") && (
            <TouchGroup
              label="Series status"
              value={value.seriesStatus || ANY}
              onValueChange={(status) => onChange({ seriesStatus: seriesStatus(status) })}
            >
              <TouchChoice value={ANY}>Any status</TouchChoice>
              {SERIES_STATUSES.map((status) => (
                <TouchChoice key={status} value={status}>{status}</TouchChoice>
              ))}
            </TouchGroup>
          )}

          {offered("favorite") && (
            <TouchGroup
              label="Favorites"
              value={value.favorite ? "favorite" : ANY}
              onValueChange={(favorite) => onChange({ favorite: favorite === "favorite" })}
            >
              <TouchChoice value={ANY}>All titles</TouchChoice>
              <TouchChoice value="favorite">Favorites only</TouchChoice>
            </TouchGroup>
          )}
        </div>
        <SheetFooter className="flex-row border-t">
          <Button
            type="button"
            variant="ghost"
            className="min-h-11 flex-1"
            disabled={count === 0}
            onClick={() => onChange(CLEARED_LIBRARY_FILTERS)}
          >
            Clear all
          </Button>
          <SheetClose asChild>
            <Button type="button" className="min-h-11 flex-1">Done</Button>
          </SheetClose>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}

function ActiveFilters({
  value,
  onChange,
  touch,
}: {
  value: LibraryFilterState
  onChange: LibraryFiltersProps["onChange"]
  touch: boolean
}) {
  type FilterChip = { key: string; label: string; patch: Partial<LibraryFilterState> }
  const candidates: Array<FilterChip | null> = [
    value.genre ? { key: "genre", label: `Genre: ${value.genre}`, patch: { genre: "" } } : null,
    value.decade ? { key: "decade", label: `Released: ${value.decade}s`, patch: { decade: "" } } : null,
    value.watched ? {
      key: "watched",
      label: WATCH_FILTERS.find((option) => option.id === value.watched)?.label ?? value.watched,
      patch: { watched: "" },
    } : null,
    value.parental
      ? { key: "parental", label: `Rated: ${value.parental}`, patch: { parental: "" } }
      : null,
    value.seriesStatus
      ? { key: "seriesStatus", label: value.seriesStatus, patch: { seriesStatus: "" } }
      : null,
    value.favorite ? { key: "favorite", label: "Favorites", patch: { favorite: false } } : null,
  ]
  const chips = candidates.filter((chip) => chip !== null)

  if (!chips.length) return null

  return (
    <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Active filters">
      {chips.map((chip) => (
        <Button
          key={chip.key}
          type="button"
          variant="secondary"
          size="default"
          className={cn("h-9 rounded-full px-3 text-xs", touch && "min-h-11")}
          aria-label={`Remove ${chip.label} filter`}
          onClick={() => onChange(chip.patch)}
        >
          {chip.label}
          <X aria-hidden="true" />
        </Button>
      ))}
      <Button
        type="button"
        variant="ghost"
        size="default"
        className={cn("h-9 px-3 text-xs text-muted-foreground", touch && "min-h-11")}
        onClick={() => onChange(CLEARED_LIBRARY_FILTERS)}
      >
        Clear all
      </Button>
    </div>
  )
}

export function LibraryFilters({
  value,
  onChange,
  total,
  capabilities,
  source,
  tvShowsLibrary = false,
}: LibraryFiltersProps) {
  const offered = (filter: LibraryFilterId) => offersFilter(capabilities, filter)
  const libraryId = source.scope === "library" ? source.libraryId : null
  const listsOptions = offered("genre") || offered("parental")
  const libraryOptions = useLibraryOptions(listsOptions ? libraryId : null)
  const titleViewOptions = listsOptions && source.scope === "titles"
  const genres = useGenres(titleViewOptions)
  const ratings = useOfficialRatings(source.scope === "titles" ? source.query.kind ?? "" : "", titleViewOptions)
  const touchInput = useTouchInput()
  const options: FilterOptions = libraryId === null
    ? {
        genres: genres.data?.genres ?? [],
        genresLoading: genres.isPending,
        ratings: ratings.data?.officialRatings ?? [],
        ratingsLoading: ratings.isPending,
        offered,
      }
    : {
        genres: libraryOptions.data?.genres ?? [],
        genresLoading: libraryOptions.isPending,
        ratings: libraryOptions.data?.officialRatings ?? [],
        ratingsLoading: libraryOptions.isPending,
        offered,
      }
  const sorts = sortsFor(capabilities)
  const fieldClass = cn(
    "border-edge bg-field shadow-none hover:bg-field-hover",
    touchInput && "min-h-11",
  )
  const iconSize = touchInput ? "icon" : "icon-sm"
  const chooseSort = (id: string) => {
    const sort = sorts.find((option) => option.id === id)
    if (!sort) return
    onChange(sort.id === "random" ? { sort: sort.id, seed: newShuffleSeed() } : { sort: sort.id })
  }
  const ascending = value.order === "asc"

  return (
    <div className="space-y-3 border-b border-hairline px-6 pb-5 sm:px-10 lg:px-14">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={value.sort} onValueChange={chooseSort}>
          <SelectTrigger
            size={touchInput ? "default" : "sm"}
            aria-label="Sort by"
            className={fieldClass}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {sorts.map((sort) => (
              <SelectItem key={sort.id} value={sort.id}>
                Sort: {librarySortLabel(sort, tvShowsLibrary)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {value.sort === "random" ? (
          <Button
            type="button"
            variant="outline"
            size={iconSize}
            aria-label="Shuffle again"
            title="Shuffle again"
            className={cn(fieldClass, touchInput && "min-w-11")}
            onClick={() => onChange({ seed: newShuffleSeed() })}
          >
            <Shuffle aria-hidden="true" />
          </Button>
        ) : (
          <Button
            type="button"
            variant="outline"
            size={iconSize}
            aria-label={ascending ? "Sort descending" : "Sort ascending"}
            title={ascending ? "Ascending. Sort descending" : "Descending. Sort ascending"}
            className={cn(fieldClass, touchInput && "min-w-11")}
            onClick={() => onChange({ order: ascending ? "desc" : "asc" })}
          >
            {ascending ? <ArrowUpNarrowWide aria-hidden="true" /> : <ArrowDownWideNarrow aria-hidden="true" />}
          </Button>
        )}

        {capabilities.filters.length > 0 && (touchInput ? (
          <TouchFilters value={value} onChange={onChange} options={options} />
        ) : (
          <DesktopFilters value={value} onChange={onChange} options={options} />
        ))}

        {total !== null && (
          <span className="ml-auto text-sm text-muted-foreground" aria-live="polite">
            {total.toLocaleString()} {total === 1 ? "item" : "items"}
          </span>
        )}
      </div>
      <ActiveFilters value={value} onChange={onChange} touch={touchInput} />
    </div>
  )
}
