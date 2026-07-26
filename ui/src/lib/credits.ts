// Splitting a Jellyfin `People` list into the two things a detail page shows
// differently: performers, who get faces, and crew, who read as a list.

import type { Person } from "./api"

/**
 * `Type` is Jellyfin's own field and is occasionally absent, in which case a
 * name with a role is still a performance credit worth showing.
 */
export function castOf(people: Person[]) {
  return people.filter(
    (person) => person.name && (person.type === "Actor" || (!person.type && person.role)),
  )
}

/**
 * Jellyfin's `PersonKind` values are camel case — "GuestStar", not a label.
 * Splitting on the capitals is enough for every value it currently emits.
 */
function jobLabel(type: string) {
  return type.replace(/([a-z])([A-Z])/g, "$1 $2")
}

/** Directors, writers, and the rest, grouped by the job they are credited for. */
export function crewOf(people: Person[]) {
  const groups = new Map<string, string[]>()
  for (const person of people) {
    if (!person.name || !person.type || person.type === "Actor") continue
    const job = jobLabel(person.type)
    const names = groups.get(job) ?? []
    // A crew member credited twice for the same job would otherwise be listed
    // twice, which happens on imported metadata more often than it should.
    if (!names.includes(person.name)) names.push(person.name)
    groups.set(job, names)
  }
  return [...groups.entries()].map(([job, names]) => ({ job, names }))
}
