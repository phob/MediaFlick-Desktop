import { Badge } from "@/components/ui/badge"
import type { SeerrRequestStatus, SeerrStatus } from "@/lib/api"

/**
 * What Seerr already knows about a title. `unknown` is deliberately not a badge:
 * "we have never heard of this" is the default state of everything on a
 * discovery page, and a badge on every card would say nothing.
 */
const MEDIA_LABELS: Partial<Record<SeerrStatus, { label: string; variant: "default" | "secondary" | "outline" | "destructive" }>> = {
  pending: { label: "Requested", variant: "secondary" },
  processing: { label: "Downloading", variant: "secondary" },
  partial: { label: "Partly available", variant: "outline" },
  available: { label: "Available", variant: "default" },
  blacklisted: { label: "Blocked", variant: "destructive" },
}

const REQUEST_LABELS: Record<SeerrRequestStatus, { label: string; variant: "default" | "secondary" | "outline" | "destructive" }> = {
  pending: { label: "Awaiting approval", variant: "secondary" },
  approved: { label: "Approved", variant: "default" },
  declined: { label: "Declined", variant: "destructive" },
  failed: { label: "Failed", variant: "destructive" },
  unknown: { label: "Unknown", variant: "outline" },
}

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

export function SeerrRequestStatusBadge({ status }: { status: SeerrRequestStatus }) {
  const entry = REQUEST_LABELS[status] ?? REQUEST_LABELS.unknown
  return <Badge variant={entry.variant}>{entry.label}</Badge>
}
