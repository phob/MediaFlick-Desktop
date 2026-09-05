import { fireEvent, render, screen } from "@testing-library/react"
import { expect, test } from "vitest"
import { SubtitlePreview } from "@/components/SubtitlePreview"
import { DEFAULT_COMFORT } from "@/lib/viewing"

test("scene changes preserve the current subtitle draft", () => {
  const comfort = { ...DEFAULT_COMFORT, subtitleOutline: 8, subtitleBackground: 60 }
  const { rerender } = render(<SubtitlePreview comfort={comfort} />)
  fireEvent.click(screen.getByRole("button", { name: "Night" }))
  expect(screen.getByRole("button", { name: "Night" }).getAttribute("aria-pressed")).toBe("true")
  expect(screen.getByRole("button", { name: "Day" }).getAttribute("aria-pressed")).toBe("false")
  expect(screen.getByText("Somewhere beyond").style.backgroundColor).toBe("rgba(0, 0, 0, 0.6)")
  rerender(<SubtitlePreview comfort={{ ...comfort, subtitleBackground: 0 }} />)
  expect(screen.getByText("Somewhere beyond").style.backgroundColor).toBe("rgba(0, 0, 0, 0)")
  expect(screen.getByRole("button", { name: "Night" }).getAttribute("aria-pressed")).toBe("true")
})
