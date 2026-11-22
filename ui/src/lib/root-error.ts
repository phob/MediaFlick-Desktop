import { isRouteErrorResponse } from "react-router-dom"

const FALLBACK_ERROR_SUMMARY = "An unexpected interface error occurred."
const MAX_SUMMARY_LENGTH = 240

const SECRET_PATTERNS: readonly [RegExp, string][] = [
  [/([?&](?:api_key|apikey|token|access_token|password|secret)=)[^&#\s]+/gi, "$1[redacted]"],
  [/(\bauthorization\s*:\s*)(?:bearer\s+)?[^\s,;]+/gi, "$1[redacted]"],
  [/(\bbearer\s+)[^\s,;]+/gi, "$1[redacted]"],
  [
    /(\b(?:x-emby-token|x-mediabrowser-token|api_key|apikey|access_token|token|password|secret)\s*[:=]\s*)[^\s&,;]+/gi,
    "$1[redacted]",
  ],
]

function redactSummary(raw: string): string {
  const singleLine = raw.split(/\r?\n/, 1)[0]?.trim() ?? ""
  if (!singleLine) return FALLBACK_ERROR_SUMMARY
  let sanitized = singleLine
  for (const [pattern, replacement] of SECRET_PATTERNS) {
    sanitized = sanitized.replace(pattern, replacement)
  }
  if (sanitized.length > MAX_SUMMARY_LENGTH) {
    return `${sanitized.slice(0, MAX_SUMMARY_LENGTH - 1)}…`
  }
  return sanitized
}

export function formatRootErrorSummary(error: unknown): string {
  if (isRouteErrorResponse(error)) {
    const statusLine = [error.status, error.statusText].filter(Boolean).join(" ")
    return redactSummary(statusLine || FALLBACK_ERROR_SUMMARY)
  }
  if (error instanceof Error) {
    return redactSummary(error.message || error.name)
  }
  if (typeof error === "string") {
    return redactSummary(error)
  }
  if (typeof error === "object" && error !== null && "status" in error && "statusText" in error) {
    const status = typeof error.status === "number" ? String(error.status) : ""
    const statusText = typeof error.statusText === "string" ? error.statusText : ""
    const statusLine = [status, statusText].filter(Boolean).join(" ")
    if (statusLine) return redactSummary(statusLine)
  }
  return FALLBACK_ERROR_SUMMARY
}

export function reloadApp() {
  if (window.location.pathname !== "/" || window.location.search !== "" || window.location.hash !== "") {
    window.location.assign("/")
    return
  }
  window.location.reload()
}
