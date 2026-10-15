import { useLayoutEffect, useState } from "react"

/**
 * Where `element` starts inside `scroller`'s scrolled content, re-measured when
 * either resizes or the detail page above it grows. `null` until measured: a
 * virtualizer must not place rows before it knows this offset.
 */
export function useScrollMargin(element: HTMLElement | null, scroller: HTMLElement | null) {
  const [margin, setMargin] = useState<number | null>(null)

  useLayoutEffect(() => {
    if (!element || !scroller) return
    const measure = () => {
      const next = element.getBoundingClientRect().top
        - scroller.getBoundingClientRect().top
        + scroller.scrollTop
      setMargin((previous) => (previous === next ? previous : next))
    }

    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    observer.observe(scroller)
    const page = element.closest<HTMLElement>(".detail-page")
    if (page) observer.observe(page)
    return () => observer.disconnect()
  }, [element, scroller])

  return margin
}
