import { existsSync, writeFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"

const db = new DatabaseSync(path.join(process.env.XDG_DATA_HOME, "mediaflick-desktop", "library.db"))
db.exec("BEGIN EXCLUSIVE")
writeFileSync(process.env.MEDIAFLICK_VERIFY_HOLD_READY, "held\n")
const parent = process.ppid
const timer = setInterval(() => {
  if (!existsSync(process.env.MEDIAFLICK_VERIFY_HOLD_RELEASE) && process.ppid === parent) return
  clearInterval(timer)
  db.exec("ROLLBACK")
  db.close()
}, 100)
