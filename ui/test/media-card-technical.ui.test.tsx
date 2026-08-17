import { act, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { MediaCard } from "../src/components/MediaCard"
import type { ItemSummary, MediaStream } from "../src/lib/api"
import { formatVideoRange, summarizeCardMedia } from "../src/lib/format"
import { TechnicalContext } from "../src/lib/technical-context"
import { requireElement } from "./support/fixtures"

function stream(overrides: Partial<MediaStream>): MediaStream {
  return {
    index: 0,
    type: null,
    codec: null,
    profile: null,
    language: null,
    title: null,
    displayTitle: null,
    width: null,
    height: null,
    channels: null,
    audioSpatialFormat: null,
    videoRange: null,
    videoRangeType: null,
    bitDepth: null,
    isDefault: false,
    isForced: false,
    isHearingImpaired: false,
    isExternal: false,
    ...overrides,
  }
}

const technicalStreams = [
  stream({
    type: "Video",
    codec: "hevc",
    width: 3840,
    height: 1608,
    videoRange: "HDR",
    videoRangeType: "DOVIWithHDR10Plus",
    bitDepth: 10,
  }),
  stream({
    index: 1,
    type: "Audio",
    codec: "truehd",
    profile: "Dolby TrueHD with Dolby Atmos",
    channels: 8,
    audioSpatialFormat: "DolbyAtmos",
    isDefault: true,
  }),
  // Subtitle detail is intentionally not part of a card's technical readout.
  stream({ index: 2, type: "Subtitle", codec: "subrip" }),
]

const movie: ItemSummary = {
  id: "movie-1",
  kind: "Movie",
  name: "The Matrix",
  year: 1999,
  runtimeTicks: 8_160_000_000,
  communityRating: 8.2,
  officialRating: "R",
  seriesId: null,
  seriesName: null,
  indexNumber: null,
  parentIndexNumber: null,
  primaryImageTag: null,
  thumbImageTag: null,
  logoImageTag: null,
  backdropImageTag: null,
  childCount: null,
  premiereDate: null,
  seasonId: null,
  played: false,
  playCount: 0,
  positionTicks: 0,
  favorite: false,
}

const series: ItemSummary = {
  ...movie,
  id: "series-1",
  kind: "Series",
  name: "Severance",
  childCount: 2,
}

// Streams are never part of a summary row any more; cards read them from the
// live batched technical channel, so tests provide that channel directly.
afterEach(() => vi.unstubAllGlobals())

function withTechnical(streams: ReadonlyMap<string, MediaStream[]>, children: ReactNode) {
  return (
    <MemoryRouter>
      <TechnicalContext.Provider value={{ items: streams, register: () => () => {} }}>
        {children}
      </TechnicalContext.Provider>
    </MemoryRouter>
  )
}

describe("media-card technical formatting", () => {
  test("formats Jellyfin Dolby Vision ranges with compact canonical labels", () => {
    expect(formatVideoRange(stream({ videoRangeType: "DOVIWithHDR10Plus" }))).toBe("DV&HDR10+")

    const dolbyVision = stream({ type: "Video", videoRangeType: "DOVI" })
    expect(formatVideoRange(dolbyVision)).toBe("DV")
    expect(summarizeCardMedia([dolbyVision])).toEqual({
      video: ["DV"],
      audio: [],
      description: "Video: Dolby Vision",
    })
  })

  test("prioritizes resolution, dynamic range, lossless audio, and spatial audio", () => {
    const summary = summarizeCardMedia(technicalStreams)

    expect(summary?.video).toEqual(["4K", "DV&HDR10+"])
    expect(summary?.audio).toEqual(["TrueHD", "Atmos"])
    expect(summary?.description).toBe(
      "Video: 4K, Dolby Vision / HDR10+, HEVC, 10-bit; Audio: TrueHD, Atmos, 7.1",
    )
  })

  test("uses meaningful codec and channel fallbacks for ordinary SDR media", () => {
    const summary = summarizeCardMedia([
      stream({ type: "Video", codec: "h264", width: 1920, height: 804, videoRange: "SDR" }),
      stream({
        type: "Audio",
        codec: "dts",
        profile: "DTS-HD MA",
        channels: 8,
        isDefault: true,
      }),
    ])

    expect(summary?.video).toEqual(["1080p", "H.264"])
    expect(summary?.audio).toEqual(["DTS-HD MA", "7.1"])
  })

  test("omits the readout when no useful stream metadata exists", () => {
    expect(summarizeCardMedia(undefined)).toBeNull()
    expect(summarizeCardMedia([stream({ type: "Subtitle", codec: "subrip" })])).toBeNull()
  })

  test("renders one integrated semantic readout rather than badges or pills", () => {
    const { container } = render(
      withTechnical(
        new Map([[movie.id, technicalStreams]]),
        <MediaCard item={movie} preview={false} />,
      ),
    )

    const readout = screen.getByLabelText(/Technical media information/)
    expect(readout.tagName).toBe("DL")
    expect(readout.textContent).toContain("4K")
    expect(readout.textContent).toContain("DV&HDR10+")
    expect(readout.textContent).not.toContain("DOVIWithHDR10Plus")
    expect(readout.textContent).toContain("TrueHD")
    expect(readout.textContent).toContain("Atmos")
    expect(readout.getAttribute("aria-label")).toContain("Dolby Vision / HDR10+")
    expect(readout.getAttribute("title")).toContain("Dolby Vision / HDR10+")
    expect(readout.getAttribute("title")).toContain("HEVC")
    expect(container.querySelector('[data-slot="badge"]')).toBeNull()
  })

  test("series cards read the same live channel as movie cards", () => {
    render(
      withTechnical(
        new Map([
          [
            series.id,
            [
              stream({ type: "Video", codec: "h264", width: 1920, height: 1080, videoRange: "SDR" }),
              stream({ index: 1, type: "Audio", codec: "eac3", channels: 6, isDefault: true }),
            ],
          ],
        ]),
        <MediaCard item={series} preview={false} />,
      ),
    )

    const readout = screen.getByLabelText(/Technical media information/)
    expect(readout.textContent).toContain("1080p")
    expect(readout.textContent).toContain("H.264")
  })

  test("a card whose streams have not arrived renders no readout at all", () => {
    const { container } = render(
      withTechnical(new Map(), <MediaCard item={movie} preview={false} />),
    )
    expect(container.querySelector(".card-technical-readout")).toBeNull()
  })

  test("mounted shelf cards register only when they approach the viewport", () => {
    let reportIntersection: ((visible: boolean) => void) | undefined
    vi.stubGlobal(
      "IntersectionObserver",
      class TestIntersectionObserver implements IntersectionObserver {
        readonly root = null
        readonly rootMargin = "0px"
        readonly scrollMargin = "0px"
        readonly thresholds = [0]

        constructor(callback: IntersectionObserverCallback) {
          reportIntersection = (visible) => {
            const bounds = new DOMRect()
            callback(
              [{
                boundingClientRect: bounds,
                intersectionRatio: visible ? 1 : 0,
                intersectionRect: bounds,
                isIntersecting: visible,
                rootBounds: null,
                target: document.body,
                time: 0,
              }],
              this,
            )
          }
        }
        disconnect() {}
        observe() {}
        takeRecords() { return [] }
        unobserve() {}
      },
    )
    const unregister = vi.fn()
    const register = vi.fn(() => unregister)

    render(
      <MemoryRouter>
        <TechnicalContext.Provider value={{ items: new Map(), register }}>
          <MediaCard item={movie} preview={false} />
        </TechnicalContext.Provider>
      </MemoryRouter>,
    )

    expect(register).not.toHaveBeenCalled()
    if (!reportIntersection) throw new Error("Expected the card to create an intersection observer")
    const report = reportIntersection
    act(() => report(true))
    expect(register).toHaveBeenCalledWith(movie.id)
    act(() => report(false))
    expect(unregister).toHaveBeenCalledTimes(1)
  })
})

describe("media-card playback indicators", () => {
  test("uses the same thickness and bright fill for watched and in-progress media", () => {
    const watched = render(
      <MemoryRouter>
        <MediaCard item={{ ...movie, played: true }} preview={false} />
      </MemoryRouter>,
    )
    const watchedTrack = requireElement(
      watched.container.querySelector<HTMLElement>('[title="Watched"]'),
      "watched progress track",
    )
    const watchedFill = requireElement(
      watchedTrack.firstElementChild instanceof HTMLElement
        ? watchedTrack.firstElementChild
        : null,
      "watched progress fill",
    )

    expect(watchedTrack.className).toContain("h-[3px]")
    expect(watchedFill.className).toBe("h-full bg-primary")
    expect(watchedFill.style.width).toBe("100%")

    watched.unmount()

    const progressed = render(
      <MemoryRouter>
        <MediaCard
          item={{ ...movie, id: "movie-2", positionTicks: (movie.runtimeTicks ?? 0) / 2 }}
          preview={false}
        />
      </MemoryRouter>,
    )
    const progressedFill = requireElement(
      progressed.container.querySelector<HTMLElement>('[style="width: 50%;"]'),
      "in-progress fill",
    )
    const progressedTrack = requireElement(progressedFill.parentElement, "in-progress track")

    expect(progressedTrack.className).toContain("h-[3px]")
    expect(progressedFill.className).toBe(watchedFill.className)
  })
})
