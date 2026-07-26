import { Film, LogOut, RefreshCw } from "lucide-react"
import type { ReactNode } from "react"
import { Link, useNavigate } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { api } from "@/lib/api"
import { useLogout } from "@/lib/queries"
import { useState } from "react"
import { toast } from "sonner"

export function AppShell({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const logout = useLogout()
  const [search, setSearch] = useState("")

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-header shrink-0 items-center gap-3 border-b px-4">
        <Link to="/" className="flex items-center gap-2 font-medium">
          <Film className="size-5 text-primary" />
          MediaFlick
        </Link>
        <Link to="/library?kind=Movie" className="text-sm text-muted-foreground hover:text-foreground">
          Movies
        </Link>
        <Link to="/library?kind=Series" className="text-sm text-muted-foreground hover:text-foreground">
          Series
        </Link>
        <form
          className="ml-auto"
          onSubmit={(event) => {
            event.preventDefault()
            if (search.trim()) navigate(`/library?search=${encodeURIComponent(search.trim())}`)
          }}
        >
          <Input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Search…"
            className="w-64"
          />
        </form>
        <Button
          variant="ghost"
          size="icon"
          title="Sync library"
          onClick={() => {
            void api
              .sync()
              .then(() => toast.success("Sync requested"))
              .catch((error: Error) => toast.error(error.message))
          }}
        >
          <RefreshCw className="size-4" />
        </Button>
        <Button variant="ghost" size="icon" title="Sign out" onClick={() => logout.mutate()}>
          <LogOut className="size-4" />
        </Button>
      </header>
      <main className="min-h-0 flex-1 overflow-y-auto">{children}</main>
    </div>
  )
}
