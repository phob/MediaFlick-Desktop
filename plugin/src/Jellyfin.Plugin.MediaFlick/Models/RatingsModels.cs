using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Models;

public sealed record RatingTargetRequest(
    string ItemId,
    string Kind,
    string MediaType,
    string Provider,
    string ProviderId);

public sealed record RatingBatchRequest(
    int BoundaryVersion,
    IReadOnlyList<RatingTargetRequest>? Items);

public sealed record RatingBatchItemResponse(
    string ItemId,
    JsonArray Ratings,
    string Origin,
    long FetchedAt,
    string? SourceUpdatedAt,
    bool Stale);

public sealed record RatingBatchResponse(
    int BoundaryVersion,
    IReadOnlyList<RatingBatchItemResponse> Items,
    RatingQuotaResponse Quota,
    long? RetryAt,
    string? Diagnostic);

public sealed record RatingQuotaResponse(long? Limit, long? Remaining, long? ResetAt);

public sealed record RatingSourceResponse(
    string Id,
    string Label,
    string ShortLabel,
    double ScaleMax,
    string Format,
    bool Known);

public sealed record RatingProviderStatusResponse(
    bool Configured,
    bool Valid,
    string Validation,
    string? Detail,
    RatingQuotaResponse Quota,
    long? RetryAt,
    long? LastCheckedAt,
    string Storage,
    bool UsedForRatings,
    bool PreparationOnly);

public sealed record RatingsCapabilityResponse(
    int BoundaryVersion,
    ContractVersionRange SupportedBoundary,
    bool Available,
    bool Valid,
    string Status,
    string Origin,
    bool FallbackOnly,
    IReadOnlyList<string> CredentialPrecedence,
    IReadOnlyList<RatingSourceResponse> Sources,
    RatingQuotaResponse Quota,
    long? RetryAt,
    long? LastCheckedAt,
    RatingProviderStatusResponse Tmdb);

public sealed record ContractVersionRange(int Min, int Max);

public sealed record RatingAdminStatusResponse(
    RatingProviderStatusResponse Mdblist,
    RatingProviderStatusResponse Tmdb);

public sealed record RatingSecretUpdate(string ApiKey);
