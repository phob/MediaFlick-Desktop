import { afterAll, afterEach, describe, expect, test } from "vitest"
import { installAppSurfaceGuard } from "../src/lib/app-surface"

describe("app surface guard", () => {
  const cleanup = installAppSurfaceGuard()

  afterAll(() => {
    cleanup()
  })

  afterEach(() => {
    document.body.innerHTML = ""
  })

  function press(key: string, init: KeyboardEventInit = {}, target: EventTarget = document.body) {
    const event = new KeyboardEvent("keydown", { cancelable: true, key, ...init })
    target.dispatchEvent(event)
    return event
  }

  function drag(type: string) {
    // jsdom has no DragEvent; cancelability is all the guard contract needs.
    const event = new Event(type, { cancelable: true })
    document.body.dispatchEvent(event)
    return event
  }

  test("select-all is blocked outside editable controls", () => {
    expect(press("a", { ctrlKey: true }).defaultPrevented).toBe(true)
    expect(press("A", { ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(true)
    expect(press("a", { metaKey: true }).defaultPrevented).toBe(true)
  })

  test("select-all keeps working inside editable controls", () => {
    const input = document.createElement("input")
    const textarea = document.createElement("textarea")
    const editable = document.createElement("div")
    editable.setAttribute("contenteditable", "true")
    document.body.append(input, textarea, editable)

    for (const target of [input, textarea, editable]) {
      expect(press("a", { ctrlKey: true }, target).defaultPrevented).toBe(false)
    }
  })

  test("caret browsing and reload cannot be triggered", () => {
    expect(press("F7").defaultPrevented).toBe(true)
    expect(press("F5").defaultPrevented).toBe(true)
    expect(press("r", { ctrlKey: true }).defaultPrevented).toBe(true)
  })

  test("document shortcuts are blocked everywhere", () => {
    for (const key of ["p", "s", "u"]) {
      expect(press(key, { ctrlKey: true }).defaultPrevented).toBe(true)
    }
  })

  test("page zoom is pinned", () => {
    for (const key of ["-", "=", "+", "0"]) {
      expect(press(key, { ctrlKey: true }).defaultPrevented).toBe(true)
    }
    const wheel = new WheelEvent("wheel", { cancelable: true, ctrlKey: true, deltaY: -4 })
    document.body.dispatchEvent(wheel)
    expect(wheel.defaultPrevented).toBe(true)
  })

  test("dragging files or links onto the shell does nothing", () => {
    for (const type of ["dragenter", "dragover", "drop"]) {
      expect(drag(type).defaultPrevented).toBe(true)
    }
  })

  test("ordinary typing and scrolling are untouched", () => {
    expect(press("a").defaultPrevented).toBe(false)
    expect(press("-", { ctrlKey: false }).defaultPrevented).toBe(false)
    const wheel = new WheelEvent("wheel", { cancelable: true, deltaY: 120 })
    document.body.dispatchEvent(wheel)
    expect(wheel.defaultPrevented).toBe(false)
  })
})
