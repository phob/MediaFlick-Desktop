import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, test, vi } from "vitest"
import { api, type ServerInfo } from "@/lib/api"
import SignIn from "@/routes/SignIn"
import { clientSettingsFixture } from "./support/settings"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const serverUrl = "https://jellyfin.example"
const serverInfo: ServerInfo = { serverUrl, serverName: "My Jellyfin", version: "12.0.0", quickConnect: true }

beforeEach(() => {
  vi.spyOn(api, "settings").mockResolvedValue({ ...clientSettingsFixture(), serverUrl: null })
  vi.spyOn(api, "connect").mockResolvedValue(serverInfo)
  vi.spyOn(api, "quickConnectStart").mockResolvedValue({ serverUrl, code: "123456", secret: "test-secret" })
  vi.spyOn(api, "quickConnectPoll").mockResolvedValue({ authenticated: false })
  vi.spyOn(api, "login").mockRejectedValue(new Error("Incorrect username or password"))
})

afterEach(() => vi.restoreAllMocks())

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
  expect(screen.getByText(/Enter your server address first/)).toBeTruthy()
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

test("shows progress and connection failure without hiding Quick Connect", async () => {
  let rejectProbe: (error: Error) => void = () => {}
  vi.mocked(api.connect).mockReturnValue(new Promise((_, reject) => { rejectProbe = reject }))
  renderSignIn()
  enterServer()
  expect(await screen.findByText("Checking Quick Connect availability…")).toBeTruthy()
  expect(quickConnectButton().disabled).toBe(true)
  await act(async () => rejectProbe(new Error("Connection refused")))
  expect(await screen.findByText(/Could not check Quick Connect/)).toBeTruthy()
  expect(quickConnectButton().disabled).toBe(true)
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

  vi.mocked(api.connect).mockResolvedValue({ ...serverInfo, quickConnect: false })
  enterServer("https://other.example")
  expect(await screen.findByText(/Quick Connect is not enabled on this server/)).toBeTruthy()
  expect(quickConnectButton().disabled).toBe(true)
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
