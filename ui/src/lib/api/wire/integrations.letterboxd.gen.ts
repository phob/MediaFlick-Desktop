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
  entryUrl: string | null,
  rating: number | null,
  review: string | null,
  reviewTruncated: boolean,
  watchedDate: string | null,
  stale: boolean,
};

export type ReviewLookup = {
  reviews: Array<MemberReview>,
  configuredProfiles: number,
  unavailableProfiles: number,
};
