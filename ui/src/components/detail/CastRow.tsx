import { DetailCastRail } from "@/components/detail/DetailPrimitives"
import { personImageUrl, type Person } from "@/lib/api"
import { castSearchPath } from "@/lib/cast-search"
import { castOf } from "@/lib/credits"

/** Actors get faces; the crew reads better as a list, and lives in the facts. */
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
  return <DetailCastRail entries={entries} />
}
