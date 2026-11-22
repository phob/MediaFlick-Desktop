const EXPLICIT_SCHEME = /^[a-z][a-z\d+.-]*:(?!\d)/i
const IPV4_OCTET = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/

export function looksLikeServerAddress(value: string): boolean {
  const trimmed = value.trim()
  const lower = trimmed.toLowerCase()
  let candidate: string
  if (lower.startsWith("http://") || lower.startsWith("https://")) candidate = trimmed
  else if (EXPLICIT_SCHEME.test(trimmed)) return false
  else candidate = `http://${trimmed}`
  if (!URL.canParse(candidate)) return false

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
