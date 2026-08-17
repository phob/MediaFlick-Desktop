import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, test, vi } from "vitest"
import { MediaInfoView } from "../src/components/detail/MediaInfo"
import type { MediaSource, MediaStream } from "../src/lib/api"

const save = { isPending: false, mutate: vi.fn() }

function stream(overrides: Partial<MediaStream>): MediaStream {
  return {
    index: 0,
    codec: null,
    language: null,
    title: null,
    displayTitle: null,
    width: null,
    height: null,
    channels: null,
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

const source: MediaSource = {
  id: "source-a",
  name: "Feature",
  container: "mkv",
  fileName: "movie.mkv",
  size: null,
  bitrate: null,
  defaultAudioStreamIndex: 1,
  defaultSubtitleStreamIndex: null,
  video: [],
  audio: [
    stream({ index: 1, language: "eng", codec: "aac", channels: 2, isDefault: true }),
    stream({
      index: 2,
      language: "jpn",
      title: "Director commentary",
      codec: "dts",
      channels: 6,
    }),
  ],
  subtitles: [
    stream({
      index: 4,
      language: "eng",
      title: "English SDH",
      codec: "subrip",
      isForced: true,
      isHearingImpaired: true,
      isExternal: true,
    }),
  ],
}

beforeEach(() => save.mutate.mockReset())

describe("per-item media track controls", () => {
  test("shows a named source selector when the item has multiple files", () => {
    render(
      <MediaInfoView
        save={save}
        sources={[
          source,
          { ...source, id: "opaque-source-b", name: "Director's cut", fileName: null },
        ]}
        preference={{
          mediaSourceId: "opaque-source-b",
          mediaSourceIndex: 1,
          audioStreamIndex: 2,
          subtitleStreamIndex: null,
        }}
        isPending={false}
      />,
    )

    const picker = screen.getByRole("combobox", { name: "Media source" })
    expect(picker.textContent).toContain("Director's cut")
    expect(picker.textContent).not.toContain("opaque-source-b")
  })

  test("restores understandable selected audio and subtitle labels", () => {
    render(
      <MediaInfoView
        save={save}
        sources={[source]}
        preference={{
          mediaSourceId: "source-a",
          mediaSourceIndex: 0,
          audioStreamIndex: 2,
          subtitleStreamIndex: 4,
        }}
        isPending={false}
      />,
    )

    const audio = screen.getByRole("combobox", { name: "Audio track" })
    const subtitle = screen.getByRole("combobox", { name: "Subtitle track" })
    expect(audio.textContent).toMatch(/Director commentary/)
    expect(audio.textContent).toMatch(/DTS/)
    expect(audio.textContent).toMatch(/5\.1/)
    expect(subtitle.textContent).toMatch(/English SDH/)
    expect(subtitle.textContent).toMatch(/Forced/)
    expect(subtitle.textContent).toMatch(/SDH/)
  })

  test("offers subtitles off and saves it with the current audio/source", () => {
    render(
      <MediaInfoView
        save={save}
        sources={[source]}
        preference={{
          mediaSourceId: "source-a",
          mediaSourceIndex: 0,
          audioStreamIndex: 2,
          subtitleStreamIndex: 4,
        }}
        isPending={false}
      />,
    )

    fireEvent.click(screen.getByRole("combobox", { name: "Subtitle track" }))
    fireEvent.click(screen.getByRole("option", { name: "Subtitles off" }))

    expect(save.mutate).toHaveBeenCalledWith({
      mediaSourceId: "source-a",
      mediaSourceIndex: 0,
      audioStreamIndex: 2,
      subtitleStreamIndex: null,
    })
  })

  test("keeps a single-track item as read-only media information", () => {
    render(
      <MediaInfoView
        save={save}
        sources={[{ ...source, audio: source.audio.slice(0, 1), subtitles: [] }]}
        preference={{
          mediaSourceId: "source-a",
          mediaSourceIndex: 0,
          audioStreamIndex: 1,
          subtitleStreamIndex: null,
        }}
        isPending={false}
      />,
    )

    expect(screen.queryByRole("combobox")).toBeNull()
    expect(screen.getByText(/AAC/)).toBeTruthy()
  })
})
