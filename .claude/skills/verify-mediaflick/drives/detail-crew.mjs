// A movie's Crew rail against Jellyfin's public demo server, read-only: the
// rail's headshot links, one entry for a writer-director, no crew lines left in
// Details, then a crew face opens the person page with the titles they directed.
//   just verify detail-crew
const DEMO = "https://demo.jellyfin.org/stable"
const MOVIE = "Night of the Living Dead"
const DIRECTOR = "George A. Romero"

// The rail's text is not in the accessibility tree as one node; read it whole.
const sectionText = (ctx, heading) =>
  ctx.app.evaluate(`[...document.querySelectorAll("section")].find((section) => section.querySelector("h2")?.textContent === ${JSON.stringify(heading)})?.innerText ?? null`)

export default async function detailCrew(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")

  await ctx.fill({ role: "textbox", name: "Search the library" }, "night of the living")
  await ctx.until(async () => (await ctx.pathname()) === "/library?search=night%20of%20the%20living", "the search route")
  await ctx.press({ role: "link", name: `Open details for ${MOVIE}` }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${MOVIE} page`)
  await ctx.find({ role: "heading", name: MOVIE })

  await ctx.find({ role: "heading", name: "Cast" }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: "Crew" }, { timeout: 30000 })
  const tree = await ctx.snapshot("01-movie")
  const crewLinks = [...tree.matchAll(/link 'Find titles with ([^']+)'/g)].map((match) => match[1])
  ctx.step(`people links: ${JSON.stringify(crewLinks)}`)
  await ctx.find({ role: "link", name: `Find titles with ${DIRECTOR}` })

  const crew = await sectionText(ctx, "Crew")
  ctx.step(`crew rail: ${JSON.stringify(crew)}`)
  ctx.check(crew?.split(DIRECTOR).length === 2, `${DIRECTOR} appears once in the Crew rail`)
  ctx.check(crew?.includes(`${DIRECTOR}\nDirector, Writer`), `${DIRECTOR} is labelled Director, Writer`)
  const labels = await ctx.app.evaluate(`[...document.querySelectorAll("dt")].map((label) => label.textContent)`)
  ctx.step(`fact labels: ${JSON.stringify(labels)}`)
  ctx.check(!labels.some((label) => ["Director", "Writer", "Producer"].includes(label)), "the Details panel no longer lists crew jobs")

  await ctx.app.evaluate(`[...document.querySelectorAll("h2")].find((heading) => heading.textContent === "Crew")?.scrollIntoView({ block: "center" })`)
  // Headshots load lazily, so the scroll above is what starts this one.
  await ctx.until(`(() => { const image = document.querySelector('a[aria-label=${JSON.stringify(`Find titles with ${DIRECTOR}`)}] img'); return image?.complete && image.naturalWidth > 0 })()`, `${DIRECTOR}'s headshot to load`)
  await ctx.screenshot("01-crew-rail")

  await ctx.press({ role: "link", name: `Find titles with ${DIRECTOR}` })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?mode=person"), "the person page")
  ctx.step(`person route: ${await ctx.pathname()}`)
  await ctx.find({ role: "heading", name: DIRECTOR })
  await ctx.find({ role: "link", name: `Open details for ${MOVIE}` }, { timeout: 30000 })
  const person = await ctx.snapshot("02-person")
  const titles = [...person.matchAll(/link 'Open details for ([^']+)'/g)].map((match) => match[1])
  ctx.step(`person titles: ${JSON.stringify(titles)}`)
  ctx.check(titles.length >= 1, `the person page lists ${titles.length} title(s) for a crew-only credit`)
  await ctx.screenshot("02-person")
}
