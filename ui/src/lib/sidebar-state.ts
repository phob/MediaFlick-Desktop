export function sidebarShouldBeOpen(pathname: string, pointerIsOverSidebar: boolean, userMenuIsOpen: boolean) {
  return pathname === "/" || pointerIsOverSidebar || userMenuIsOpen
}

export function sidebarShouldOverlayContent(pathname: string) {
  return pathname !== "/"
}
