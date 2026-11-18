import type { QueryClient } from "@tanstack/react-query"
import { api } from "./api"
import { queryKeys } from "./query-client"
import { accountKey } from "./session-queries"
import { browsingQueryOptions, viewingQueryOptions } from "./settings-queries"

/**
 * Seeds the queries the first frame reads from one `/api/startup` request, so
 * they need not wait on status one round trip at a time. Anything missing or
 * failed is left to its own query.
 */
export async function primeStartupQueries(client: QueryClient, pathname: string) {
  let startup
  try {
    startup = await api.startup(pathname === "/")
  } catch {
    return
  }
  const { status } = startup
  client.setQueryData(queryKeys.status, status)
  client.setQueryData(queryKeys.settings, startup.settings)
  const account = accountKey(status)
  if (startup.viewing) client.setQueryData(viewingQueryOptions(account).queryKey, startup.viewing)
  if (startup.browsing) client.setQueryData(browsingQueryOptions(account).queryKey, startup.browsing)
  if (startup.home) client.setQueryData(queryKeys.home, startup.home)
  if (startup.billboard) client.setQueryData(queryKeys.billboard, startup.billboard)
}
