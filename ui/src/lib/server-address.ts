// Mirrors `has_explicit_scheme` in src/preferences/model.rs: `host:8096` is a
// port, not a scheme.
const EXPLICIT_SCHEME = /^[a-z][a-z\d+.-]*:(?!\d)/i
const IPV4_OCTET = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/

/**
 * Whether a Server field value already names a complete host, so probing it
 * while the user is still typing is worth a request. Normalizes like the
 * native `normalize_server_url`.
 */
export function looksLikeServerAddress(value: string): boolean {
  const trimmed = value.trim()
  const lower = trimmed.toLowerCase()
  let candidate: string
  if (lower.startsWith("http://") || lower.startsWith("https://")) candidate = trimmed
  else if (EXPLICIT_SCHEME.test(trimmed)) return false
  else candidate = `http://${trimmed}`
  if (!URL.canParse(candidate)) return false

  // The URL parser rewrites shorthand IPv4 (`192.168.1` becomes `192.168.0.1`),
  // so the completeness checks read the host as typed.
  const authority = candidate.slice(candidate.indexOf("://") + 3).split(/[/\\?#]/, 1)[0]
  const hostAndPort = authority.slice(authority.lastIndexOf("@") + 1)
  if (/:\d+$/.test(hostAndPort)) return true
  const host = hostAndPort.replace(/:$/, "").toLowerCase()
  if (host === "localhost" || host.startsWith("[")) return true

  const labels = host.split(".")
  const last = labels[labels.length - 1]
  if (/^\d+$/.test(last)) return labels.length === 4 && labels.every((label) => IPV4_OCTET.test(label))
  return labels.length > 1 && /^\p{L}{2,}$/u.test(last)
}
