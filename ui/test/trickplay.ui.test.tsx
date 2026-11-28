import { fireEvent, render, screen } from "@testing-library/react"
import { expect, test, vi } from "vitest"
import { Timeline } from "@/components/player/Timeline"
import { trickplayFrameAt } from "@/components/player/format"

const INFO = {
  width: 320,
  height: 180,
  tileWidth: 10,
  tileHeight: 10,
  thumbnailCount: 250,
  intervalMs: 10_000,
}

test("trickplayFrameAt maps timestamps across mosaic tiles and clamps bounds", () => {
  expect(trickplayFrameAt(INFO, -5_000, 160)).toEqual({
    tileIndex: 0,
    offsetX: 0,
    offsetY: 0,
    previewWidth: 160,
    previewHeight: 90,
    sheetWidth: 1600,
    sheetHeight: 900,
  })

  expect(trickplayFrameAt(INFO, 1_120_000, 160)).toEqual({
    tileIndex: 1,
    offsetX: 320,
    offsetY: 90,
    previewWidth: 160,
    previewHeight: 90,
    sheetWidth: 1600,
    sheetHeight: 900,
  })

  expect(trickplayFrameAt(INFO, 99_999_000, 160)).toEqual({
    tileIndex: 2,
    offsetX: 1440,
    offsetY: 360,
    previewWidth: 160,
    previewHeight: 90,
    sheetWidth: 1600,
    sheetHeight: 900,
  })

  expect(trickplayFrameAt({ ...INFO, intervalMs: 0 }, 5_000, 160)).toBeNull()
})

test("Timeline hover renders trickplay sprite when present and omits it when absent or errored", () => {
  const { container, rerender } = render(
    <Timeline
      position={0}
      duration={2_500_000}
      chapters={[]}
      segments={[]}
      bufferedUntil={null}
      trickplay={{ itemId: "movie-1", mediaSourceId: "src-1", info: INFO }}
      onScrub={vi.fn()}
      onSeek={vi.fn()}
    />,
  )

  const wrapper = container.firstElementChild
  if (!(wrapper instanceof HTMLElement)) {
    throw new Error("expected Timeline root element")
  }
  vi.spyOn(wrapper, "getBoundingClientRect").mockReturnValue({
    left: 0,
    width: 250,
    right: 250,
    top: 0,
    bottom: 40,
    height: 40,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  })

  fireEvent.pointerMove(wrapper, { clientX: 112 })
  const preview = screen.getByTestId("trickplay-preview")
  expect(preview.style.backgroundImage).toContain("/api/trickplay/movie-1/320/1?mediaSourceId=src-1")
  expect(preview.style.backgroundPosition).toBe("-384px -108px")
  expect(preview.style.backgroundSize).toBe("1920px 1080px")

  const loaderImg = preview.querySelector("img")
  if (!(loaderImg instanceof HTMLImageElement)) {
    throw new Error("expected hidden trickplay loader img")
  }
  fireEvent.error(loaderImg)
  expect(screen.queryByTestId("trickplay-preview")).toBeNull()

  rerender(
    <Timeline
      position={0}
      duration={2_500_000}
      chapters={[]}
      segments={[]}
      bufferedUntil={null}
      trickplay={null}
      onScrub={vi.fn()}
      onSeek={vi.fn()}
    />,
  )
  fireEvent.pointerMove(wrapper, { clientX: 50 })
  expect(screen.getByText("8:20")).toBeTruthy()
  expect(screen.queryByTestId("trickplay-preview")).toBeNull()
})
