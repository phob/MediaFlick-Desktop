import { QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, test, vi } from "vitest"
import { RequestDialog } from "../src/components/seerr/RequestDialog"
import { api, type SeerrResult, type SeerrSeason, type SeerrStatus, type SeerrStatusInfo } from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"

afterEach(() => vi.restoreAllMocks())

const series: SeerrResult = {
  mediaType: "tv",
  tmdbId: 1399,
  title: "Severance",
  year: 2022,
  overview: null,
  posterPath: null,
  backdropPath: null,
  voteAverage: null,
  status: "partial",
  status4k: "unknown",
  libraryItemId: null,
}

const status: SeerrStatusInfo = {
  linked: true,
  mapped: true,
  instance: { movie4kEnabled: false, series4kEnabled: true, partialRequestsEnabled: true },
  user: null,
  capabilities: {
    movie: { request: true, autoApprove: false },
    tv: { request: true, autoApprove: false },
    movie4k: { request: false, autoApprove: false },
    tv4k: { request: true, autoApprove: false },
    advancedRequest: false,
  },
  quota: null,
}

function season(seasonNumber: number, current: SeerrStatus): SeerrSeason {
  return { seasonNumber, name: null, episodeCount: 9, airDate: null, status: current, status4k: "unknown" }
}

function renderDialog() {
  const client = testQueryClient()
  client.setQueryData(queryKeys.seerrStatus, status)
  client.setQueryData(queryKeys.seerrMedia("tv", series.tmdbId), {
    ...series,
    seasons: [season(1, "unknown"), season(2, "available"), season(3, "unknown")],
  })
  const request = vi.spyOn(api.seerr, "request").mockResolvedValue({
    id: 1,
    status: "pending",
    mediaType: "tv",
    tmdbId: series.tmdbId,
    is4k: false,
    createdAt: null,
    updatedAt: null,
    mediaStatus: "pending",
    seasons: [3],
    libraryItemId: null,
  })
  render(
    <QueryClientProvider client={client}>
      <RequestDialog result={series} onClose={() => {}} />
    </QueryClientProvider>,
  )
  return request
}

test("season choices are checkboxes that shape the request", async () => {
  const request = renderDialog()
  const first = screen.getByRole("checkbox", { name: /Season 1/ })
  expect(screen.getByRole("checkbox", { name: /Season 2/ }).hasAttribute("disabled")).toBe(true)

  fireEvent.click(first)
  fireEvent.click(screen.getByRole("button", { name: "Request" }))
  await waitFor(() => expect(request).toHaveBeenCalledWith(expect.objectContaining({ seasons: [3], is4k: false })))
})

test("the 4K choice re-offers every season Seerr lacks in 4K", async () => {
  const request = renderDialog()
  fireEvent.click(screen.getByRole("checkbox", { name: "Request in 4K" }))
  fireEvent.click(screen.getByRole("button", { name: "Request" }))
  await waitFor(() => expect(request).toHaveBeenCalledWith(expect.objectContaining({ seasons: [1, 2, 3], is4k: true })))
})
