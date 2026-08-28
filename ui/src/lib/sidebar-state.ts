export function sidebarShouldBeOpen(pathname: string, pointerIsOverSidebar: boolean) {
  return pathname === "/" || pointerIsOverSidebar
}

export function sidebarShouldOverlayContent(pathname: string) {
  return pathname !== "/"
}
