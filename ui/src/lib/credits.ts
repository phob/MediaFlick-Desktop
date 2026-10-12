// Splitting a Jellyfin `People` list into the two rails a detail page shows:
// performers, and the crew who held one of the key jobs.

import type { Person } from "./api"

const CAST_KINDS = new Set(["Actor", "GuestStar"])

/** Jellyfin `PersonKind` values the Crew rail shows, in rail order. */
const CREW_JOBS = [
  { kind: "Creator", label: "Creator" },
  { kind: "Director", label: "Director" },
  { kind: "Writer", label: "Writer" },
  { kind: "Producer", label: "Producer" },
  { kind: "Composer", label: "Composer" },
] as const

/**
 * `Type` is Jellyfin's own field and is occasionally absent, in which case a
 * name with a role is still a performance credit worth showing.
 */
export function castOf(people: Person[]) {
  return people.filter(
    (person) =>
      person.name && ((person.type && CAST_KINDS.has(person.type)) || (!person.type && person.role)),
  )
}

export interface CrewMember {
  person: Person
  /** Display labels in rail order, such as "Director". */
  jobs: string[]
}

/**
 * One entry per person, ordered by their highest-ranked key job and then by
 * Jellyfin's credit order, so a writer-director appears once.
 */
export function crewOf(people: Person[]): CrewMember[] {
  const members = new Map<string, { person: Person; ranks: Set<number>; order: number }>()
  people.forEach((person, order) => {
    const rank = CREW_JOBS.findIndex((job) => job.kind === person.type)
    if (!person.name || rank < 0) return
    const key = person.id ?? person.name
    const member = members.get(key) ?? { person, ranks: new Set<number>(), order }
    member.ranks.add(rank)
    if (!member.person.imageTag && person.imageTag) member.person = person
    members.set(key, member)
  })
  return [...members.values()]
    .map(({ person, ranks, order }) => ({ person, ranks, order, rank: Math.min(...ranks) }))
    .sort((left, right) => left.rank - right.rank || left.order - right.order)
    .map(({ person, ranks }) => ({
      person,
      jobs: CREW_JOBS.filter((_, rank) => ranks.has(rank)).map((job) => job.label),
    }))
}
