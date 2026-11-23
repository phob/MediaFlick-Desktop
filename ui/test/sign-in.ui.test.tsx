import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, test, vi } from "vitest"
import { api, ApiError, type ServerInfo } from "@/lib/api"
import SignIn from "@/routes/SignIn"
import { clientSettingsFixture } from "./support/settings"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const serverUrl = "https://jellyfin.example"
const serverInfo: ServerInfo = { serverUrl, serverName: "My Jellyfin", version: "12.0.0", quickConnect: true, tooOld: null }

beforeEach(() => {
  vi.spyOn(api, "settings").mockResolvedValue({ ...clientSettingsFixture(), serverUrl: null })
  vi.spyOn(api, "connect").mockResolvedValue(serverInfo)
  vi.spyOn(api, "quickConnectStart").mockResolvedValue({ serverUrl, code: "123456", secret: "test-secret" })
  vi.spyOn(api, "quickConnectPoll").mockResolvedValue({ authenticated: false })
  vi.spyOn(api, "login").mockRejectedValue(new Error("Incorrect username or password"))
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function renderSignIn() {
  return render(<TestProviders client={testQueryClient()}><SignIn /></TestProviders>)
}

function enterServer(value = serverUrl) {
  const input = screen.getByLabelText("Server")
  fireEvent.change(input, { target: { value } })
  fireEvent.blur(input)
}

function quickConnectButton() {
  return screen.getByRole<HTMLButtonElement>("button", { name: "Use Quick Connect" })
}

test("fresh configuration offers Quick Connect before any server or credentials are entered", async () => {
  renderSignIn()
  await waitFor(() => expect(api.settings).toHaveBeenCalled())
  expect(quickConnectButton().disabled).toBe(true)
  expect(api.connect).not.toHaveBeenCalled()
  expect(api.quickConnectStart).not.toHaveBeenCalled()

  enterServer(`  ${serverUrl}  `)
  await waitFor(() => expect(quickConnectButton().disabled).toBe(false))
  expect(api.connect).toHaveBeenCalledWith(serverUrl, expect.any(AbortSignal))
  fireEvent.click(quickConnectButton())

  expect(await screen.findByText("123456")).toBeTruthy()
  await waitFor(() => expect(api.quickConnectPoll).toHaveBeenCalledWith(serverUrl, "test-secret", expect.any(AbortSignal)))
  expect(api.quickConnectStart).toHaveBeenCalledWith(serverUrl)
  expect(api.login).not.toHaveBeenCalled()
})

test("disabled Quick Connect stays visible and password sign-in remains available", async () => {
  vi.mocked(api.connect).mockResolvedValue({ ...serverInfo, quickConnect: false })
  renderSignIn()
  enterServer()
  expect(await screen.findByText(/Quick Connect is not enabled on this server/)).toBeTruthy()
  expect(quickConnectButton().disabled).toBe(true)

  fireEvent.change(screen.getByLabelText("Username"), { target: { value: "viewer" } })
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "password" } })
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }))
  expect(await screen.findByText("Incorrect username or password")).toBeTruthy()
  expect(api.login).toHaveBeenCalledWith(serverUrl, "viewer", "password")
  expect(api.quickConnectStart).not.toHaveBeenCalled()
})

test("a server older than Jellyfin 10.11.0 is refused before any credentials are sent", async () => {
  vi.mocked(api.connect).mockResolvedValue({
    ...serverInfo,
    version: "10.10.7",
    quickConnect: false,
    tooOld: { version: "10.10.7", minimum: "10.11.0" },
  })
  renderSignIn()
  enterServer()
  const refusal = await screen.findByRole("alert")
  expect(refusal.textContent).toBe("This server runs Jellyfin 10.10.7. MediaFlick needs Jellyfin 10.11.0 or newer.")
  expect(quickConnectButton().disabled).toBe(true)

  fireEvent.change(screen.getByLabelText("Username"), { target: { value: "viewer" } })
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "password" } })
  const signIn = screen.getByRole<HTMLButtonElement>("button", { name: "Sign in" })
  expect(signIn.disabled).toBe(true)
  fireEvent.click(signIn)
  expect(api.login).not.toHaveBeenCalled()
})

test("editing a saved server clears its code and cancels its active poll before blur", async () => {
  vi.mocked(api.settings).mockResolvedValue(clientSettingsFixture())
  let pollSignal: AbortSignal | undefined
  vi.mocked(api.quickConnectPoll).mockImplementation((_server, _secret, signal) => {
    pollSignal = signal
    return new Promise(() => {})
  })
  renderSignIn()
  await waitFor(() => expect(quickConnectButton().disabled).toBe(false))
  fireEvent.click(quickConnectButton())
  expect(await screen.findByText("123456")).toBeTruthy()
  await waitFor(() => expect(pollSignal).toBeDefined())

  fireEvent.change(screen.getByLabelText("Server"), { target: { value: "" } })
  expect(screen.queryByText("123456")).toBeNull()
  expect(quickConnectButton().disabled).toBe(true)
  expect(pollSignal?.aborted).toBe(true)
})

test("a late code from a previous server cannot start polling after the address changes", async () => {
  let resolveStart: (value: Awaited<ReturnType<typeof api.quickConnectStart>>) => void = () => {}
  vi.mocked(api.quickConnectStart).mockReturnValue(new Promise((resolve) => { resolveStart = resolve }))
  renderSignIn()
  enterServer()
  await waitFor(() => expect(quickConnectButton().disabled).toBe(false))
  fireEvent.click(quickConnectButton())
  await waitFor(() => expect(api.quickConnectStart).toHaveBeenCalled())
  fireEvent.change(screen.getByLabelText("Server"), { target: { value: "https://other.example" } })
  await act(async () => resolveStart({ serverUrl, code: "123456", secret: "test-secret" }))
  expect(screen.queryByText("123456")).toBeNull()
  expect(quickConnectButton().disabled).toBe(true)
  expect(api.quickConnectPoll).not.toHaveBeenCalled()
})

test("an expired Quick Connect code stops polling, restores the start button, and allows requesting a new code", async () => {
  vi.useFakeTimers()
  vi.mocked(api.quickConnectStart)
    .mockResolvedValueOnce({ serverUrl, code: "111111", secret: "expired-secret" })
    .mockResolvedValueOnce({ serverUrl, code: "222222", secret: "fresh-secret" })
  vi.mocked(api.quickConnectPoll)
    .mockResolvedValueOnce({ authenticated: false })
    .mockRejectedValueOnce(new ApiError("Jellyfin returned HTTP 404", 404))
    .mockResolvedValue({ authenticated: false })

  renderSignIn()
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  enterServer()
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(quickConnectButton().disabled).toBe(false)

  fireEvent.click(quickConnectButton())
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(screen.getByText("111111")).toBeTruthy()
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(1)

  await act(async () => { await vi.advanceTimersByTimeAsync(2_000) })
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(2)
  expect(screen.queryByText("111111")).toBeNull()
  expect(quickConnectButton().disabled).toBe(false)
  expect(screen.getByText(/expired/i)).toBeTruthy()

  await act(async () => { await vi.advanceTimersByTimeAsync(6_000) })
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(2)

  fireEvent.click(quickConnectButton())
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(screen.getByText("222222")).toBeTruthy()
  expect(screen.queryByText(/expired/i)).toBeNull()
  expect(api.quickConnectPoll).toHaveBeenCalledWith(serverUrl, "fresh-secret", expect.any(AbortSignal))
})

test("a Quick Connect poll failure stops the 2-second polling loop and restores the start button", async () => {
  vi.useFakeTimers()
  vi.mocked(api.quickConnectStart)
    .mockResolvedValueOnce({ serverUrl, code: "123456", secret: "first-secret" })
    .mockResolvedValueOnce({ serverUrl, code: "654321", secret: "retry-secret" })
  vi.mocked(api.quickConnectPoll)
    .mockResolvedValueOnce({ authenticated: false })
    .mockRejectedValueOnce(new ApiError("could not reach Jellyfin", 502))
    .mockResolvedValue({ authenticated: false })

  renderSignIn()
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  enterServer()
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })

  fireEvent.click(quickConnectButton())
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(1)

  await act(async () => { await vi.advanceTimersByTimeAsync(2_000) })
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(2)
  expect(screen.getByText("could not reach Jellyfin")).toBeTruthy()
  expect(screen.queryByText("123456")).toBeNull()
  expect(quickConnectButton().disabled).toBe(false)

  await act(async () => { await vi.advanceTimersByTimeAsync(6_000) })
  expect(api.quickConnectPoll).toHaveBeenCalledTimes(2)

  fireEvent.click(quickConnectButton())
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(screen.getByText("654321")).toBeTruthy()
  expect(screen.queryByText("could not reach Jellyfin")).toBeNull()
  expect(api.quickConnectPoll).toHaveBeenCalledWith(serverUrl, "retry-secret", expect.any(AbortSignal))
})
