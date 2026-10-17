export interface LetterboxdReview {
  profileId: string
  username: string
  displayName: string
  profileUrl: string
  /** Canonical Letterboxd film entry; absent if the feed supplied an unsafe URL. */
  entryUrl: string | null
  rating: number | null
  /** Native code strips provider HTML before this reaches React. */
  review: string | null
  reviewTruncated: boolean
  watchedDate: string | null
  /** True when a failed refresh fell back to an older in-memory feed. */
  stale: boolean
}

export interface LetterboxdReviewsResponse {
  reviews: LetterboxdReview[]
  configuredProfiles: number
  unavailableProfiles: number
}

export interface LetterboxdProfile {
  id: string
  provider: "letterboxd"
  profileKey: string
  displayName: string
  canonicalUrl: string
  enabled: boolean
  verificationStatus: "verified" | "unverified"
  createdAt: number
  lastCheckedAt: number | null
}
