export function sidebarShouldBeOpen(pathname: string, pointerIsOverSidebar: boolean) {
  return pathname === "/" || pointerIsOverSidebar
}
