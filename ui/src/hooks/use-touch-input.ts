import { useMediaQuery } from "@/hooks/use-media-query"

const TOUCH_INPUT_QUERY = "(hover: none), (pointer: coarse), (any-pointer: coarse)"

export function useTouchInput() {
  return useMediaQuery(TOUCH_INPUT_QUERY)
}
