import { Badge } from "@/components/ui/badge"
import type { SeerrRequestStatus, SeerrStatus } from "@/lib/api"

type StatusBadgeDescription = {
  label: string
  variant: "default" | "secondary" | "outline" | "destructive"
}

/**
 * What Seerr already knows about a title. `unknown` is deliberately not a badge:
 * "we have never heard of this" is the default state of everything on a
 * discovery page, and a badge on every card would say nothing.
 */
const MEDIA_LABELS = {
  unknown: null,
  pending: { label: "Requested", variant: "secondary" },
  processing: { label: "Downloading", variant: "secondary" },
  partial: { label: "Partly available", variant: "outline" },
  available: { label: "Available", variant: "default" },
  blacklisted: { label: "Blocked", variant: "destructive" },
} satisfies Record<SeerrStatus, StatusBadgeDescription | null>

const REQUEST_LABELS = {
  pending: { label: "Awaiting approval", variant: "secondary" },
  approved: { label: "Approved", variant: "default" },
  declined: { label: "Declined", variant: "destructive" },
  failed: { label: "Failed", variant: "destructive" },
  unknown: { label: "Unknown", variant: "outline" },
} satisfies Record<SeerrRequestStatus, StatusBadgeDescription>

export function SeerrStatusBadge({
  status,
  className,
}: {
  status: SeerrStatus
  className?: string
}) {
  const entry = MEDIA_LABELS[status]
  if (!entry) return null
  return (
    <Badge variant={entry.variant} className={className}>
      {entry.label}
    </Badge>
  )
}

export function SeerrRequestStatusBadge({
  status,
  suppressUnknown = false,
}: {
  status: SeerrRequestStatus
  suppressUnknown?: boolean
}) {
  if (status === "unknown" && suppressUnknown) return null
  const entry = REQUEST_LABELS[status] ?? REQUEST_LABELS.unknown
  return <Badge variant={entry.variant}>{entry.label}</Badge>
}
