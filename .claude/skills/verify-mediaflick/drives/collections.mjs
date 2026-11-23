const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const COLLECTION_SETTINGS = { role: "link", name: "Collections", within: { role: "navigation", name: "Settings navigation" } }
const TARGET = { css: "[data-verify-target]" }

async function sidebarLink(ctx, name) {
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name })
}

const today = () => new Date().toLocaleDateString("sv-SE")
const sidebarNames = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll('[data-sidebar="sidebar"] a')].map((a) => a.textContent.trim())`)
const cardCount = (ctx) => ctx.app.evaluate(`document.querySelectorAll('main a[aria-label^="Open "]').length`)
const profiles = (ctx) => ctx.readConfig("collections.json")?.accounts?.flatMap((account) => account.profiles ?? []) ?? []

const mark = (ctx, selector, text, exclude = []) => ctx.app.evaluate(`(() => {
  document.querySelectorAll("[data-verify-target]").forEach((node) => node.removeAttribute("data-verify-target"))
  const excluded = ${JSON.stringify(exclude)}
  const node = [...document.querySelectorAll(${JSON.stringify(selector)})].find((candidate) => (
    !candidate.disabled
    && candidate.textContent.includes(${JSON.stringify(text)})
    && !excluded.some((skip) => candidate.textContent.includes(skip))
  ))
  node?.setAttribute("data-verify-target", "")
  return node ? node.textContent.replace(/\\s+/g, " ").trim() : null
})()`)

const configuredTitles = (ctx) => ctx.app.evaluate(
  `[...document.querySelectorAll('main button[aria-label^="Edit "]')].map((button) => button.getAttribute("aria-label").slice("Edit ".length))`,
)

async function createTemplateCollection(ctx, excludeTitles) {
  const picked = await ctx.until(() => mark(ctx, "main button", "TMDB", excludeTitles), "a TMDB template", 20000)
  await ctx.press(TARGET)
  const addDialog = { role: "dialog", name: "Add collection" }
  await ctx.find(addDialog, { timeout: 15000 })
  const title = await ctx.app.evaluate(`document.getElementById("collection-title")?.value ?? null`)
  await ctx.press({ role: "button", name: "Save", within: addDialog })
  await ctx.until(() => ctx.app.evaluate(`/\\d+ total ·/.test(document.querySelector('[role="dialog"]')?.textContent ?? "")`), "the preview before saving", 90000)
  return { picked, title, addDialog }
}

export default async function collections(ctx) {
  await ctx.signIn({})
  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const evidence = {}
  const settings = (await ctx.api("/api/collections/settings")).body
  evidence.settings = { effectiveMode: settings.effectiveMode, modeSelection: settings.modeSelection, readiness: settings.readiness }
  ctx.check(settings.effectiveMode === "mediaFlick" && settings.readiness?.tmdb === true, "MediaFlick mode by default with TMDB ready through the Companion")
  ctx.check(settings.modeSelection === null && settings.readiness?.mdblist === true, "no mode saved yet, and MDBList ready through the Companion")
  const names = await sidebarNames(ctx)
  ctx.check(names.includes("Movie Franchises") && names.includes("My Collections") && !names.includes("Jellyfin Collections"), "the sidebar shows Movie Franchises and My Collections")

  await sidebarLink(ctx, "Movie Franchises")
  await ctx.until(async () => (await ctx.pathname()) === "/collections/franchises", "/collections/franchises")
  const franchises = await ctx.until(async () => {
    const body = (await ctx.api(`/api/collections/franchises?localDate=${today()}`)).body
    return body?.status === "ready" ? body : null
  }, "the franchise snapshot to be ready", 180000)
  const listed = await ctx.until(async () => (await cardCount(ctx)) || null, "franchise cards", 30000)
  evidence.franchises = { total: franchises.franchises?.length, cards: listed }
  ctx.check(listed === franchises.franchises.length, `one card per franchise (${listed})`)
  await ctx.park()
  await ctx.screenshot("01-franchises")
  const franchise = franchises.franchises.find((entry) => (entry.missing ?? entry.missingCount ?? 0) > 0) ?? franchises.franchises[0]
  const franchiseCard = await ctx.app.evaluate(`[...document.querySelectorAll('main a[aria-label^="Open "]')].map((a) => a.getAttribute("aria-label")).find((label) => label.includes(${JSON.stringify(franchise.title ?? franchise.name ?? "")})) ?? document.querySelector('main a[aria-label^="Open "]').getAttribute("aria-label")`)
  await ctx.press({ role: "link", name: franchiseCard })
  await ctx.find({ role: "heading", name: "Owned" }, { timeout: 30000 })
  evidence.franchiseDetail = { card: franchiseCard, missing: await ctx.exists({ role: "heading", name: "Missing" }) }
  await ctx.snapshot("02-franchise-detail")
  await ctx.screenshot("02-franchise-detail")

  await sidebarLink(ctx, "My Collections")
  await ctx.find({ role: "heading", name: "No collections yet" }, { timeout: 30000 })
  await ctx.press({ role: "link", name: "Choose templates" })
  await ctx.find({ role: "textbox", name: "Search templates" }, { timeout: 30000 })
  const template = await ctx.until(() => mark(ctx, "main button", "TMDB"), "a TMDB template", 20000)
  evidence.template = template
  await ctx.press(TARGET)
  await ctx.find({ role: "dialog", name: "Add collection" }, { timeout: 15000 })
  const title = await ctx.app.evaluate(`document.getElementById("collection-title")?.value ?? null`)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.app.evaluate(`/\\d+ total ·/.test(document.querySelector('[role="dialog"]')?.textContent ?? "")`), "the preview before saving", 90000)
  await ctx.snapshot("03-collection-preview")
  await ctx.press({ role: "button", name: "Save" })
  const saveStarted = Date.now()
  await ctx.until(() => profiles(ctx).some((profile) => profile.title === title), `collections.json to hold ${title}`, 240000)
  ctx.step(`creating the collection took ${Date.now() - saveStarted} ms`)
  evidence.created = { title, profiles: profiles(ctx).length }
  await sidebarLink(ctx, "My Collections")
  await ctx.find({ role: "link", name: `Open ${title}` }, { timeout: 60000 })
  await ctx.press({ role: "link", name: `Open ${title}` })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/collections/mine/"), "the collection's page")
  await ctx.snapshot("04-my-collection")
  await ctx.screenshot("04-my-collection")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press(COLLECTION_SETTINGS)

  const EDIT_DIALOG = { role: "dialog", name: "Edit collection" }
  const DISCARD_DIALOG = { role: "alertdialog", name: "Discard unsaved changes?" }
  const unsavedTitle = `${title} (unsaved)`
  await ctx.press({ role: "button", name: `Edit ${title}` })
  await ctx.find(EDIT_DIALOG, { timeout: 15000 })
  await ctx.fill({ role: "textbox", name: "Title", within: EDIT_DIALOG }, unsavedTitle)
  await ctx.key("Escape")
  const escapeOutcome = await ctx.until(async () => {
    if (await ctx.exists(DISCARD_DIALOG)) return "prompt"
    if (!(await ctx.exists(EDIT_DIALOG))) return "closed"
    return false
  }, "Discard unsaved changes? dialog after pressing Escape on a dirty Edit collection wizard", 10000)
  ctx.check(
    escapeOutcome === "prompt",
    "Escape on a dirty Edit collection wizard opens Discard unsaved changes? instead of closing immediately",
  )
  await ctx.snapshot("04a-wizard-discard-prompt")
  await ctx.screenshot("04a-wizard-discard-prompt")
  await ctx.press({ role: "button", name: "Keep editing", within: DISCARD_DIALOG })
  await ctx.until(async () => !(await ctx.exists(DISCARD_DIALOG)), "Discard unsaved changes? dialog to close on Keep editing")
  await ctx.find(EDIT_DIALOG, { timeout: 10000 })
  const keptTitle = await ctx.app.evaluate(`document.getElementById("collection-title")?.value ?? null`)
  ctx.check(keptTitle === unsavedTitle, "Keep editing preserves the edited collection title")
  await ctx.fill({ role: "textbox", name: "Description", within: EDIT_DIALOG }, "Still editable after keeping edits")
  const keptDescription = await ctx.app.evaluate(`document.getElementById("collection-description")?.value ?? null`)
  ctx.check(
    keptDescription === "Still editable after keeping edits",
    "focus and typing work in the reopened Edit collection wizard after Keep editing",
  )
  await ctx.snapshot("04b-wizard-kept-editing")
  await ctx.screenshot("04b-wizard-kept-editing")
  await ctx.key("Escape")
  await ctx.find(DISCARD_DIALOG, { timeout: 10000 })
  await ctx.press({ role: "button", name: "Discard changes", within: DISCARD_DIALOG })
  await ctx.until(
    async () => !(await ctx.exists(DISCARD_DIALOG)) && !(await ctx.exists(EDIT_DIALOG)),
    "Discard changes to close both dialogs",
  )
  ctx.check(
    profiles(ctx).some((profile) => profile.title === title) && !profiles(ctx).some((profile) => profile.title === unsavedTitle),
    "discarding dirty wizard edits leaves the saved collection title unchanged in collections.json",
  )
  await ctx.screenshot("04c-wizard-discarded")
  evidence.wizardDiscardGuard = { keptTitle, keptDescription, savedTitles: profiles(ctx).map((profile) => profile.title) }

  const second = await createTemplateCollection(ctx, [title, "Custom"])
  await ctx.press({ role: "button", name: "Save", within: second.addDialog })
  await ctx.until(() => profiles(ctx).some((profile) => profile.title === second.title), `collections.json to hold ${second.title}`, 240000)
  await ctx.until(async () => !(await ctx.exists(second.addDialog)), "Add collection dialog to close after creating second collection")

  await ctx.press({ role: "button", name: `Move ${title} down` })
  await ctx.until(async () => {
    const order = await configuredTitles(ctx)
    return order[0] === second.title && order[1] === title
  }, "reordered draft before adding a third collection")

  const third = await createTemplateCollection(ctx, [title, second.title, "Custom"])
  await ctx.press({ role: "button", name: "Save", within: third.addDialog })
  await ctx.until(() => profiles(ctx).some((profile) => profile.title === third.title), `collections.json to hold ${third.title}`, 240000)
  await ctx.until(async () => !(await ctx.exists(third.addDialog)), "Add collection dialog to close after creating third collection")

  const expectedOrder = [second.title, title, third.title]
  await ctx.until(async () => {
    const order = await configuredTitles(ctx)
    return order.length >= 2
  }, "configured collections list after creating third collection")
  await ctx.snapshot("04d-reorder-draft-after-create")
  await ctx.screenshot("04d-reorder-draft-after-create")
  const renderedOrder = await configuredTitles(ctx)
  ctx.check(
    renderedOrder.length === 3
      && renderedOrder[0] === expectedOrder[0]
      && renderedOrder[1] === expectedOrder[1]
      && renderedOrder[2] === expectedOrder[2],
    `creating a collection while reorder draft is dirty keeps the new collection visible in order (${renderedOrder.join(", ")}; expected ${expectedOrder.join(", ")})`,
  )

  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => {
    const saved = profiles(ctx).map((profile) => profile.title)
    return saved.length === 3
      && saved[0] === expectedOrder[0]
      && saved[1] === expectedOrder[1]
      && saved[2] === expectedOrder[2]
  }, "collections.json to save the rebased collection order", 30000)
  await ctx.until(() => ctx.app.evaluate(
    `![...document.querySelectorAll('[role="status"]')].some((node) => node.textContent.includes("You have unsaved changes."))`,
  ), "unsaved changes notice to clear after saving rebased collection order", 10000)
  const modeSelectionAfterReorderSave = ctx.readConfig("collections.json")?.accounts?.[0]?.modeSelection ?? null
  ctx.check(modeSelectionAfterReorderSave === null, "saving a rebased collection reorder leaves automatic modeSelection unpinned")
  await ctx.snapshot("04e-reorder-draft-saved")
  await ctx.screenshot("04e-reorder-draft-saved")
  evidence.reorderDraftAdd = {
    beforeTitles: [title, second.title],
    createdTitle: third.title,
    renderedOrder,
    savedOrder: profiles(ctx).map((profile) => profile.title),
    modeSelection: modeSelectionAfterReorderSave,
  }

  await ctx.choose({ name: "Collection mode" }, "Jellyfin")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("collections.json")?.accounts?.some((account) => account.modeSelection === "jellyfin"), "modeSelection jellyfin in collections.json")
  await ctx.until(async () => {
    const now = await sidebarNames(ctx)
    return now.includes("Jellyfin Collections") && !now.includes("Movie Franchises")
  }, "the sidebar to switch to Jellyfin Collections")
  await sidebarLink(ctx, "Jellyfin Collections")
  const boxsets = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=BoxSet&limit=0`)).TotalRecordCount
  const shown = await ctx.until(async () => {
    const count = await cardCount(ctx)
    return count === boxsets ? count : null
  }, `one card for each of the ${boxsets} BoxSets`, 30000)
  evidence.jellyfin = { boxsets, rendered: shown }
  await ctx.park()
  await ctx.screenshot("05-jellyfin-collections")
  await ctx.app.route("/collections/franchises")
  await ctx.until(async () => (await ctx.pathname()) === "/collections/jellyfin", "/collections/franchises redirects to /collections/jellyfin")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press(COLLECTION_SETTINGS)
  for (const name of expectedOrder) {
    await ctx.press({ role: "button", name: `Delete ${name}` })
  }
  await ctx.choose({ name: "Collection mode" }, "MediaFlick")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => profiles(ctx).length === 0 && ctx.readConfig("collections.json")?.accounts?.every((account) => account.modeSelection !== "jellyfin"), "collections.json without the collections, in MediaFlick mode")
  ctx.writeEvidence("collections.json", evidence)
}
