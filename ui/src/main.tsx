import { QueryClientProvider } from "@tanstack/react-query"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import App from "./App"
import { NavigationHistory } from "./components/NavigationHistory"
import "./app.css"
import { Toaster } from "./components/ui/sonner"
import { installAppSurfaceGuard } from "./lib/app-surface"
import { primeStartupQueries } from "./lib/queries"
import { queryClient } from "./lib/query-client"

// Browser history rather than hash routing: the scheme is registered
// STANDARD | SECURE | CORS_ENABLED | FETCH_ENABLED (src/shell/cef/mod.rs), so
// pushState has proper origin semantics, and `handle()` in
// src/shell/cef/api.rs already serves the shell for unknown non-API paths.
installAppSurfaceGuard()
const router = createBrowserRouter([{ path: "*", element: <NavigationHistory><App /></NavigationHistory> }])
// The window stays hidden behind the loading cover until the first route is
// ready, so rendering after the one startup request costs nothing visible and
// saves the queries from asking for the same data separately.
void primeStartupQueries(window.location.pathname).finally(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
        <Toaster position="bottom-right" />
      </QueryClientProvider>
    </StrictMode>,
  )
})
