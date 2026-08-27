import { useEffect, useState } from "react"

export function currentLocalDate() {
  const date = new Date()
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-")
}

/** Re-key release visibility just after the next local midnight. */
export function useLocalDate() {
  const [value, setValue] = useState(currentLocalDate)
  useEffect(() => {
    let timer = 0
    const schedule = () => {
      const now = new Date()
      const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1)
      timer = window.setTimeout(() => {
        setValue(currentLocalDate())
        schedule()
      }, Math.max(1_000, next.getTime() - now.getTime() + 1_000))
    }
    schedule()
    return () => window.clearTimeout(timer)
  }, [])
  return value
}
