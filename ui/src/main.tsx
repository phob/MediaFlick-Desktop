import { QueryClientProvider } from "@tanstack/react-query"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import App from "./App"
import "./app.css"
import { Toaster } from "./components/ui/sonner"
import { installAppSurfaceGuard } from "./lib/app-surface"
import { queryClient } from "./lib/query-client"

// `BrowserRouter` rather than hash routing: the scheme is registered
// STANDARD | SECURE | CORS_ENABLED | FETCH_ENABLED (src/shell/cef/mod.rs), so
// pushState has proper origin semantics, and `handle()` in
// src/shell/cef/api.rs already serves the shell for unknown non-API paths.
installAppSurfaceGuard()
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
        <Toaster position="bottom-right" />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)
