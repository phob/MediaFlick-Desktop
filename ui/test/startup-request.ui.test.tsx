import { afterEach, describe, expect, test, vi } from "vitest"
import { api, type StartupResponse } from "../src/lib/api"
import { primeStartupQueries } from "../src/lib/queries"
import { createQueryClient, queryKeys } from "../src/lib/query-client"
import { appStatus } from "./support/fixtures"

const queryClient = createQueryClient()

const status = appStatus({
  authenticated: true,
  serverUrl: "https://jellyfin.example",
  userId: "user-1",
  libraryReady: true,
})

function startup(overrides: Partial<StartupResponse> = {}): StartupResponse {
  return {
    status,
    settings: null,
    viewing: null,
    browsing: { last: "/library" },
    home: { configuration: { elements: [] } as never, continueWatching: [], rows: [] },
    billboard: { items: [] },
    ...overrides,
  }
}

afterEach(() => {
  vi.restoreAllMocks()
  queryClient.clear()
})

describe("startup request", () => {
  test("seeds every answered first-frame query from one request", async () => {
    vi.spyOn(api, "startup").mockResolvedValue(startup())
    const statusRequest = vi.spyOn(api, "status")

    await primeStartupQueries(queryClient, "/")

    expect(statusRequest).not.toHaveBeenCalled()
    expect(queryClient.getQueryData(queryKeys.status)).toEqual(status)
    expect(queryClient.getQueryData(queryKeys.home)).toEqual(startup().home)
    expect(queryClient.getQueryData(queryKeys.billboard)).toEqual({ items: [] })
    // Unanswered parts stay empty so their own queries request them.
    expect(queryClient.getQueryState(queryKeys.settings)).toBeUndefined()
  })
})
