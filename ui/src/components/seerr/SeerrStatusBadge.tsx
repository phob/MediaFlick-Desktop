import { Badge } from "@/components/ui/badge"
import type { SeerrActivity, SeerrRequestStatus, SeerrStatus } from "@/lib/api"

type StatusBadgeDescription = {
  label: string
  variant: "default" | "secondary" | "outline" | "destructive"
}

/**
 * What Seerr already knows about a title. `unknown` is deliberately not a badge:
 * "we have never heard of this" is the default state of everything on a
 * discovery page, and a badge on every card would say nothing.
 *
 * `processing` covers everything from an approved request to an imported file,
 * so on its own it only claims that the request is in progress; the activity
 * says what is actually happening.
 */
const MEDIA_LABELS = {
  unknown: null,
  pending: { label: "Requested", variant: "secondary" },
  processing: { label: "Processing", variant: "secondary" },
  partial: { label: "Partly available", variant: "outline" },
  available: { label: "Available", variant: "default" },
  blacklisted: { label: "Blocked", variant: "destructive" },
} satisfies Record<SeerrStatus, StatusBadgeDescription | null>

/**
 * Without Radarr and Sonarr connected to the Companion, every approved request
 * that is not downloading reads "Processing". The description says that the
 * request itself is working, so the plain label is not mistaken for a fault.
 */
const PROCESSING_DESCRIPTION =
  "Request approved and handed to the server's download service. It becomes available once imported. " +
  "Release and download details appear when the MediaFlick Companion is connected to Radarr and Sonarr."

const ACTIVITY_LABELS = {
  downloading: {
    label: "Downloading",
    description: "The download service is downloading this now.",
  },
  searching: {
    label: "Searching",
    description: "Released, but not downloaded yet. The download service keeps looking for a copy.",
  },
  "in-cinemas": {
    label: "In cinemas",
    description: "Only in cinemas so far. It downloads once a digital or physical release is out.",
  },
  unreleased: {
    label: "Unreleased",
    description: "Not released yet. It downloads once it is out.",
  },
  "awaiting-episodes": {
    label: "Awaiting episodes",
    description: "Every aired episode is here. New episodes download as they air.",
  },
} satisfies Record<SeerrActivity, { label: string; description: string }>

const REQUEST_LABELS = {
  pending: { label: "Awaiting approval", variant: "secondary" },
  approved: { label: "Approved", variant: "default" },
  declined: { label: "Declined", variant: "destructive" },
  failed: { label: "Failed", variant: "destructive" },
  unknown: { label: "Unknown", variant: "outline" },
} satisfies Record<SeerrRequestStatus, StatusBadgeDescription>

export function SeerrStatusBadge({
  status,
  activity,
  className,
}: {
  status: SeerrStatus
  activity?: SeerrActivity | null
  className?: string
}) {
  const entry = MEDIA_LABELS[status]
  if (!entry) return null
  if (status !== "processing") {
    return (
      <Badge variant={entry.variant} className={className}>
        {entry.label}
      </Badge>
    )
  }
  // An activity this build does not know, from a newer Companion, falls back.
  const known = activity && Object.hasOwn(ACTIVITY_LABELS, activity) ? ACTIVITY_LABELS[activity] : null
  return (
    <Badge
      variant={entry.variant}
      className={className}
      title={known?.description ?? PROCESSING_DESCRIPTION}
    >
      {known?.label ?? entry.label}
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
