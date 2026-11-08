import type { ReactNode } from "react"
import { Link } from "react-router-dom"
import { DetailFact, DetailFactPanel } from "@/components/detail/DetailPrimitives"
import { Badge } from "@/components/ui/badge"
import type { GenericItemDetail, ItemAbout, ItemDetail } from "@/lib/api"
import { formatDate } from "@/lib/format"
import { facetRoute } from "@/lib/libraries"

export function DetailFacts({ item, about }: { item: ItemDetail | GenericItemDetail; about?: ItemAbout }) {
  const title = item.layout === "title" ? item : null
  const studios = about?.studios ?? []
  const tags = title ? about?.tags ?? [] : []
  const premiere = formatDate(item.premiereDate)
  const added = formatDate(item.dateCreated)

  const facts: { label: string; value: ReactNode }[] = [
    studios.length
      ? {
          label: "Studio",
          value: title
            ? studios.map((studio, index) => (
                <span key={studio}>
                  {index > 0 && ", "}
                  <Link to={facetRoute("studio", studio, title)} className="hover:underline">
                    {studio}
                  </Link>
                </span>
              ))
            : studios.join(", "),
        }
      : null,
    premiere ? { label: "Premiered", value: premiere } : null,
    added ? { label: "Added", value: added } : null,
    title && title.playCount > 0
      ? { label: "Plays", value: `${title.playCount} ${title.playCount === 1 ? "time" : "times"}` }
      : null,
    title && tags.length
      ? {
          label: "Tags",
          value: (
            <div className="flex flex-wrap gap-1.5">
              {tags.map((tag) => (
                <Badge key={tag} variant="outline" asChild className="border-border font-normal hover:bg-secondary/70">
                  <Link to={facetRoute("tag", tag, title)}>{tag}</Link>
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
