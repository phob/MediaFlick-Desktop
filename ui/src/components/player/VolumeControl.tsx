import { Volume2, VolumeX } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Slider } from "@/components/ui/slider"
import { cn } from "@/lib/utils"

function MuteButton({ muted, volume, title, onToggle, className }: {
  muted: boolean
  volume: number
  title?: string
  onToggle: () => void
  className?: string
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      title={title}
      aria-label={muted ? "Unmute" : "Mute"}
      onClick={onToggle}
      className={cn("size-8", className)}
    >
      {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
    </Button>
  )
}

export function VolumeControl({ muted, volume, muteTitle, onToggleMute, onVolumeDraft, onVolumeCommit }: {
  muted: boolean
  volume: number
  muteTitle?: string
  onToggleMute: () => void
  onVolumeDraft: (volume: number) => void
  onVolumeCommit: (volume: number) => void
}) {
  return (
    <>
      <MuteButton muted={muted} volume={volume} title={muteTitle} onToggle={onToggleMute} className="shrink-0" />
      <Slider
        value={[muted ? 0 : volume]}
        max={100}
        step={1}
        aria-label={`Volume ${Math.round(volume)}%`}
        onValueChange={([value]) => onVolumeDraft(value)}
        onValueCommit={([value]) => onVolumeCommit(value)}
        className="h-8 flex-1 cursor-pointer"
      />
    </>
  )
}
