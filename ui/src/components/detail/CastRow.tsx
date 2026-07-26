import { User } from "lucide-react"
import { useState } from "react"
import { personImageUrl, type Person } from "@/lib/api"
import { castOf } from "@/lib/credits"

function Headshot({ person }: { person: Person }) {
  const [failed, setFailed] = useState(false)
  const source = personImageUrl(person)

  if (!source || failed) {
    return (
      <div className="grid size-24 place-items-center rounded-full bg-card text-muted-foreground">
        <User className="size-8" aria-hidden />
      </div>
    )
  }
  return (
    <img
      src={source}
      alt=""
      decoding="async"
      onError={() => setFailed(true)}
      className="size-24 rounded-full object-cover"
    />
  )
}

/** Actors get faces; the crew reads better as a list, and lives in the facts. */
export function CastRow({ people }: { people: Person[] }) {
  const cast = castOf(people)
  if (!cast.length) return null

  return (
    <section className="flex flex-col gap-3">
      <h2 className="px-6 text-base font-medium">Cast</h2>
      <div className="flex gap-6 overflow-x-auto px-6 pb-2">
        {cast.map((person, index) => (
          <figure
            key={`${person.id ?? person.name}-${index}`}
            className="flex w-28 shrink-0 flex-col items-center gap-2 text-center"
          >
            <Headshot person={person} />
            <figcaption className="flex flex-col gap-0.5">
              <span className="text-xs leading-tight">{person.name}</span>
              {person.role && (
                <span className="text-xs leading-tight text-muted-foreground">{person.role}</span>
              )}
            </figcaption>
          </figure>
        ))}
      </div>
    </section>
  )
}
