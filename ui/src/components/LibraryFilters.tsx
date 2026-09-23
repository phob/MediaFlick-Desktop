import { Heart, ListFilter, X } from "lucide-react"
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
import {
  activeLibraryFilterCount,
  CLEARED_LIBRARY_FILTERS,
  LIBRARY_SORTS,
  RELEASE_DECADES,
  type LibraryFilterState,
} from "@/lib/library-filters"
import { useGenres } from "@/lib/queries"
import { cn } from "@/lib/utils"

const ANY = "__any__"

const WATCHED = [
  { id: ANY, label: "Any watch status" },
  { id: "false", label: "Unwatched" },
  { id: "true", label: "Watched" },
] as const

interface LibraryFiltersProps {
  value: LibraryFilterState
  onChange: (patch: Partial<LibraryFilterState>) => void
  total: number | null
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
  genres,
  genresLoading,
}: {
  value: LibraryFilterState
  onChange: LibraryFiltersProps["onChange"]
  genres: string[]
  genresLoading: boolean
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
              onValueChange={(watched) =>
                onChange({
                  watched: watched === "true" || watched === "false" ? watched : "",
                })
              }
            >
              {WATCHED.map((option) => (
                <DropdownMenuRadioItem key={option.id} value={option.id} className="min-h-9">
                  {option.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSub>
          <DropdownMenuSubTrigger className="min-h-9">
            <span>My List</span>
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
                <Heart aria-hidden="true" /> In My List
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

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

/** One touch-sized radio group per filter; `ANY` stands for "no filter". */
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
  genres,
  genresLoading,
}: {
  value: LibraryFilterState
  onChange: LibraryFiltersProps["onChange"]
  genres: string[]
  genresLoading: boolean
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

          <TouchGroup
            label="Watch status"
            value={value.watched || ANY}
            onValueChange={(watched) =>
              onChange({ watched: watched === "true" || watched === "false" ? watched : "" })}
          >
            {WATCHED.map((option) => (
              <TouchChoice key={option.id} value={option.id}>{option.label}</TouchChoice>
            ))}
          </TouchGroup>

          <TouchGroup
            label="My List"
            value={value.favorite ? "favorite" : ANY}
            onValueChange={(favorite) => onChange({ favorite: favorite === "favorite" })}
          >
            <TouchChoice value={ANY}>All titles</TouchChoice>
            <TouchChoice value="favorite">In My List</TouchChoice>
          </TouchGroup>
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
      label: value.watched === "true" ? "Watched" : "Unwatched",
      patch: { watched: "" },
    } : null,
    value.favorite ? { key: "favorite", label: "In My List", patch: { favorite: false } } : null,
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

export function LibraryFilters({ value, onChange, total }: LibraryFiltersProps) {
  const genres = useGenres()
  const touchInput = useTouchInput()
  const genreOptions = genres.data?.genres ?? []

  return (
    <div className="space-y-3 border-b border-hairline px-6 pb-5 sm:px-10 lg:px-14">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={value.sort || "name"} onValueChange={(sort) => onChange({ sort })}>
          <SelectTrigger
            size={touchInput ? "default" : "sm"}
            aria-label="Sort by"
            className={cn(
              "border-edge bg-field shadow-none hover:bg-field-hover",
              touchInput && "min-h-11",
            )}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {LIBRARY_SORTS.map((sort) => (
              <SelectItem key={sort.id} value={sort.id}>
                Sort: {sort.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {touchInput ? (
          <TouchFilters
            value={value}
            onChange={onChange}
            genres={genreOptions}
            genresLoading={genres.isPending}
          />
        ) : (
          <DesktopFilters
            value={value}
            onChange={onChange}
            genres={genreOptions}
            genresLoading={genres.isPending}
          />
        )}

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
