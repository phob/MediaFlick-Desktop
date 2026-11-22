import { QueryClientProvider } from "@tanstack/react-query"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import App from "./App"
import { NavigationHistory } from "./components/NavigationHistory"
import "./app.css"
import { Toaster } from "./components/ui/sonner"
import { installAppSurfaceGuard } from "./lib/app-surface"
import { listenForLibrariesChangedBeforeBridge } from "./lib/library-events"
import { primeStartupQueries } from "./lib/startup-queries"
import { createQueryClient } from "./lib/query-client"

installAppSurfaceGuard()
const router = createBrowserRouter([{ path: "*", element: <NavigationHistory><App /></NavigationHistory> }])
const queryClient = createQueryClient()
listenForLibrariesChangedBeforeBridge()
const root = document.getElementById("root")
if (!root) throw new Error("index.html has no #root element")
void primeStartupQueries(queryClient, window.location.pathname).finally(() => {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
        <Toaster position="bottom-right" />
      </QueryClientProvider>
    </StrictMode>,
  )
})
