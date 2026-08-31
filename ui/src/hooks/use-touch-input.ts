import { useMediaQuery } from "@/hooks/use-media-query"

const TOUCH_INPUT_QUERY = "(hover: none), (pointer: coarse), (any-pointer: coarse)"

/** Prefer a non-nested sheet when the primary input cannot reliably hover. */
export function useTouchInput() {
  return useMediaQuery(TOUCH_INPUT_QUERY)
}
