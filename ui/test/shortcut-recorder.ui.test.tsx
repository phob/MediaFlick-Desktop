import { useState } from "react"
import { fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, test, vi } from "vitest"
import { ShortcutRecorder } from "@/components/ShortcutRecorder"

afterEach(() => vi.restoreAllMocks())

function Recorder() {
  const [value, setValue] = useState("w")
  return <ShortcutRecorder id="shortcut" label="Mark watched key" value={value} onChange={setValue} />
}

test("captures a combination without bubbling playback events, and clears it", () => {
  const playback = vi.fn()
  window.addEventListener("keydown", playback)
  try {
    render(<Recorder />)
    const recorder = screen.getByRole("button", { name: "Mark watched key" })
    fireEvent.click(recorder)
    fireEvent.keyDown(recorder, { key: "Control", ctrlKey: true })
    expect(recorder.textContent).toContain("Press a combination")
    fireEvent.keyDown(recorder, { key: "P", ctrlKey: true, shiftKey: true })
    expect(recorder.textContent).toContain("Ctrl + Shift + P")
    expect(playback).not.toHaveBeenCalled()
    expect(screen.getByRole("status").textContent).toContain("Save to apply")
    fireEvent.click(screen.getByRole("button", { name: "Clear mark watched key" }))
    expect(recorder.textContent).toContain("Disabled")
  } finally {
    window.removeEventListener("keydown", playback)
  }
})

test("Escape and blur cancel recording; unsupported and repeated keys do not replace the binding", () => {
  render(<Recorder />)
  const recorder = screen.getByRole("button", { name: "Mark watched key" })
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, { key: "Process", isComposing: true })
  fireEvent.keyDown(recorder, { key: "p", repeat: true })
  expect(recorder.textContent).toContain("Press a combination")
  fireEvent.keyDown(recorder, { key: "Escape" })
  expect(recorder.textContent).toBe("W")
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, { key: "Tab" })
  expect(recorder.textContent).toBe("W")
  fireEvent.click(recorder)
  fireEvent.blur(recorder)
  expect(recorder.textContent).toBe("W")
})

test("macOS records Command and Option combinations and displays Command", () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel")
  render(<Recorder />)
  const recorder = screen.getByRole("button", { name: "Mark watched key" })
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, { key: "w", metaKey: true })
  expect(recorder.textContent).toBe("⌘ + W")
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, { key: "π", code: "KeyP", altKey: true })
  expect(recorder.textContent).toBe("Alt + P")
})
