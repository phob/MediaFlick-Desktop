// Holds library.db open with a write lock, as another program would, for the
// 0.2.0 upgrade drive's 6-in-use scenario. The file is
// $XDG_DATA_HOME/mediaflick-desktop/library.db; nothing on the command line
// names the profile. It writes MEDIAFLICK_VERIFY_HOLD_READY once the lock is
// held, and lets go when MEDIAFLICK_VERIFY_HOLD_RELEASE appears or when the
// process that started it (the app it was launched beside) is gone.
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
