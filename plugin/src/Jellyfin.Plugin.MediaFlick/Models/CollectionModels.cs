using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Models;

public sealed record CollectionProviderRequest(
    JsonObject Source,
    string MediaType,
    CollectionResultLimit Limit,
    IReadOnlyList<long>? OwnedTmdbIds = null);

public sealed record CollectionResultLimit(string Kind, int? Count);

public sealed record NormalizedProviderTitle(
    string MediaType,
    long TmdbId,
    string Title,
    string? OriginalTitle,
    int? Year,
    string Overview,
    string? ReleaseDate,
    int SourceOrder,
    string? PosterPath,
    string? BackdropPath,
    bool Adult);

public sealed record CollectionProviderResult(
    IReadOnlyList<NormalizedProviderTitle> Items,
    int Total,
    int Movies,
    int Series,
    string? SourceIdentity = null);

public sealed record FranchiseResolveRequest(
    IReadOnlyList<long> TmdbIds,
    IReadOnlyList<long>? CollectionIds = null);

public sealed record FranchiseMembership(long TmdbId, long? CollectionId);

public sealed record NormalizedFranchise(
    long CollectionId,
    string Name,
    string? PosterPath,
    string? BackdropPath,
    long CommittedAt,
    IReadOnlyList<NormalizedProviderTitle> Items);

public sealed record FranchiseResolveResponse(
    IReadOnlyList<NormalizedFranchise> Franchises,
    IReadOnlyList<FranchiseMembership> Memberships);

public sealed record PublicListSearchRequest(string Query);

public sealed record PublicListSelectorRequest(string Selector);

public sealed record PublicListSummary(string Id, string Name, string? Owner);

public sealed record PublicListSearchResponse(IReadOnlyList<PublicListSummary> Lists);

public sealed record PublicListValidationResponse(string Id, string Name, string? Owner);

public sealed record ExternalIdentityRequest(string MediaType, string? ImdbId, string? TvdbId);

public sealed record IdentityResolveRequest(IReadOnlyList<ExternalIdentityRequest> Items);

public sealed record ResolvedExternalIdentity(
    string MediaType,
    string Provider,
    string ProviderId,
    long TmdbId);

public sealed record IdentityResolveResponse(IReadOnlyList<ResolvedExternalIdentity> Mappings);
