import { useId, useState } from "react"
import type { PlayerComfort } from "@/lib/api"
import { Button } from "@/components/ui/button"

const scenes = {
  Day: ["#86c5e4", "#f9ead1", "#899ea1", "#496d76", "#acc7c6"],
  Dusk: ["#514f82", "#f3b48e", "#74718d", "#343f5c", "#9585a1"],
  Night: ["#101b34", "#485574", "#34435d", "#15283b", "#425a70"],
} as const

export function SubtitlePreview({ comfort }: { comfort: PlayerComfort }) {
  const [scene, setScene] = useState<keyof typeof scenes>("Day")
  const id = useId()
  const [sky, horizon, mountain, foreground, water] = scenes[scene]
  return <figure className="space-y-3" aria-label="Subtitle preview">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <span className="text-sm font-medium">Subtitle preview</span>
      <div className="flex gap-1" role="group" aria-label="Preview scene">
        {(Object.keys(scenes) as (keyof typeof scenes)[]).map((name) => <Button key={name} type="button" size="sm" variant={scene === name ? "secondary" : "ghost"} aria-pressed={scene === name} onClick={() => setScene(name)}>{name}</Button>)}
      </div>
    </div>
    <div className="relative aspect-video overflow-hidden rounded-lg border" style={{ containerType: "inline-size" }}>
      <svg className="absolute inset-0 size-full" viewBox="0 0 800 450" aria-hidden="true">
        <defs>
          <linearGradient id={`${id}-sky`} x2="0" y2="1"><stop stopColor={sky} /><stop offset="1" stopColor={horizon} /></linearGradient>
          <linearGradient id={`${id}-water`} x2="1" y2="1"><stop stopColor={horizon} /><stop offset="1" stopColor={water} /></linearGradient>
        </defs>
        <path fill={`url(#${id}-sky)`} d="M0 0H800V450H0Z" />
        <circle cx="610" cy="108" r="35" fill="#fff3d3" opacity={scene === "Night" ? 0.65 : 0.9} />
        <path fill={mountain} d="M0 260 130 105 235 214 360 90 550 275 685 165 800 260V450H0Z" />
        <path fill={horizon} opacity=".65" d="m97 145 33-40 58 60-56-23-15 21Zm220-10 43-45 62 61-58-28-22 29Z" />
        <path fill={`url(#${id}-water)`} d="M0 278Q220 246 420 278T800 272V450H0Z" />
        <path stroke={horizon} strokeWidth="2" opacity=".5" d="M420 303h240m-315 25h245m-125 34h285m-430 29h260m-110 30h270" />
        <path fill={foreground} d="M0 230 80 270 180 293 95 320 230 354 85 380 0 395Zm800 84-110 25 62 24-147 40 195 47Z" />
      </svg>
      <div className="absolute inset-[7%]">
        <div className="absolute w-full text-center font-sans font-medium text-white" style={{
          top: `${comfort.subtitlePosition}%`,
          transform: `translateY(-${comfort.subtitlePosition}%)`,
          fontSize: `${3.5 * comfort.subtitleSize / 100}cqw`,
          lineHeight: 1.4,
        }}>
          {["Somewhere beyond", "the familiar horizon."].map((line) => <div key={line}><span style={{
            backgroundColor: `rgb(0 0 0 / ${comfort.subtitleBackground}%)`,
            padding: "0.08em 0.25em",
            // Paint the fill last so large strokes cannot cover the letters.
            paintOrder: "stroke fill",
            WebkitTextFillColor: "white",
            WebkitTextStroke: `${comfort.subtitleOutline * 0.16}cqw black`,
          }}>{line}</span></div>)}
        </div>
      </div>
    </div>
    <figcaption className="text-xs text-muted-foreground">Compare readability in bright and dark scenes. Actual appearance can vary by subtitle track.</figcaption>
  </figure>
}
