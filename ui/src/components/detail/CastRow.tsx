import { DetailPeopleRail } from "@/components/detail/DetailPrimitives"
import { personImageUrl, type Person } from "@/lib/api"
import { castSearchPath } from "@/lib/cast-search"
import { castOf, crewOf } from "@/lib/credits"

export function CastRow({ people }: { people: Person[] }) {
  const cast = castOf(people)
  const entries = cast.map((person, index) => {
    const name = person.name ?? "Cast member"
    return {
      key: `${person.id ?? person.name}-${index}`,
      name,
      role: person.role,
      imageUrl: personImageUrl(person),
      to: castSearchPath({ jellyfinId: person.id, name }),
    }
  })
  return <DetailPeopleRail heading="Cast" entries={entries} />
}

export function CrewRow({ people }: { people: Person[] }) {
  const entries = crewOf(people).map(({ person, jobs }, index) => {
    const name = person.name ?? "Crew member"
    return {
      key: `${person.id ?? person.name}-${index}`,
      name,
      role: jobs.join(", "),
      imageUrl: personImageUrl(person),
      to: castSearchPath({ jellyfinId: person.id, name }),
    }
  })
  return <DetailPeopleRail heading="Crew" entries={entries} />
}
