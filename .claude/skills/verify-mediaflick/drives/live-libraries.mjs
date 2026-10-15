// Every library the app serves live, against Jellyfin's own answers: each
// offered sort, and favorites where offered, over two pages; the refusal of
// every sort and filter it does not offer; empty filter options; and Latest
// against the Jellyfin query the library's profile names. On the demo server
// the run signs in through a loopback proxy and also proves that a dropped or
// failing live request answers an error, never an empty page. Read-only.
//   just verify live-libraries
//   just verify live-libraries --account https://jellyfin.example.com
const DEMO = "https://demo.jellyfin.org/stable"
const SHAPES = {
  music: { types: ["MusicAlbum"], latest: "grouped" },
  musicVideos: { types: ["MusicVideo"], latest: "grouped" },
  books: { types: ["Book", "AudioBook"], latest: "grouped" },
  boxSets: { types: ["BoxSet"], latest: "newest" },
  trailers: { types: ["Trailer"], latest: "grouped" },
  homeVideos: { types: null, latest: "grouped" },
  photos: { types: null, latest: "grouped" },
  playlists: { types: null, latest: "newest" },
  liveTv: { types: null, latest: "newest" },
  mixed: { types: null, latest: "newest" },
}
const SORTS = {
  name: { sortBy: "SortName", order: "Ascending" },
  added: { sortBy: "DateCreated,SortName", order: "Descending" },
  year: { sortBy: "ProductionYear,PremiereDate,SortName", order: "Descending" },
}
const UNOFFERED = ["sort=random&seed=7", "sort=rating", "sort=episode-added", "sort=plays", "watched=unwatched", "genre=Rock", "decade=1990", "parental=PG", "seriesStatus=Ended", "favorite=false"]
const PAGE = 24
const LATEST_LIMIT = 20
const IDS_ONLY = "Fields=&EnableImages=false&EnableUserData=false"

const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)
const shapeQuery = (library) => {
  const { types } = SHAPES[library.kind]
  return types ? `&Recursive=true&IncludeItemTypes=${types.join(",")}` : ""
}

async function signInThrough(ctx, proxy) {
  await ctx.find({ role: "button", name: "Sign in" }, { timeout: 30000 })
  await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
  await ctx.fill({ role: "textbox", name: "Username" }, "demo")
  await ctx.press({ role: "button", name: "Sign in" })
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
}

async function serverPage(ctx, userId, library, { sort, favorite, offset, limit }) {
  const { sortBy, order } = SORTS[sort]
  const page = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${library.id}${shapeQuery(library)}${favorite ? "&Filters=IsFavorite" : ""}&SortBy=${sortBy}&SortOrder=${order}&StartIndex=${offset}&Limit=${limit}&${IDS_ONLY}`)
  return { total: page.TotalRecordCount, ids: page.Items.map((item) => item.Id) }
}

async function appPage(ctx, library, { sort, favorite, offset, limit }) {
  const response = await ctx.api(`/api/libraries/${library.id}/items?sort=${sort}${favorite ? "&favorite=true" : ""}&offset=${offset}&limit=${limit}`)
  if (response.status !== 200) return { status: response.status, error: response.body?.error ?? null, ids: [] }
  return { status: 200, total: response.body.total, ids: response.body.items.map((item) => item.id) }
}

async function gridCase(ctx, expect, userId, library, sort, favorite) {
  const label = `${library.name} (${library.kind}) sort=${sort}${favorite ? " favorites" : ""}`
  const probe = await serverPage(ctx, userId, library, { sort, favorite, offset: 0, limit: 1 })
  const limit = Math.max(1, Math.min(PAGE, Math.ceil(probe.total / 2)))
  const pages = []
  for (const offset of [0, limit]) {
    const app = await appPage(ctx, library, { sort, favorite, offset, limit })
    const server = await serverPage(ctx, userId, library, { sort, favorite, offset, limit })
    pages.push({ offset, limit, app, server })
    expect(app.status === 200, `${label} offset ${offset}: answers 200 (${app.status} ${app.error ?? ""})`)
    expect(app.total === server.total, `${label} offset ${offset}: total ${app.total} equals Jellyfin's TotalRecordCount ${server.total}`)
    expect(sameList(app.ids, server.ids), `${label} offset ${offset}: the same ${server.ids.length} ids in Jellyfin's order (app ${app.ids.length})`)
  }
  const [first, second] = pages.map((page) => page.app.ids)
  const shared = first.filter((id) => second.includes(id))
  expect(shared.length === 0, `${label}: its two pages share no item (${shared.length} shared)`)
  expect(first.length + second.length === Math.min(probe.total, 2 * limit), `${label}: two pages of ${limit} hold ${first.length + second.length} of ${probe.total}`)
  return { sort, favorite, total: probe.total, limit, pages }
}

async function latestCase(ctx, expect, userId, library) {
  const { latest } = SHAPES[library.kind]
  const app = await ctx.api(`/api/libraries/${library.id}/latest?limit=${LATEST_LIMIT}`)
  const server = latest === "grouped"
    ? await ctx.jellyfin(`/Items/Latest?userId=${userId}&ParentId=${library.id}&Limit=${LATEST_LIMIT}&${IDS_ONLY}`)
    : (await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${library.id}${shapeQuery(library)}&SortBy=DateCreated,SortName&SortOrder=Descending&Limit=${LATEST_LIMIT}&${IDS_ONLY}`)).Items
  const appIds = (app.body?.items ?? []).map((item) => item.id)
  const serverIds = server.map((item) => item.Id)
  const query = latest === "grouped" ? "/Items/Latest" : "/Items newest first"
  expect(app.status === 200 && sameList(appIds, serverIds), `${library.name} (${library.kind}): latest lists the same ${serverIds.length} ids in order as ${query} (app ${app.status}, ${appIds.length} ids)`)
  return { query, status: app.status, appIds, serverIds, types: [...new Set(server.map((item) => item.Type))] }
}

async function refusals(ctx, expect, library) {
  const offered = library.browse.filters.includes("favorite")
  const cases = offered ? UNOFFERED : [...UNOFFERED, "favorite=true"]
  const answers = {}
  for (const query of cases) answers[query] = (await ctx.api(`/api/libraries/${library.id}/items?${query}`)).status
  const accepted = Object.entries(answers).filter(([, status]) => status !== 400)
  expect(accepted.length === 0, `${library.name} (${library.kind}): every sort and filter it does not offer answers 400 (${JSON.stringify(accepted)})`)
  const letters = (await ctx.api(`/api/libraries/${library.id}/letters`)).status
  expect(letters === 400, `${library.name} (${library.kind}): letters answer 400 (${letters})`)
  return { ...answers, letters }
}

async function failures(ctx, expect, library, setFailure) {
  const result = {}
  for (const mode of ["drop", "error"]) {
    setFailure(mode)
    const answer = await ctx.api(`/api/libraries/${library.id}/items?limit=5`)
    setFailure(null)
    result[mode] = { status: answer.status, error: answer.body?.error ?? null, items: answer.body?.items ?? null }
    expect(answer.status >= 500 && !Array.isArray(answer.body?.items), `${library.name}: a ${mode === "drop" ? "dropped" : "failing (HTTP 500)"} Jellyfin request answers ${answer.status} with an error, not an empty page (${JSON.stringify(result[mode])})`)
  }
  const healed = await ctx.api(`/api/libraries/${library.id}/items?limit=5`)
  expect(healed.status === 200, `${library.name}: the grid answers 200 again once the server answers (${healed.status})`)
  return result
}

export default async function liveLibraries(ctx) {
  let failure = null
  let failTarget = null
  const liveRequest = (url) => url.pathname === "/Items" && url.searchParams.get("parentId") === failTarget
  const proxy = ctx.account
    ? null
    : await ctx.proxy({
      upstream: DEMO,
      fail: (url) => failure === "drop" && liveRequest(url),
      answer: (url) => (failure === "error" && liveRequest(url) ? { status: 500, body: { error: "verify" } } : null),
    })
  if (proxy) await signInThrough(ctx, proxy)
  else await ctx.signIn({})
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo through a loopback proxy", checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }

  const { userId } = (await ctx.api("/api/status")).body
  const listed = (await ctx.api("/api/libraries")).body
  const live = listed.libraries.filter((library) => SHAPES[library.kind])
  report.libraries = listed.libraries.map(({ id, name, kind, browse }) => ({ id, name, kind, browse }))
  report.liveKindsNotOnServer = Object.keys(SHAPES).filter((kind) => !live.some((library) => library.kind === kind))
  expect(live.length > 0, `${live.length} live libraries listed (${live.map((library) => `${library.name}:${library.kind}`).join(", ")})`)

  report.live = []
  for (const library of live) {
    const recursive = Boolean(SHAPES[library.kind].types)
    const { sorts, filters, letterIndex } = library.browse
    expect(
      sameList(sorts, ["name", "added", "year"]) && sameList(filters, recursive ? ["favorite"] : []) && letterIndex === false,
      `${library.name} (${library.kind}): offers name, added and year sorts, ${recursive ? "the favorites filter" : "no filter"} and no A-Z (${JSON.stringify(library.browse)})`,
    )
    const entry = { id: library.id, name: library.name, kind: library.kind, browse: library.browse, grids: [] }
    for (const sort of library.browse.sorts) {
      entry.grids.push(await gridCase(ctx, expect, userId, library, sort, false))
      if (library.browse.filters.includes("favorite")) entry.grids.push(await gridCase(ctx, expect, userId, library, sort, true))
    }
    entry.refusals = await refusals(ctx, expect, library)
    const options = await ctx.api(`/api/libraries/${library.id}/options`)
    entry.options = { status: options.status, body: options.body }
    expect(options.status === 200 && sameList(options.body, { genres: [], officialRatings: [] }), `${library.name} (${library.kind}): options answer empty lists (${options.status} ${JSON.stringify(options.body)})`)
    entry.latest = await latestCase(ctx, expect, userId, library)
    report.live.push(entry)
  }

  if (proxy && live.length) {
    const target = live[0]
    failTarget = target.id
    report.failures = { library: target.name, ...(await failures(ctx, expect, target, (mode) => { failure = mode })) }
  } else {
    ctx.step("server failures are only exercised on the demo server, through the proxy")
  }

  ctx.writeEvidence("live-libraries.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
