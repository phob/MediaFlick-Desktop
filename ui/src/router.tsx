import type { ReactNode } from "react"
import type { RouteObject } from "react-router-dom"
import { RootErrorScreen } from "@/components/RootErrorScreen"

export function createAppRoutes(
  element: ReactNode,
  errorElement: ReactNode = <RootErrorScreen />,
): RouteObject[] {
  return [{ path: "*", element, errorElement }]
}
