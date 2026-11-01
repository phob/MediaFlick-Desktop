import { useQuery } from "@tanstack/react-query"
import { Check, ChevronsUpDown } from "lucide-react"
import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { useLiveSearch } from "@/hooks/use-live-search"
import { api, type SeerrCompany } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { cn } from "@/lib/utils"

/** Searches TMDB companies through Seerr and picks one for the movie catalog. */
export function StudioCombobox({ value, onValueChange }: {
  value: SeerrCompany | undefined
  onValueChange: (studio: SeerrCompany) => void
}) {
  const [open, setOpen] = useState(false)
  const [term, setTerm] = useState("")
  const [search, setSearch] = useLiveSearch(term, setTerm)
  const settled = search.trim() === term
  const results = useQuery({
    queryKey: queryKeys.seerrCompanies(term),
    queryFn: ({ signal }) => api.seerr.companies(term, signal),
    enabled: open && settled && term.length >= 2,
    staleTime: 10 * 60_000,
    retry: false,
  })
  const studios = settled ? results.data?.results ?? [] : []
  const message = search.trim().length < 2 ? "Type at least two characters to search."
    : !settled || results.isFetching ? "Searching…"
    : results.error ? "Could not search studios."
    : "No studios found."
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          role="combobox"
          aria-label="Filter by studio"
          aria-expanded={open}
          aria-haspopup="listbox"
          className="h-10 w-52 justify-between border-edge bg-field font-normal"
        >
          <span className="truncate">{value?.name ?? "Any studio"}</span>
          <ChevronsUpDown className="size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 max-w-[calc(100vw-2rem)] p-0">
        <Command shouldFilter={false} label="Search studios">
          <CommandInput aria-label="Search studios" placeholder="Search studios" value={search} onValueChange={setSearch} />
          <CommandList label="Studios" aria-busy={results.isFetching}>
            <CommandEmpty>{message}</CommandEmpty>
            <CommandGroup>
              {studios.map((studio) => (
                <CommandItem
                  key={studio.id}
                  value={String(studio.id)}
                  onSelect={() => {
                    onValueChange(studio)
                    setOpen(false)
                  }}
                >
                  <Check aria-hidden className={cn("size-4", value?.id === studio.id ? "opacity-100" : "opacity-0")} />
                  <span className="min-w-0 break-words">{studio.name}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
          {results.error && settled && (
            <Button type="button" variant="ghost" disabled={results.isFetching} onClick={() => void results.refetch()}>
              Retry search
            </Button>
          )}
        </Command>
      </PopoverContent>
    </Popover>
  )
}
