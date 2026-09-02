import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, test, vi } from "vitest"
import { queryKeys } from "@/lib/query-client"
import Calendar from "@/routes/Calendar"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

function isoDate(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${year}-${month}-${day}`
}

function dateFromToday(days: number) {
  const date = new Date()
  date.setDate(date.getDate() + days)
  return isoDate(date)
}

function currentCalendarWindow() {
  const now = new Date()
  const gridStart = new Date(now.getFullYear(), now.getMonth(), 1)
  gridStart.setDate(gridStart.getDate() - ((gridStart.getDay() + 6) % 7))
  const gridEnd = new Date(gridStart)
  gridEnd.setDate(gridEnd.getDate() + 41)
  return { start: isoDate(gridStart), end: isoDate(gridEnd) }
}

function renderCalendar() {
  const client = testQueryClient()
  const window = currentCalendarWindow()
  client.setQueryData(queryKeys.calendar(window.start, window.end), {
    entries: [
      {
        kind: "movie",
        date: dateFromToday(1),
        dateKind: "cinema",
        title: "Tomorrow in cinemas",
        seriesTitle: null,
        season: null,
        episode: null,
        tmdbId: 700,
        tvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
      },
    ],
    refreshedAt: null,
    sources: {},
    windowStart: window.start,
    windowEnd: window.end,
    provider: "plugin",
  })
  client.setQueryData(queryKeys.seerrStatus, {
    configured: false,
    linked: false,
    expired: false,
    serverUrl: null,
    instance: { movie4kEnabled: false, series4kEnabled: false, partialRequestsEnabled: false },
    user: null,
    capabilities: null,
    quota: null,
  })
  return render(
    <TestProviders client={client}>
        <Calendar />
    </TestProviders>,
  )
}

describe("release calendar Today navigation", () => {
  test("jumps to today's agenda position when the page opens", async () => {
    const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView")
    renderCalendar()

    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "auto", block: "start" }))
    const target = scrollIntoView.mock.instances[0]
    if (!(target instanceof HTMLElement)) throw new Error("Expected the agenda scroll target")
    expect(target.dataset.calendarDate).toBe(dateFromToday(0))

    scrollIntoView.mockRestore()
  })

  test("jumps to today's month cell even when the current month is already selected", async () => {
    const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView")
    const view = renderCalendar()
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled())

    fireEvent.mouseDown(screen.getByRole("tab", { name: "Month" }), { button: 0, ctrlKey: false })
    await waitFor(() => {
      expect(view.container.querySelector(`[data-calendar-date="${dateFromToday(0)}"].min-h-32`)).toBeTruthy()
    })
    scrollIntoView.mockClear()
    fireEvent.click(screen.getByRole("button", { name: "Today" }))

    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" }))
    const target = scrollIntoView.mock.instances[0]
    if (!(target instanceof HTMLElement)) throw new Error("Expected the month scroll target")
    expect(target.dataset.calendarDate).toBe(dateFromToday(0))
    expect(target.classList.contains("min-h-32")).toBe(true)

    scrollIntoView.mockRestore()
  })
})
