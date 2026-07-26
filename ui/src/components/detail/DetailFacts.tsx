import type { ReactNode } from "react"
import { Badge } from "@/components/ui/badge"
import type { ItemDetail } from "@/lib/api"
import { crewOf } from "@/lib/credits"
import { formatDate } from "@/lib/format"

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[8rem_1fr] gap-4 py-2 text-sm">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  )
}

/**
 * Everything about the item that is not artwork, playback, or a synopsis:
 * credits, studios, dates, and the tags the library was imported with.
 */
export function DetailFacts({ item }: { item: ItemDetail }) {
  const crew = crewOf(item.people)
  const premiere = formatDate(item.premiereDate)
  const added = formatDate(item.dateCreated)

  const facts: { label: string; value: ReactNode }[] = [
    ...crew.map((group) => ({ label: group.job, value: group.names.join(", ") })),
    item.studios.length ? { label: "Studio", value: item.studios.join(", ") } : null,
    premiere ? { label: "Premiered", value: premiere } : null,
    added ? { label: "Added", value: added } : null,
    item.playCount > 0
      ? { label: "Plays", value: `${item.playCount} ${item.playCount === 1 ? "time" : "times"}` }
      : null,
    item.tags.length
      ? {
          label: "Tags",
          value: (
            <div className="flex flex-wrap gap-1.5">
              {item.tags.map((tag) => (
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
      <h2 className="text-base font-medium">Details</h2>
      <dl className="divide-y divide-border/60 rounded-lg bg-card/60 px-4 py-1">
        {facts.map((fact) => (
          <Fact key={fact.label} label={fact.label}>
            {fact.value}
          </Fact>
        ))}
      </dl>
    </section>
  )
}
