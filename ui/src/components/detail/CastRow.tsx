import { User } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { personImageUrl, type Person } from "@/lib/api"
import { castSearchPath } from "@/lib/cast-search"
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
      loading="lazy"
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
    <section className="flex min-w-0 flex-col gap-4">
      <h2 className="section-title px-6 sm:px-10 lg:px-14">Cast</h2>
      <div className="media-strip flex gap-6 overflow-x-auto px-6 pb-3 sm:px-10 lg:px-14">
        {cast.map((person, index) => {
          const name = person.name ?? "Cast member"
          return (
            <Link
              key={`${person.id ?? person.name}-${index}`}
              to={castSearchPath({ jellyfinId: person.id, name })}
              aria-label={`Find titles featuring ${name}`}
              className="flex w-28 shrink-0 flex-col items-center gap-2 rounded-media text-center outline-none transition-colors hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Headshot person={person} />
              <span className="flex flex-col gap-0.5">
                <span className="text-xs leading-tight">{name}</span>
                {person.role && (
                  <span className="text-xs leading-tight text-muted-foreground">{person.role}</span>
                )}
              </span>
            </Link>
          )
        })}
      </div>
    </section>
  )
}
