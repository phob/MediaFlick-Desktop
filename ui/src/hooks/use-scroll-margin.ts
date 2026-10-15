import { useLayoutEffect, useState } from "react"

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
