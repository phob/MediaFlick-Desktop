/** What the reader can be asked to do, by a key or by a panel. */
export type ReaderCommand =
  | { kind: "turn"; to: "left" | "right" | "next" | "prev" }
  | { kind: "scroll"; by: -1 | 1 }
  | { kind: "section"; to: "next" | "prev" }
  | { kind: "edge"; to: "start" | "end" }
  | { kind: "return" }
  | { kind: "font"; step: -1 | 1 }
  | { kind: "open"; layer: Panel | "menu" }
  | { kind: "back" }
  /** Moves the focus inside a panel: a row, ten rows, or to an end. */
  | { kind: "move"; to: "previous" | "next" | "previousTen" | "nextTen" | "first" | "last" }

export type Panel = "toc" | "goto" | "settings" | "keys"

/**
 * What has the keyboard. A panel remembers whether it was opened from the
 * menu, so Back returns there with the panel's own entry focused, or from
 * reading, so Back returns to the book.
 */
export type ReaderLayer =
  | { kind: "reading" }
  | { kind: "menu"; focus: MenuEntry }
  | { kind: Panel; from: "reading" | "menu" }

type MenuEntry = Panel | "close"

export type LayerKind = ReaderLayer["kind"]

/** The next layer, or `"leave"` when Back leaves the reader. */
export function nextLayer(
  layer: ReaderLayer,
  change: { kind: "open"; layer: Panel | "menu" } | { kind: "back" } | { kind: "jumped" },
): ReaderLayer | "leave" {
  switch (change.kind) {
    case "jumped":
      return { kind: "reading" }
    case "open":
      if (change.layer === "menu") return { kind: "menu", focus: "toc" }
      return { kind: change.layer, from: layer.kind === "menu" ? "menu" : "reading" }
    case "back":
      if (layer.kind === "reading") return "leave"
      if (layer.kind === "menu") return { kind: "reading" }
      return layer.from === "menu" ? { kind: "menu", focus: layer.kind } : { kind: "reading" }
  }
}
