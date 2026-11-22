import type { Person } from "./api"

const CAST_KINDS = new Set(["Actor", "GuestStar"])

const CREW_JOBS = [
  { kind: "Creator", label: "Creator" },
  { kind: "Director", label: "Director" },
  { kind: "Writer", label: "Writer" },
  { kind: "Producer", label: "Producer" },
  { kind: "Composer", label: "Composer" },
] as const

export function castOf(people: Person[]) {
  return people.filter(
    (person) =>
      person.name && ((person.type && CAST_KINDS.has(person.type)) || (!person.type && person.role)),
  )
}

export interface CrewMember {
  person: Person
  jobs: string[]
}

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
