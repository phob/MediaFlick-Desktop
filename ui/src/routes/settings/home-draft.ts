import type { HomeConfiguration, HomeElement } from "../../lib/api/types/home.ts"
import { sameJson } from "../../lib/json.ts"

export function homeElementKey(element: Pick<HomeElement, "kind" | "id">): string {
  return `${element.kind}:${element.id}`
}

export function reconcileHomeDraft(
  previous: HomeConfiguration | undefined,
  draft: HomeConfiguration | undefined,
  source: HomeConfiguration | undefined,
): HomeConfiguration | undefined {
  if (previous == null || draft == null || source == null) return source
  if (sameJson(previous, draft)) return source

  const billboard = draft.billboard !== previous.billboard ? draft.billboard : source.billboard
  const watching = {
    continueWatching: draft.watching.continueWatching !== previous.watching.continueWatching
      ? draft.watching.continueWatching
      : source.watching.continueWatching,
    nextUp: draft.watching.nextUp !== previous.watching.nextUp
      ? draft.watching.nextUp
      : source.watching.nextUp,
    combine: draft.watching.combine !== previous.watching.combine
      ? draft.watching.combine
      : source.watching.combine,
  }

  const prevByKey = new Map(previous.elements.map((element) => [homeElementKey(element), element]))
  const draftByKey = new Map(draft.elements.map((element) => [homeElementKey(element), element]))
  const sourceByKey = new Map(source.elements.map((element) => [homeElementKey(element), element]))

  const resolveElement = (sourceElement: HomeElement): HomeElement => {
    const key = homeElementKey(sourceElement)
    const draftElement = draftByKey.get(key)
    const previousElement = prevByKey.get(key)
    const enabled = draftElement && (!previousElement || draftElement.enabled !== previousElement.enabled)
      ? draftElement.enabled
      : sourceElement.enabled
    return { ...sourceElement, enabled }
  }

  const orderChanged = !sameJson(
    draft.elements.map(homeElementKey),
    previous.elements.map(homeElementKey),
  )

  if (!orderChanged) {
    return {
      billboard,
      watching,
      elements: source.elements.map(resolveElement),
    }
  }

  const seen = new Set<string>()
  const elements: HomeElement[] = []
  for (const draftElement of draft.elements) {
    const key = homeElementKey(draftElement)
    const sourceElement = sourceByKey.get(key)
    if (!sourceElement || seen.has(key)) continue
    seen.add(key)
    elements.push(resolveElement(sourceElement))
  }
  for (const sourceElement of source.elements) {
    const key = homeElementKey(sourceElement)
    if (seen.has(key)) continue
    seen.add(key)
    elements.push(resolveElement(sourceElement))
  }

  return { billboard, watching, elements }
}
