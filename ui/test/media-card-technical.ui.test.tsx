import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { MediaCard } from "../src/components/MediaCard"
import type { ItemSummary, MediaStream } from "../src/lib/api"
import { formatVideoRange, summarizeCardMedia } from "../src/lib/format"

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
  mediaStreams: technicalStreams,
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
      <MemoryRouter>
        <MediaCard item={movie} preview={false} />
      </MemoryRouter>,
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
})

describe("media-card playback indicators", () => {
  test("uses the same thickness and bright fill for watched and in-progress media", () => {
    const watched = render(
      <MemoryRouter>
        <MediaCard item={{ ...movie, played: true }} preview={false} />
      </MemoryRouter>,
    )
    const watchedTrack = watched.container.querySelector('[title="Watched"]') as HTMLElement
    const watchedFill = watchedTrack.firstElementChild as HTMLElement

    expect(watchedTrack.className).toContain("h-[3px]")
    expect(watchedFill.className).toBe("h-full bg-primary")
    expect(watchedFill.style.width).toBe("100%")

    watched.unmount()

    const progressed = render(
      <MemoryRouter>
        <MediaCard
          item={{ ...movie, id: "movie-2", positionTicks: movie.runtimeTicks! / 2 }}
          preview={false}
        />
      </MemoryRouter>,
    )
    const progressedFill = progressed.container.querySelector('[style="width: 50%;"]') as HTMLElement
    const progressedTrack = progressedFill.parentElement as HTMLElement

    expect(progressedTrack.className).toContain("h-[3px]")
    expect(progressedFill.className).toBe(watchedFill.className)
  })
})
