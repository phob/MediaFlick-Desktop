import { useQuery } from "@tanstack/react-query"
import { Check, ChevronsUpDown } from "lucide-react"
import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { useLiveSearch } from "@/hooks/use-live-search"
import { api, type PublicCollectionList } from "@/lib/api"
import { cn } from "@/lib/utils"

export default function PublicListCombobox({ account, value, disabled, onValueChange }: {
  account: string
  value: string
  disabled: boolean
  onValueChange: (value: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [term, setTerm] = useState("")
  const [search, setSearch] = useLiveSearch(term, setTerm)
  const [selected, setSelected] = useState<PublicCollectionList | null>(null)
  const settled = search.trim() === term
  const results = useQuery({
    queryKey: ["collections", account, "public-list-search", term],
    queryFn: ({ signal }) => api.collections.searchPublicLists(term, signal),
    enabled: open && !disabled && settled && term.length >= 2,
    staleTime: 60_000,
  })
  const lists = settled ? results.data?.lists ?? [] : []
  const current = selected?.id === value ? selected : lists.find((list) => list.id === value)
  const message = search.trim().length < 2 ? "Type at least two characters to search."
    : !settled || results.isFetching ? "Searching…"
    : results.error ? "Could not search public lists."
    : "No public lists found."
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger asChild>
      <Button type="button" variant="outline" role="combobox" aria-label="Choose MDBList public list" aria-expanded={open} aria-haspopup="listbox" disabled={disabled} className="h-auto min-h-9 w-full justify-between whitespace-normal text-left">
        <span className="min-w-0 break-words">{current ? `${current.name}${current.owner ? ` by ${current.owner}` : ""}` : value ? `Public list: ${value}` : "Search public lists"}</span>
        <ChevronsUpDown className="size-4 shrink-0 opacity-50" />
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" className="w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-2rem)] p-0">
      <Command shouldFilter={false} label="Search MDBList public lists">
        <CommandInput aria-label="Search MDBList public lists" placeholder="Search public lists" value={search} onValueChange={setSearch} disabled={disabled} />
        <CommandList label="MDBList public lists" aria-busy={results.isFetching}>
          <CommandEmpty>{message}</CommandEmpty>
          <CommandGroup>
            {lists.map((list) => <CommandItem key={list.id} value={list.id} aria-label={`${list.name}${list.owner ? ` by ${list.owner}` : ""}${value === list.id ? " (selected)" : ""}`} disabled={disabled} onSelect={() => {
              setSelected(list)
              onValueChange(list.id)
              setOpen(false)
            }}>
              <Check aria-hidden className={cn("size-4", value === list.id ? "opacity-100" : "opacity-0")} />
              <span className="min-w-0 break-words">{list.name}{list.owner && <span className="ml-2 text-muted-foreground"> by {list.owner}</span>}{value === list.id && <span className="sr-only"> (selected)</span>}</span>
            </CommandItem>)}
          </CommandGroup>
        </CommandList>
        {results.error && settled && <Button type="button" variant="ghost" disabled={disabled || results.isFetching} onClick={() => void results.refetch()}>Retry search</Button>}
      </Command>
    </PopoverContent>
  </Popover>
}
