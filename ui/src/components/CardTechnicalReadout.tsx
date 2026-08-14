import { AudioLines, ScanLine } from "lucide-react"
import type { ItemSummary } from "@/lib/api"
import { summarizeCardMedia } from "@/lib/format"
import { useCardTechnical } from "@/lib/technical-context"

/**
 * Shared compact video/audio facts for library cards and expanded previews.
 * Streams arrive over the live batched technical channel keyed by item id;
 * until they land (or when the server is unreachable) nothing is drawn.
 */
export function CardTechnicalReadout({
  item,
  variant = "card",
  active = true,
}: {
  item: Pick<ItemSummary, "id">
  variant?: "card" | "preview"
  /** False while a mounted shelf card is outside the visible viewport. */
  active?: boolean
}) {
  const streams = useCardTechnical(item.id, active)
  const technical = summarizeCardMedia(streams)
  if (!technical) return null

  return (
    <dl
      className={variant === "preview"
        ? "card-technical-readout preview-technical-readout"
        : "card-technical-readout"}
      aria-label={`Technical media information. ${technical.description}`}
      title={technical.description}
    >
      {technical.video.length > 0 && (
        <div className="card-technical-row">
          <dt className="sr-only">Video</dt>
          <ScanLine aria-hidden />
          <dd className="card-technical-values">
            {technical.video.map((fact, index) => (
              <span key={fact}>
                {index > 0 && <span className="card-technical-separator" aria-hidden>·</span>}
                {fact}
              </span>
            ))}
          </dd>
        </div>
      )}
      {technical.audio.length > 0 && (
        <div className="card-technical-row">
          <dt className="sr-only">Audio</dt>
          <AudioLines aria-hidden />
          <dd className="card-technical-values">
            {technical.audio.map((fact, index) => (
              <span key={fact}>
                {index > 0 && <span className="card-technical-separator" aria-hidden>·</span>}
                {fact}
              </span>
            ))}
          </dd>
        </div>
      )}
    </dl>
  )
}
