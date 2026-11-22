import type * as Wire from "./wire.gen.ts"

type Routes = Wire.Routes
type Route = keyof Routes
export type RouteOf<Kind> = { [K in Route]: Routes[K]["kind"] extends Kind ? K : never }[Route]

type Query = Record<string, string | number | boolean | null | undefined>

type Call<K extends Route> = { query?: Query; signal?: AbortSignal }
  & (Routes[K]["params"] extends null ? { params?: never } : { params: Routes[K]["params"] })
  & (Routes[K]["body"] extends null ? { body?: never }
    : Partial<Routes[K]["body"]> extends Routes[K]["body"] ? { body?: Routes[K]["body"] }
      : { body: Routes[K]["body"] })
export type CallArgs<K extends Route> = Partial<Call<K>> extends Call<K> ? [call?: Call<K>] : [call: Call<K>]

function queryString(params: Query) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === "") continue
    search.set(key, String(value))
  }
  const encoded = search.toString()
  return encoded ? `?${encoded}` : ""
}

export function target(route: Route, call: { params?: Readonly<Record<string, string | number>> | null; query?: Query }) {
  const space = route.indexOf(" ")
  const path = route
    .slice(space + 1)
    .replace(/\{(\w+)\}/g, (_, name: string) => encodeURIComponent(String(call.params?.[name])))
  return { method: route.slice(0, space), url: `${path}${queryString(call.query ?? {})}` }
}

export function link<K extends RouteOf<"binary">>(route: K, ...[call]: CallArgs<K>) {
  return target(route, call ?? {}).url
}
