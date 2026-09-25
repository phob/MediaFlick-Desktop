import { render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { MediaCard } from "../src/components/MediaCard"
import type { ItemSummary, MediaStream } from "../src/lib/api"
import { summarizeCardMedia } from "../src/lib/format"
import { TechnicalContext } from "../src/lib/technical-context"

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
  test("prioritizes resolution, dynamic range, lossless audio, and spatial audio", () => {
    const summary = summarizeCardMedia(technicalStreams)

    expect(summary?.video).toEqual(["4K", "DV&HDR10+"])
    expect(summary?.audio).toEqual(["TrueHD", "Atmos"])
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
    render(withTechnical(new Map(), <MediaCard item={movie} preview={false} />))
    expect(screen.queryByLabelText(/Technical media information/)).toBeNull()
  })
})
