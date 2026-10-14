// A person page's Discover section, reached as a user does: library search,
// a movie, then a Cast or Crew face. Read-only.
//   Demo server (no Companion): expects the "not connected" explanation.
//   --account SERVER with the Companion and Seerr: expects Seerr's own state
//   (results or the empty text), never an error.
//   just verify person-discover
//   just verify person-discover --account https://jellyfin.example.com
const DEMO = "https://demo.jellyfin.org/stable"

const discoverText = (ctx) =>
  ctx.app.evaluate(`document.querySelector('section[aria-labelledby="cast-discover-heading"]')?.innerText ?? null`)

async function pickMovie(ctx) {
  if (!ctx.account) return { name: "Night of the Living Dead", search: "night of the living", person: "George A. Romero" }
  const { Items } = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&sortBy=SortName&fields=People&limit=100`)
  const movie = Items.find((item) => item.People?.some((person) => person.Type === "Director"))
  if (!movie) throw new Error("no movie with a director among the first 100")
  return { name: movie.Name, search: movie.Name, person: movie.People.find((person) => person.Type === "Director").Name }
}

export default async function personDiscover(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  if (ctx.account) {
    await ctx.until(async () => {
      const status = (await ctx.api("/api/status")).body
      return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
    }, "the first sync to finish", 300000)
  }
  const { name, search, person } = await pickMovie(ctx)
  ctx.step(`movie ${name}, person ${person}`)

  await ctx.fill({ role: "textbox", name: "Search the library" }, search)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${name}` }, { nth: 0, timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${name} page`)
  await ctx.press({ role: "link", name: `Find titles with ${person}` }, { nth: 0, timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?mode=person"), "the person page")
  await ctx.find({ role: "heading", name: "Discover" }, { timeout: 30000 })

  const settled = await ctx.until(async () => {
    const text = await discoverText(ctx)
    return text && !/Checking Seerr|Matching this person|Finishing the progressive/.test(text) ? text : null
  }, "the Discover section to settle", 60000)
  ctx.writeEvidence("person-discover.json", { server: ctx.account?.server ?? DEMO, movie: name, person, discover: settled })
  ctx.step(`Discover reads: ${JSON.stringify(settled)}`)
  await ctx.app.evaluate(`document.querySelector('section[aria-labelledby="cast-discover-heading"]')?.scrollIntoView({ block: "center" })`)
  await ctx.park()
  await ctx.snapshot("person-discover")
  await ctx.screenshot("person-discover")

  ctx.check(!settled.includes("unknown error"), "Discover never says unknown error")
  if (ctx.account) {
    ctx.check(!settled.includes("Seerr is unavailable") && !settled.includes("not connected"), "with the Companion's Seerr, Discover shows Seerr's own answer")
  } else {
    ctx.check(settled.includes("Seerr is not connected. Your server results above are still complete."), "without the Companion, Discover says Seerr is not connected")
    await ctx.find({ role: "link", name: "Back to Home" })
  }
}
