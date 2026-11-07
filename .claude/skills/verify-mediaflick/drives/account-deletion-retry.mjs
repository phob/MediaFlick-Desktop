// A "Delete local account data" that fails halfway, then a sign-in to the same account.
//   just verify account-deletion-retry --account SERVER --signed-out
// A directory planted at accounts.json.bak makes the deletion's first step fail after its
// journal entry is written. The UI must then show the app as signed out. Signing in again
// must finish the pending deletion first, so the journal is empty and a relaunch keeps the
// new session instead of deleting the account again.
import { mkdirSync, rmSync, writeFileSync } from "node:fs"

const pendingFor = (ctx, userId) => (ctx.readConfig("pending-deletions.json")?.deletions ?? []).filter((entry) => entry.userId === userId)

export default async function accountDeletionRetry(ctx) {
  const report = {}
  const first = await ctx.signIn({})
  ctx.revokeAppSessionAfterRun()
  const firstToken = ctx.appSession()
  const userId = (await ctx.api("/api/status")).body.userId
  report.userId = userId

  const plant = ctx.configFile("accounts.json.bak")
  rmSync(plant, { recursive: true, force: true })
  mkdirSync(plant)
  writeFileSync(`${plant}/blocker`, "planted by account-deletion-retry")
  ctx.step("planted a directory at accounts.json.bak")

  // The UI polls /api/status only while a sync runs, so delete once it is idle:
  // only the delete's own failure handling can then show the app signed out.
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status.bootstrap?.state === "ready" && !status.syncing && !status.syncProgress?.active
  }, "the first sync to finish", 180000)

  try {
    await ctx.press({ role: "link", name: "Settings" })
    await ctx.press({ role: "link", name: "Application", within: { role: "navigation", name: "Settings navigation" } })
    await ctx.fill({ role: "textbox", name: "Type DELETE to confirm local account deletion" }, "DELETE")
    await ctx.press({ role: "button", name: "Delete local account data" })
    await ctx.press({ role: "button", name: "Delete local account data", within: { role: "alertdialog", name: "Delete local account data?" } })
    // The journal entry is written before the deletion fails; the UI leaves the
    // shell only once the delete request has answered with its failure.
    await ctx.find({ role: "link", name: "Back to sign in" }, { timeout: 20000 })
    ctx.step("the UI shows the app signed out after the failed deletion")
    report.pendingAfterFailure = pendingFor(ctx, userId).length
    ctx.check(report.pendingAfterFailure === 1, "the failed deletion left its journal entry")
    const status = (await ctx.api("/api/status")).body
    report.statusAfterFailure = { authenticated: status.authenticated }
    ctx.check(status.authenticated === false, "/api/status reports the half-deleted account as signed out")
    await ctx.screenshot("01-after-failed-delete")
  } finally {
    rmSync(plant, { recursive: true, force: true })
    ctx.step("removed the planted directory")
  }
  await firstToken.revoke()

  await ctx.press({ role: "link", name: "Back to sign in" })
  await ctx.signIn({})
  ctx.revokeAppSessionAfterRun()
  report.pendingAfterSignIn = pendingFor(ctx, userId).length
  ctx.check(report.pendingAfterSignIn === 0, "signing in again finished the pending deletion")
  await ctx.screenshot("02-signed-in-again")

  await ctx.relaunch()
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  const relaunched = (await ctx.api("/api/status")).body
  report.statusAfterRelaunch = { authenticated: relaunched.authenticated, userId: relaunched.userId }
  ctx.check(relaunched.authenticated === true && relaunched.userId === userId, "the relaunch keeps the new session")
  report.pendingAfterRelaunch = pendingFor(ctx, userId).length
  ctx.check(report.pendingAfterRelaunch === 0, "no deletion is pending after the relaunch")
  await ctx.screenshot("03-after-relaunch")
  ctx.writeEvidence("account-deletion-retry.json", { ...report, firstSignIn: first?.userName ?? null })
}
