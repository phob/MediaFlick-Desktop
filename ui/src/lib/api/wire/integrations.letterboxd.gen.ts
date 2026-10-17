// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ExternalProfile = {
  id: string,
  provider: string,
  profileKey: string,
  displayName: string,
  canonicalUrl: string,
  enabled: boolean,
  verificationStatus: string,
  createdAt: number,
  lastCheckedAt: number | null,
};

export type MemberReview = {
  profileId: string,
  username: string,
  displayName: string,
  profileUrl: string,
  /**
   * Canonical Letterboxd film entry; absent if the feed supplied an unsafe URL.
   */
  entryUrl: string | null,
  rating: number | null,
  /**
   * Native code strips provider HTML before this reaches React.
   */
  review: string | null,
  reviewTruncated: boolean,
  watchedDate: string | null,
  /**
   * True when a failed refresh fell back to an older in-memory feed.
   */
  stale: boolean,
};

export type ReviewLookup = {
  reviews: Array<MemberReview>,
  configuredProfiles: number,
  unavailableProfiles: number,
};
