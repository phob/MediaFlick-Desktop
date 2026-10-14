import type { SyncFailure } from "@/lib/api"

export const SYNC_FAILURES: Record<SyncFailure, { title: string; detail: string }> = {
  unreachable: {
    title: "Can't reach your Jellyfin server",
    detail: "You're signed in, but MediaFlick lost the connection while loading your library. Check that the server is running and that this device can reach it.",
  },
  server: {
    title: "Your Jellyfin server couldn't send your library",
    detail: "The server answered, but not with your library. It may be busy or restarting.",
  },
  storage: {
    title: "MediaFlick couldn't save your library",
    detail: "Your library could not be written on this device. Check that the disk has free space.",
  },
}
