import type { ReactNode } from "react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import type { PlayerTrack } from "@/lib/api"
import { trackLabel } from "@/components/player/format"

export const SUBTITLES_OFF = "__off__"

export interface TrackChoices {
  tracks: PlayerTrack[]
  value: string
  allowOff?: boolean
  onValueChange: (value: string) => void
}

function TrackRadioItems({ tracks, value, allowOff = false, onValueChange }: TrackChoices) {
  return (
    <DropdownMenuRadioGroup value={value} onValueChange={onValueChange}>
      {allowOff && <DropdownMenuRadioItem value={SUBTITLES_OFF}>Off</DropdownMenuRadioItem>}
      {tracks.map((track, index) => (
        <DropdownMenuRadioItem key={track.id} value={String(track.id)}>
          <span className="min-w-0 truncate">{trackLabel(track, index)}</span>
        </DropdownMenuRadioItem>
      ))}
    </DropdownMenuRadioGroup>
  )
}

export function TrackMenu({
  icon,
  label,
  onOpenChange,
  ...choices
}: TrackChoices & {
  icon: ReactNode
  label: string
  onOpenChange?: (open: boolean) => void
}) {
  const selected = choices.tracks.find((track) => String(track.id) === choices.value)
  const title = selected ? `${label}: ${trackLabel(selected, choices.tracks.indexOf(selected))}` : label

  return (
    <DropdownMenu onOpenChange={onOpenChange}>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="icon" title={title} aria-label={label}>
          {icon}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="end" sideOffset={10} className="w-72">
        <DropdownMenuLabel>{label}</DropdownMenuLabel>
        <TrackRadioItems {...choices} />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function TrackSubmenu({ label, ...choices }: TrackChoices & { label: string }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>{label}</DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="w-72">
        <TrackRadioItems {...choices} />
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}
