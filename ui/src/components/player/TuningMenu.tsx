import {
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from "@/components/ui/dropdown-menu"
import type { PlayerCommand } from "@/lib/api"

export interface PlaybackTuning {
  audioDelay: string
  subtitleDelay: string
  subtitleScale: string
  videoFit: "fit" | "fill"
  videoAspect: "source" | "4:3" | "16:9" | "21:9"
  toneMapping: "auto" | "clip" | "mobius" | "reinhard" | "hable" | "bt.2390"
  deinterlace: boolean
}

function isVideoFit(value: string): value is PlaybackTuning["videoFit"] {
  return value === "fit" || value === "fill"
}

function isVideoAspect(value: string): value is PlaybackTuning["videoAspect"] {
  return value === "source" || value === "4:3" || value === "16:9" || value === "21:9"
}

function isToneMapping(value: string): value is PlaybackTuning["toneMapping"] {
  return ["auto", "clip", "mobius", "reinhard", "hable", "bt.2390"].includes(value)
}

const DELAYS = ["-2", "-1", "-0.5", "0", "0.5", "1", "2"]

function signedSeconds(value: string) {
  return `${Number(value) >= 0 ? "+" : ""}${value}s`
}

function DelaySubmenu({ label, value, onValueChange }: {
  label: string
  value: string
  onValueChange: (value: string) => void
}) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>{label}</DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup value={value} onValueChange={onValueChange}>
          {DELAYS.map((delay) => (
            <DropdownMenuRadioItem key={delay} value={delay}>
              {Number(delay) === 0 ? "No delay" : signedSeconds(delay)}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

export function TuningMenu({
  tuning,
  onChange,
}: {
  tuning: PlaybackTuning
  onChange: (patch: Partial<PlaybackTuning>, command: PlayerCommand, feedback: string) => void
}) {
  return (
    <>
      <DelaySubmenu
        label="Audio delay"
        value={tuning.audioDelay}
        onValueChange={(value) => onChange(
          { audioDelay: value },
          { command: "set-audio-delay", delaySeconds: Number(value) },
          `Audio delay ${signedSeconds(value)}`,
        )}
      />
      <DelaySubmenu
        label="Subtitle delay"
        value={tuning.subtitleDelay}
        onValueChange={(value) => onChange(
          { subtitleDelay: value },
          { command: "set-subtitle-delay", delaySeconds: Number(value) },
          `Subtitle delay ${signedSeconds(value)}`,
        )}
      />
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Subtitle size</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.subtitleScale}
            onValueChange={(value) =>
              onChange(
                { subtitleScale: value },
                { command: "set-subtitle-scale", scale: Number(value) },
                `Subtitle size ${Math.round(Number(value) * 100)}%`,
              )
            }
          >
            {[
              ["0.75", "Small"],
              ["1", "Normal"],
              ["1.25", "Large"],
              ["1.5", "Extra large"],
            ].map(([value, label]) => (
              <DropdownMenuRadioItem key={value} value={value}>
                {label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Video fit</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.videoFit}
            onValueChange={(value) => {
              if (!isVideoFit(value)) return
              onChange(
                { videoFit: value },
                { command: "set-video-fit", fit: value },
                value === "fill" ? "Video fills the window" : "Video fits the window",
              )
            }}
          >
            <DropdownMenuRadioItem value="fit">Fit</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="fill">Fill and crop</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Aspect ratio</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.videoAspect}
            onValueChange={(value) => {
              if (!isVideoAspect(value)) return
              onChange(
                { videoAspect: value },
                { command: "set-video-aspect", aspect: value },
                `Aspect ratio ${value === "source" ? "from source" : value}`,
              )
            }}
          >
            <DropdownMenuRadioItem value="source">From source</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="4:3">4:3</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="16:9">16:9</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="21:9">21:9</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>HDR tone mapping</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.toneMapping}
            onValueChange={(value) => {
              if (!isToneMapping(value)) return
              onChange(
                { toneMapping: value },
                { command: "set-tone-mapping", mode: value },
                `Tone mapping ${value}`,
              )
            }}
          >
            {[
              ["auto", "Automatic"],
              ["bt.2390", "BT.2390"],
              ["mobius", "Mobius"],
              ["reinhard", "Reinhard"],
              ["hable", "Hable"],
              ["clip", "Clip"],
            ].map(([value, label]) => (
              <DropdownMenuRadioItem key={value} value={value}>
                {label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuItem
        onSelect={() =>
          onChange(
            { deinterlace: !tuning.deinterlace },
            { command: "set-deinterlace", enabled: !tuning.deinterlace },
            `Deinterlacing ${tuning.deinterlace ? "off" : "on"}`,
          )
        }
      >
        Deinterlacing
        <span className="ml-auto text-xs text-muted-foreground">
          {tuning.deinterlace ? "On" : "Off"}
        </span>
      </DropdownMenuItem>
    </>
  )
}
