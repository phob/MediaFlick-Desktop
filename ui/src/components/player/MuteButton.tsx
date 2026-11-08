import { Volume2, VolumeX } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export function MuteButton({ muted, volume, title, onToggle, className }: {
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
