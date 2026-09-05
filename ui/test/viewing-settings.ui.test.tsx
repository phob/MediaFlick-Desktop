import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes, useLocation } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { api, type ViewingSettings } from "@/lib/api"
import { queryClient, queryKeys } from "@/lib/query-client"
import { DEFAULT_VIEWING, ViewingContext } from "@/lib/viewing"
import { usePlaybackEventsBridge } from "@/lib/playback-events"
import { MediaCard } from "@/components/MediaCard"
import { ViewingSync } from "@/components/ViewingSync"
import Library from "@/routes/Library"
import Settings from "@/routes/Settings"
import { TestProviders } from "./test-utils"
import { itemSummary } from "./support/fixtures"

afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); queryClient.clear() })

function seed(viewing: ViewingSettings = DEFAULT_VIEWING) {
  queryClient.setQueryDefaults(["status"], {staleTime:Infinity})
  queryClient.setQueryDefaults(["viewing"], {staleTime:Infinity})
  queryClient.setQueryData(queryKeys.status, {authenticated:true, serverUrl:"server", userId:"user"})
  queryClient.setQueryData(["viewing", "server:user"], viewing)
}

test("language edits enable Save, Discard restores the text, and Save persists normalized codes", async () => {
  seed()
  const save = vi.spyOn(api, "saveViewing").mockImplementation(async (value) => value)
  render(<TestProviders client={queryClient} initialEntries={["/settings/viewing"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
  const input = screen.getByRole("textbox", {name:"Audio languages"})
  fireEvent.change(input, {target:{value:"EN, de"}})
  expect((screen.getByRole("button", {name:"Save"}) as HTMLButtonElement).disabled).toBe(false)
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect((input as HTMLInputElement).value).toBe("")
  fireEvent.change(input, {target:{value:"EN, de"}})
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({audioLanguages:["en", "de"]}), expect.anything()))
  await waitFor(() => expect((screen.getByRole("button", {name:"Save"}) as HTMLButtonElement).disabled).toBe(true))
})

test("spoiler protection removes episode titles and image URLs without hiding watched episodes", () => {
  seed()
  const episode = itemSummary({id:"episode", kind:"Episode", name:"The secret twist", primaryImageTag:"spoiler-image"})
  const {container, rerender} = render(<TestProviders client={queryClient}>
    <ViewingContext value={{...DEFAULT_VIEWING, spoilerProtection:true}}><MediaCard item={episode} /></ViewingContext>
  </TestProviders>)
  expect(container.textContent).not.toContain("The secret twist")
  expect(container.querySelector("img")).toBeNull()
  rerender(<TestProviders client={queryClient}>
    <ViewingContext value={{...DEFAULT_VIEWING, spoilerProtection:true}}><MediaCard item={{...episode, played:true}} /></ViewingContext>
  </TestProviders>)
  expect(container.textContent).toContain("The secret twist")
})

function PlaybackBridge() { usePlaybackEventsBridge(); return null }

test("manual playback cancels a pending next-episode countdown", async () => {
  vi.useFakeTimers()
  seed({...DEFAULT_VIEWING, nextEpisode:"ask", countdownSeconds:3})
  vi.spyOn(api, "playbackNeighbors").mockResolvedValue({previous:null, next:itemSummary({id:"next", kind:"Episode", name:"Next"})})
  const next = vi.spyOn(api, "playNext").mockResolvedValue({started:false})
  render(<TestProviders client={queryClient}><PlaybackBridge /></TestProviders>)
  act(() => window.__mediaFlickDesktopPlaybackStopped?.({active:false, itemId:"episode", stopReason:"eof"}))
  act(() => window.dispatchEvent(new Event("mediaflick-manual-play")))
  await act(() => vi.advanceTimersByTimeAsync(4000))
  expect(next).not.toHaveBeenCalled()
})

test("continuous playback stops at the episode limit while explicit next remains available", async () => {
  seed({...DEFAULT_VIEWING, episodeLimit:1})
  const next = vi.spyOn(api, "playNext").mockResolvedValue({started:false})
  render(<TestProviders client={queryClient}><PlaybackBridge /></TestProviders>)
  act(() => window.__mediaFlickDesktopPlaybackStopped?.({active:false, itemId:"episode", stopReason:"eof"}))
  expect(next).not.toHaveBeenCalled()
  act(() => window.__mediaFlickDesktopPlaybackStateChanged?.({active:true, itemId:"episode2"}))
  await act(async () => window.__mediaFlickDesktopPlaybackStopped?.({active:false, itemId:"episode2", stopReason:"watched-next"}))
  expect(next).toHaveBeenCalledWith("episode2")
})

function LocationProbe() { const location = useLocation(); return <output aria-label="Location">{location.pathname + location.search}</output> }

test.each([
  ["/", "/library?kind=Series"],
  ["/item/explicit", "/item/explicit"],
])("startup selection honors an explicit route: %s", async (initial, expected) => {
  seed({...DEFAULT_VIEWING, startupDestination:"series"})
  queryClient.setQueryData(["browsing", "server:user"], {})
  vi.spyOn(api, "browsing").mockResolvedValue({})
  vi.spyOn(api, "saveBrowsing").mockResolvedValue({saved:true})
  render(<TestProviders client={queryClient} initialEntries={[initial]}><ViewingSync /><LocationProbe /></TestProviders>)
  await waitFor(() => expect(screen.getByLabelText("Location").textContent).toBe(expected))
})

test("remembered Series filters take precedence over the hide-watched default", () => {
  seed({...DEFAULT_VIEWING, rememberFilters:true, hideWatched:true})
  queryClient.setQueryData(["browsing", "server:user"], {Series:"/library?kind=Series&sort=year&watched=true&filters=true", Movie:"/library?kind=Movie&sort=rating"})
  queryClient.setQueryData(queryKeys.genres, {genres:[]})
  vi.spyOn(api, "browsing").mockResolvedValue({})
  render(<TestProviders client={queryClient} initialEntries={["/library?kind=Series"]}>
    <Library components={{ItemGrid:({query}) => <output aria-label="Library query">{JSON.stringify(query)}</output>}} />
  </TestProviders>)
  expect(JSON.parse(screen.getByLabelText("Library query").textContent ?? "{}")).toMatchObject({kind:"Series", sort:"year", watched:"true"})
})

test.each([true, false])("countdown only starts another episode when one exists: %s", async (hasNext) => {
  vi.useFakeTimers()
  seed({...DEFAULT_VIEWING, nextEpisode:"ask", countdownSeconds:3})
  vi.spyOn(api, "playbackNeighbors").mockResolvedValue({previous:null, next:hasNext ? itemSummary({id:"next", kind:"Episode", name:"Next"}) : null})
  const next = vi.spyOn(api, "playNext").mockResolvedValue({started:false})
  render(<TestProviders client={queryClient}><PlaybackBridge /></TestProviders>)
  await act(async () => window.__mediaFlickDesktopPlaybackStopped?.({active:false, itemId:"episode", stopReason:"eof"}))
  await act(() => vi.advanceTimersByTimeAsync(4000))
  expect(next).toHaveBeenCalledTimes(hasNext ? 1 : 0)
})


test("poster width offers presets and Viewing reset preserves the Appearance preview delay", async () => {
  seed({...DEFAULT_VIEWING, previewDelayMs:850})
  const save = vi.spyOn(api, "saveViewing").mockImplementation(async (value) => value)
  render(<TestProviders client={queryClient} initialEntries={["/settings/viewing"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
  expect(screen.queryByRole("spinbutton", {name:"Poster width"})).toBeNull()
  expect(screen.queryByLabelText("Card preview delay")).toBeNull()
  fireEvent.pointerDown(screen.getByRole("combobox", {name:"Poster width"}), {button:0, ctrlKey:false, pointerType:"mouse"})
  fireEvent.click(await screen.findByRole("option", {name:"Large — 200 px"}))
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({posterSize:200, previewDelayMs:850}), expect.anything()))
  await waitFor(() => expect((screen.getByRole("button", {name:"Save"}) as HTMLButtonElement).disabled).toBe(true))
  fireEvent.click(screen.getByRole("button", {name:"Reset"}))
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenLastCalledWith(expect.objectContaining({posterSize:168, previewDelayMs:850}), expect.anything()))
})
