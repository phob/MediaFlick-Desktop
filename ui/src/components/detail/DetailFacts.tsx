import type { ReactNode } from "react"
import { DetailFact, DetailFactPanel } from "@/components/detail/DetailPrimitives"
import { Badge } from "@/components/ui/badge"
import type { AnyItemDetail, ItemAbout } from "@/lib/api"
import { formatDate } from "@/lib/format"

export function DetailFacts({ item, about }: { item: AnyItemDetail; about?: ItemAbout }) {
  const title = item.layout === "title"
  const studios = about?.studios ?? []
  const tags = title ? about?.tags ?? [] : []
  const premiere = formatDate(item.premiereDate)
  const added = formatDate(item.dateCreated)

  const facts: { label: string; value: ReactNode }[] = [
    studios.length ? { label: "Studio", value: studios.join(", ") } : null,
    premiere ? { label: "Premiered", value: premiere } : null,
    added ? { label: "Added", value: added } : null,
    title && item.playCount > 0
      ? { label: "Plays", value: `${item.playCount} ${item.playCount === 1 ? "time" : "times"}` }
      : null,
    tags.length
      ? {
          label: "Tags",
          value: (
            <div className="flex flex-wrap gap-1.5">
              {tags.map((tag) => (
                <Badge key={tag} variant="outline" className="border-border font-normal">
                  {tag}
                </Badge>
              ))}
            </div>
          ),
        }
      : null,
  ].filter((fact) => fact !== null)

  if (!facts.length) return null

  return (
    <section className="flex flex-col gap-3">
      <h2 className="section-title">Details</h2>
      <DetailFactPanel>
        {facts.map((fact) => (
          <DetailFact key={fact.label} label={fact.label}>
            {fact.value}
          </DetailFact>
        ))}
      </DetailFactPanel>
    </section>
  )
}
