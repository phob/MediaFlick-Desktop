import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { DatabaseBackup, FolderOpen } from "lucide-react"
import { toast } from "sonner"
import { CoverCard } from "@/components/CoverCard"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { api, ApiError, type RefusedLibrary } from "@/lib/api"
import { toastFailure } from "@/lib/failure-toast"
import { queryKeys } from "@/lib/query-client"

export function StartupFailure({ reason }: { reason: string }) {
  const recovery = useQuery({ queryKey: queryKeys.recovery, queryFn: api.recovery.status })
  const library = recovery.data?.library

  return (
    <CoverCard title="MediaFlick could not start" description={<span className="break-words select-text">{reason}</span>}>
      {library ? <LibraryRecovery library={library} /> : null}
    </CoverCard>
  )
}

function LibraryRecovery({ library }: { library: RefusedLibrary }) {
  const client = useQueryClient()
  const recreate = useMutation({
    mutationFn: api.recovery.recreateLibrary,
    onSuccess: ({ backup }) => {
      toast.success(`The old library.db was moved to ${backup}.`)
      void client.resetQueries()
    },
    onError: (error: Error) => {
      toastFailure(error)
      if (error instanceof ApiError && error.startupFailed) void client.resetQueries()
    },
  })
  const openFolder = useMutation({
    mutationFn: api.recovery.openDataFolder,
    onError: (error: Error) => toastFailure(error),
  })

  return (
    <div className="flex flex-col gap-2 sm:flex-row">
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="destructive" className="h-11 sm:flex-1" disabled={recreate.isPending}>
            <DatabaseBackup />
            {recreate.isPending ? "Recreating…" : "Recreate library"}
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Recreate the library?</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="flex flex-col gap-3">
                <p>MediaFlick starts on a new, empty library.db and fills it again from Jellyfin.</p>
                {library.refusal === "newerSchema" ? (
                  <p>
                    This library.db was written by a newer MediaFlick. Recreating replaces that version's library
                    too: installed again, it opens the new one. To keep using its library instead, cancel and install
                    that newer MediaFlick again.
                  </p>
                ) : null}
                <div>
                  <p>You lose what only library.db holds:</p>
                  <ul className="mt-1 list-disc pl-5">
                    <li>The saved sign-in. You sign in again, and Jellyfin lists MediaFlick as a new device.</li>
                    <li>
                      The local copy of your library, which syncs again from Jellyfin. Watched state, resume points and
                      favorites are kept on Jellyfin and come back with it.
                    </li>
                    <li>
                      Cached ratings and the titles in your collections. Refilling them spends MDBList and TMDB requests
                      through the Companion, and a collection that refreshes manually stays empty until you refresh it.
                    </li>
                  </ul>
                </div>
                <p>
                  You keep your settings, Letterboxd profiles, collections and their posters, audio and subtitle
                  choices, and reading positions.
                </p>
                <p className="break-words select-text">
                  Nothing is deleted: library.db and its -wal and -shm files are renamed with the date and time, to a
                  name like {library.backupName}, in {library.folder}.
                </p>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => recreate.mutate()}>
              Recreate library
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <Button
        variant="outline"
        className="h-11 sm:flex-1"
        disabled={openFolder.isPending}
        onClick={() => openFolder.mutate()}
      >
        <FolderOpen />
        Open data folder
      </Button>
    </div>
  )
}
