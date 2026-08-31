import { useMediaQuery } from "@/hooks/use-media-query"

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)"

/** Reactive operating-system motion preference shared by runtime and previews. */
export function usePrefersReducedMotion() {
  return useMediaQuery(REDUCED_MOTION_QUERY)
}
