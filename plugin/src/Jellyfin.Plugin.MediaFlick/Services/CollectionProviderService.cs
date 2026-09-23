using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CollectionProviderService
{
    private const int PreviewSize = 24;
    private const int MaximumAllResults = 10_000;
    private static readonly TimeSpan CacheLifetime = TimeSpan.FromHours(6);
    private readonly ITmdbTransport _tmdb;
    private readonly IMdbListTransport _mdbList;
    private readonly IRatingSecretStore _secrets;
    private readonly RatingsCacheStore _health;
    private readonly string _defaultLanguage;
    private readonly string _defaultRegion;
    private readonly SemaphoreSlim _requestGate = new(4, 4);
    private readonly SemaphoreSlim _tmdbValidationGate = new(1, 1);
    private readonly SemaphoreSlim _mdbListValidationGate = new(1, 1);
    private readonly ConcurrentDictionary<string, CachedResult> _cache = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, long> _identityCache = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, byte> _validatedThisRun =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly ILogger<CollectionProviderService> _logger;
    private readonly FailureLogGate _readinessFailures = new();

    internal CollectionProviderService(
        ITmdbTransport tmdb,
        IMdbListTransport mdbList,
        IRatingSecretStore secrets,
        RatingsCacheStore health,
        ILogger<CollectionProviderService> logger,
        string? preferredLanguage = null,
        string? preferredRegion = null)
    {
        _tmdb = tmdb;
        _mdbList = mdbList;
        _secrets = secrets;
        _health = health;
        _logger = logger;
        _defaultRegion = SafeRegion(preferredRegion) ?? "US";
        _defaultLanguage = SafeLanguage(preferredLanguage) is { } language
            ? language.Contains('-') ? language : $"{language}-{_defaultRegion}"
            : "en-US";
    }

    public bool TmdbReady => ProviderReady(RatingProviders.Tmdb)
        && _validatedThisRun.ContainsKey(RatingProviders.Tmdb);

    public bool MdbListReady => ProviderReady(RatingProviders.MdbList)
        && _validatedThisRun.ContainsKey(RatingProviders.MdbList);

    public async Task RefreshReadinessAsync(CancellationToken cancellationToken)
    {
        await Task.WhenAll(
            ValidateReadinessAsync(RatingProviders.Tmdb, cancellationToken),
            ValidateReadinessAsync(RatingProviders.MdbList, cancellationToken))
            .ConfigureAwait(false);
    }

    public void ClearProviderCache(string provider)
    {
        provider = RatingProviders.Normalize(provider);
        foreach (var entry in _cache.Where(entry => entry.Value.Provider == provider))
        {
            _cache.TryRemove(entry.Key, out _);
        }
        if (provider == RatingProviders.Tmdb)
        {
            _identityCache.Clear();
        }
        _validatedThisRun.TryRemove(provider, out _);
    }

    public async Task<PublicListSearchResponse> SearchPublicListsAsync(
        PublicListSearchRequest request,
        CancellationToken cancellationToken)
    {
        var query = request.Query.Trim();
        if (query.Length is < 2 or > 100)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "Enter 2 to 100 search characters");
        }
        var credential = await ValidMdbListCredentialAsync(cancellationToken).ConfigureAwait(false);
        var response = await _mdbList.ListItemsAsync(
            credential,
            "lists/search?limit=20&query=" + Uri.EscapeDataString(query),
            cancellationToken).ConfigureAwait(false);
        RecordProviderOutcome(RatingProviders.MdbList, response.StatusCode, response.RetryAt);
        if (!response.StatusCode.IsSuccess())
        {
            throw new GatewayException(StatusCodes.Status503ServiceUnavailable, "MDBList unavailable");
        }
        return new PublicListSearchResponse(NormalizePublicLists(response.Body));
    }

    public async Task<PublicListValidationResponse> ValidatePublicListAsync(
        PublicListSelectorRequest request,
        CancellationToken cancellationToken)
    {
        var credential = await ValidMdbListCredentialAsync(cancellationToken).ConfigureAwait(false);
        return await ResolvePublicListAsync(credential, request.Selector, cancellationToken)
            .ConfigureAwait(false);
    }

    public async Task<IdentityResolveResponse> ResolveIdentitiesAsync(
        IdentityResolveRequest request,
        CancellationToken cancellationToken)
    {
        if (request.Items.Count > 500)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "Too many identity mappings");
        }
        var credential = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
        var wanted = request.Items.Select(ExternalIdentity).Where(item => item is not null)
            .Cast<(string MediaType, string Provider, string ProviderId)>()
            .Distinct()
            .ToArray();
        var mappings = new ConcurrentBag<ResolvedExternalIdentity>();
        foreach (var chunk in wanted.Chunk(4))
        {
            var tasks = chunk.Select(async item =>
            {
                var cacheKey = string.Join('|', item.MediaType, item.Provider, item.ProviderId);
                if (!_identityCache.TryGetValue(cacheKey, out var tmdbId))
                {
                    tmdbId = await FindTmdbIdAsync(credential, item, cancellationToken)
                        .ConfigureAwait(false) ?? 0;
                    if (tmdbId > 0)
                    {
                        _identityCache[cacheKey] = tmdbId;
                    }
                }
                if (tmdbId > 0)
                {
                    mappings.Add(new(item.MediaType, item.Provider, item.ProviderId, tmdbId));
                }
            });
            await Task.WhenAll(tasks).ConfigureAwait(false);
        }
        return new IdentityResolveResponse(mappings
            .OrderBy(mapping => mapping.MediaType, StringComparer.Ordinal)
            .ThenBy(mapping => mapping.Provider, StringComparer.Ordinal)
            .ThenBy(mapping => mapping.ProviderId, StringComparer.Ordinal)
            .ToArray());
    }

    public async Task<ArtworkResponse> ArtworkAsync(
        string size,
        string path,
        CancellationToken cancellationToken)
    {
        var response = await _tmdb.GetArtworkAsync(size, path, cancellationToken)
            .ConfigureAwait(false);
        if (!response.StatusCode.IsSuccess()
            || response.ContentType is not ("image/jpeg" or "image/png" or "image/webp"))
        {
            throw new GatewayException(StatusCodes.Status404NotFound, "Artwork not available");
        }
        return response;
    }

    public async Task<CollectionProviderResult> PreviewAsync(
        CollectionProviderRequest request,
        CancellationToken cancellationToken)
        => await ResolveAsync(request, PreviewSize, cancellationToken).ConfigureAwait(false);

    public Task<CollectionProviderResult> ResultsAsync(
        CollectionProviderRequest request,
        CancellationToken cancellationToken)
        => ResolveAsync(request, null, cancellationToken);

    public async Task<FranchiseResolveResponse> FranchisesAsync(
        FranchiseResolveRequest request,
        CancellationToken cancellationToken)
    {
        var ids = request.TmdbIds.Where(id => id > 0).Distinct().Take(10_000).ToArray();
        var credential = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
        var collectionIds = request.CollectionIds?
            .Where(id => id > 0)
            .Distinct()
            .Take(10_000)
            .ToHashSet() ?? [];
        var memberships = new List<FranchiseMembership>(ids.Length);
        foreach (var chunk in ids.Chunk(4))
        {
            var chunkIds = chunk.ToArray();
            var tasks = chunkIds.Select(id => MovieCollectionIdAsync(credential, id, cancellationToken));
            var resolved = await Task.WhenAll(tasks).ConfigureAwait(false);
            for (var index = 0; index < resolved.Length; index++)
            {
                var collectionId = resolved[index];
                memberships.Add(new FranchiseMembership(chunkIds[index], collectionId));
                if (collectionId is > 0)
                {
                    collectionIds.Add(collectionId.Value);
                }
            }
        }
        var franchises = new List<NormalizedFranchise>();
        foreach (var collectionId in collectionIds.Order())
        {
            var detail = await TmdbCollectionAsync(credential, collectionId, cancellationToken)
                .ConfigureAwait(false);
            franchises.Add(new NormalizedFranchise(
                collectionId,
                JsonRead.String(detail, "name") ?? $"Collection {collectionId}",
                JsonRead.String(detail, "poster_path"),
                JsonRead.String(detail, "backdrop_path"),
                DateTimeOffset.UtcNow.ToUnixTimeSeconds(),
                NormalizeTmdbRows(detail["parts"] as JsonArray, "movie")));
        }
        return new FranchiseResolveResponse(franchises, memberships);
    }

    private async Task<CollectionProviderResult> ResolveAsync(
        CollectionProviderRequest request,
        int? fetchLimit,
        CancellationToken cancellationToken)
    {
        ValidateRequest(request);
        var cacheKey = JsonSerializer.Serialize(new { Request = request, FetchLimit = fetchLimit }, CompanionJson.CamelCase);
        var kind = JsonRead.String(request.Source, "kind");
        var provider = kind == "mdbListPublicList" ? RatingProviders.MdbList : RatingProviders.Tmdb;
        if (provider == RatingProviders.MdbList)
        {
            _ = await ValidMdbListCredentialAsync(cancellationToken).ConfigureAwait(false);
        }
        else
        {
            _ = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
        }
        if (_cache.TryGetValue(cacheKey, out var cached)
            && DateTimeOffset.UtcNow - cached.StoredAt < CacheLifetime)
        {
            return cached.Result;
        }
        await _requestGate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (_cache.TryGetValue(cacheKey, out cached)
                && DateTimeOffset.UtcNow - cached.StoredAt < CacheLifetime)
            {
                return cached.Result;
            }
            var result = kind switch
            {
                "tmdbDiscover" => await TmdbDiscoverAsync(request, fetchLimit, cancellationToken)
                    .ConfigureAwait(false),
                "tmdbCollection" => await ExactCollectionAsync(request, fetchLimit, cancellationToken)
                    .ConfigureAwait(false),
                "mdbListPublicList" => await MdbListAsync(request, fetchLimit, cancellationToken)
                    .ConfigureAwait(false),
                _ => throw new GatewayException(
                    StatusCodes.Status400BadRequest,
                    "Unsupported collection source")
            };
            _cache[cacheKey] = new(DateTimeOffset.UtcNow, provider, result);
            return result;
        }
        finally
        {
            _requestGate.Release();
        }
    }

    private async Task<CollectionProviderResult> TmdbDiscoverAsync(
        CollectionProviderRequest request,
        int? maximumItems,
        CancellationToken cancellationToken)
    {
        var mediaType = NormalizeMediaType(request.MediaType);
        if (mediaType == "mixed")
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "TMDB Discover requires one media type");
        }
        var credential = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
        var requestedLimit = RequestedLimit(request.Limit);
        var fetchLimit = EffectiveFetchLimit(requestedLimit, maximumItems);
        var parameters = request.Source["parameters"] as JsonObject;
        var query = DiscoverQuery(parameters, mediaType);
        var rows = new List<NormalizedProviderTitle>();
        var total = 0;
        var totalPages = 1;
        for (var page = 1; page <= totalPages && rows.Count < fetchLimit; page += 1)
        {
            query["page"] = page.ToString(CultureInfo.InvariantCulture);
            var response = await _tmdb.GetAsync(
                credential,
                DiscoverPath(mediaType, parameters),
                query,
                cancellationToken).ConfigureAwait(false);
            var body = RequireTmdb(response);
            total = (int)Math.Min(JsonRead.Positive(body["total_results"]) ?? 0, int.MaxValue);
            totalPages = (int)Math.Min(JsonRead.Positive(body["total_pages"]) ?? 1, 500);
            rows.AddRange(NormalizeTmdbRows(body["results"] as JsonArray, mediaType));
        }
        return Result(rows.Take(fetchLimit), Math.Min(total, requestedLimit));
    }

    private async Task<CollectionProviderResult> ExactCollectionAsync(
        CollectionProviderRequest request,
        int? maximumItems,
        CancellationToken cancellationToken)
    {
        var id = JsonRead.Positive(request.Source["collectionId"])
            ?? throw new GatewayException(StatusCodes.Status400BadRequest, "Invalid TMDB collection id");
        var credential = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
        var detail = await TmdbCollectionAsync(credential, id, cancellationToken).ConfigureAwait(false);
        var rows = NormalizeTmdbRows(detail["parts"] as JsonArray, "movie");
        var includeUnreleased = IncludeUnreleased(request.Source);
        if (!includeUnreleased)
        {
            var today = DateOnly.FromDateTime(DateTime.Today);
            var owned = request.OwnedTmdbIds?.Where(id => id > 0).ToHashSet() ?? [];
            rows = rows.Where(item => owned.Contains(item.TmdbId)
                    || item.ReleaseDate is { } date
                    && DateOnly.TryParse(date, CultureInfo.InvariantCulture, out var release)
                    && release <= today)
                .ToArray();
        }
        var requestedLimit = RequestedLimit(request.Limit);
        var fetchLimit = EffectiveFetchLimit(requestedLimit, maximumItems);
        return Result(rows.Take(fetchLimit), Math.Min(rows.Count, requestedLimit));
    }

    private async Task<CollectionProviderResult> MdbListAsync(
        CollectionProviderRequest request,
        int? maximumItems,
        CancellationToken cancellationToken)
    {
        var selector = JsonRead.String(request.Source, "listId") ?? string.Empty;
        var credential = await ValidMdbListCredentialAsync(cancellationToken).ConfigureAwait(false);
        var list = await ResolvePublicListAsync(credential, selector, cancellationToken)
            .ConfigureAwait(false);
        var rows = new List<NormalizedProviderTitle>();
        var offset = 0;
        var requestedLimit = RequestedLimit(request.Limit);
        var fetchLimit = EffectiveFetchLimit(requestedLimit, maximumItems);
        var mediaType = NormalizeMediaType(request.MediaType);
        while (rows.Count < fetchLimit)
        {
            var typeFilter = mediaType switch
            {
                "movie" => "&mediatype=movie",
                "series" => "&mediatype=show",
                _ => string.Empty
            };
            var resource = $"lists/{list.Id}/items?unified=true{typeFilter}&limit=1000&offset={offset}";
            var response = await _mdbList.ListItemsAsync(credential, resource, cancellationToken)
                .ConfigureAwait(false);
            RecordProviderOutcome(RatingProviders.MdbList, response.StatusCode, response.RetryAt);
            if (response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.NotFound
                or HttpStatusCode.Unauthorized)
            {
                throw new GatewayException(StatusCodes.Status404NotFound, "List not available");
            }
            if (!response.StatusCode.IsSuccess())
            {
                throw new GatewayException(StatusCodes.Status503ServiceUnavailable, "MDBList unavailable");
            }
            var page = NormalizeMdbListRows(response.Body, rows.Count, mediaType);
            rows.AddRange(page.Items);
            offset += page.SourceCount;
            if (!response.HasMore || page.SourceCount == 0)
            {
                break;
            }
        }
        return Result(rows.Take(fetchLimit), Math.Min(rows.Count, requestedLimit), list.Id);
    }

    private Task<string> ValidTmdbCredentialAsync(CancellationToken cancellationToken)
        => ValidCredentialAsync(
            RatingProviders.Tmdb,
            "TMDB unavailable",
            _tmdbValidationGate,
            async (credential, token) => ProviderOutcome.Of(await _tmdb.GetAsync(
                credential,
                "3/configuration",
                new Dictionary<string, string>(),
                token).ConfigureAwait(false)),
            cancellationToken);

    private Task<string> ValidMdbListCredentialAsync(CancellationToken cancellationToken)
        => ValidCredentialAsync(
            RatingProviders.MdbList,
            "MDBList unavailable",
            _mdbListValidationGate,
            async (credential, token) => ProviderOutcome.Of(
                await _mdbList.ValidateAsync(credential, token).ConfigureAwait(false)),
            cancellationToken);

    /// <summary>
    /// Returns the saved credential once it has been proven valid in this
    /// plugin run. Validation is serialized per provider and respects the
    /// persisted retry timing; outages keep an established key valid.
    /// </summary>
    private async Task<string> ValidCredentialAsync(
        string provider,
        string unavailable,
        SemaphoreSlim gate,
        Func<string, CancellationToken, Task<ProviderOutcome>> validate,
        CancellationToken cancellationToken)
    {
        var credential = ReadCredential(provider, unavailable);
        ThrowIfBackedOff(provider, unavailable);
        if (_validatedThisRun.ContainsKey(provider))
        {
            return credential;
        }
        await gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (_validatedThisRun.ContainsKey(provider))
            {
                return credential;
            }
            ThrowIfBackedOff(provider, unavailable);
            var outcome = await validate(credential, cancellationToken).ConfigureAwait(false);
            _health.SetHealth(provider, ProviderHealthPolicy.AfterValidation(
                _health.Health(provider),
                outcome,
                Now(),
                preserveValidOnTransientFailure: true,
                rateLimitProvesCredential: provider == RatingProviders.MdbList));
            if (!outcome.StatusCode.IsSuccess())
            {
                throw new GatewayException(StatusCodes.Status503ServiceUnavailable, unavailable);
            }
            _validatedThisRun[provider] = 0;
            return credential;
        }
        finally
        {
            gate.Release();
        }
    }

    private async Task ValidateReadinessAsync(
        string provider,
        CancellationToken cancellationToken)
    {
        try
        {
            if (!_secrets.IsConfigured(provider))
            {
                // A removed credential is not an outage to recover from.
                _readinessFailures.Recovered(provider);
                return;
            }
            if (provider == RatingProviders.Tmdb)
            {
                _ = await ValidTmdbCredentialAsync(cancellationToken).ConfigureAwait(false);
            }
            else
            {
                _ = await ValidMdbListCredentialAsync(cancellationToken).ConfigureAwait(false);
            }
            if (_readinessFailures.Recovered(provider))
            {
                _logger.LogInformation("{Provider} collection features are available again", provider);
            }
        }
        catch (GatewayException exception)
        {
            // Readiness is represented by TmdbReady/MdbListReady. The info
            // probe remains successful when an optional provider is down.
            // Every info probe repeats this check, so warn once per state;
            // the transport logs the upstream cause. Gateway messages on this
            // path are fixed plugin text, never upstream content.
            var validation = _health.Health(provider).Validation;
            _logger.Log(
                _readinessFailures.Failure(provider, validation),
                "{Provider} collection features are unavailable: {Reason} (provider state {Validation})",
                provider,
                exception.Message,
                validation);
        }
        catch (InvalidOperationException exception)
        {
            // An unreadable saved secret is unavailable without exposing its
            // storage failure to an authenticated Desktop client.
            _logger.Log(
                _readinessFailures.Failure(provider, "unreadable"),
                exception,
                "{Provider} collection readiness could not read the saved credential",
                provider);
        }
    }

    private async Task<long?> MovieCollectionIdAsync(
        string credential,
        long tmdbId,
        CancellationToken cancellationToken)
    {
        var response = await _tmdb.GetAsync(
            credential,
            $"3/movie/{tmdbId}",
            BaseQuery(),
            cancellationToken).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotFound)
        {
            return null;
        }
        return JsonRead.Positive((RequireTmdb(response)["belongs_to_collection"] as JsonObject)?["id"]);
    }

    private async Task<JsonObject> TmdbCollectionAsync(
        string credential,
        long collectionId,
        CancellationToken cancellationToken)
    {
        var response = await _tmdb.GetAsync(
            credential,
            $"3/collection/{collectionId}",
            BaseQuery(),
            cancellationToken).ConfigureAwait(false);
        return RequireTmdb(response);
    }

    private async Task<PublicListValidationResponse> ResolvePublicListAsync(
        string credential,
        string selector,
        CancellationToken cancellationToken)
    {
        selector = NormalizeListSelector(selector)
            ?? throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "Invalid MDBList public-list selector");
        if (!SafeListSelector(selector))
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "Invalid MDBList public-list selector");
        }
        var response = await _mdbList.ListItemsAsync(
            credential,
            "lists/" + selector,
            cancellationToken).ConfigureAwait(false);
        RecordProviderOutcome(RatingProviders.MdbList, response.StatusCode, response.RetryAt);
        if (response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.NotFound
            or HttpStatusCode.Unauthorized || response.Body is not JsonObject detail)
        {
            throw new GatewayException(StatusCodes.Status404NotFound, "List not available");
        }
        if (!response.StatusCode.IsSuccess()
            || IsPrivateList(detail))
        {
            throw new GatewayException(
                response.StatusCode.IsSuccess()
                    ? StatusCodes.Status404NotFound
                    : StatusCodes.Status503ServiceUnavailable,
                response.StatusCode.IsSuccess() ? "List not available" : "MDBList unavailable");
        }
        var id = JsonRead.Positive(detail["id"]) ?? JsonRead.Positive(detail["listid"]);
        if (id is null)
        {
            throw new GatewayException(StatusCodes.Status404NotFound, "List not available");
        }
        return new PublicListValidationResponse(
            id.Value.ToString(CultureInfo.InvariantCulture),
            JsonRead.String(detail, "name") ?? JsonRead.String(detail, "title") ?? $"List {id}",
            JsonRead.String(detail, "username") ?? JsonRead.String(detail["user"] as JsonObject, "username"));
    }

    private async Task<long?> FindTmdbIdAsync(
        string credential,
        (string MediaType, string Provider, string ProviderId) item,
        CancellationToken cancellationToken)
    {
        var response = await _tmdb.GetAsync(
            credential,
            "3/find/" + item.ProviderId,
            new Dictionary<string, string>
            {
                ["language"] = _defaultLanguage,
                ["external_source"] = item.Provider == "imdb" ? "imdb_id" : "tvdb_id"
            },
            cancellationToken).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotFound)
        {
            return null;
        }
        var body = RequireTmdb(response);
        var rows = body[item.MediaType == "movie" ? "movie_results" : "tv_results"] as JsonArray;
        return rows?.OfType<JsonObject>().Select(row => JsonRead.Positive(row["id"]))
            .FirstOrDefault(id => id is > 0);
    }

    private string ReadCredential(string provider, string unavailable)
    {
        try
        {
            return _secrets.Get(provider) is { Length: > 0 } credential
                ? credential
                : throw new GatewayException(StatusCodes.Status503ServiceUnavailable, unavailable);
        }
        catch (InvalidOperationException)
        {
            throw new GatewayException(StatusCodes.Status503ServiceUnavailable, unavailable);
        }
    }

    private bool ProviderReady(string provider)
    {
        try
        {
            var state = _health.Health(provider);
            return _secrets.IsConfigured(provider)
                && state.Valid
                && !ProviderHealthPolicy.IsBackedOff(state, Now());
        }
        catch (InvalidOperationException)
        {
            return false;
        }
    }

    private JsonObject RequireTmdb(TmdbResponse response)
    {
        if (!response.StatusCode.IsSuccess() || response.Body is not JsonObject body)
        {
            RecordProviderOutcome(RatingProviders.Tmdb, response.StatusCode, response.RetryAt);
            throw new GatewayException(StatusCodes.Status503ServiceUnavailable, "TMDB unavailable");
        }
        RecordProviderOutcome(RatingProviders.Tmdb, response.StatusCode, response.RetryAt);
        return body;
    }

    private void ThrowIfBackedOff(string provider, string unavailable)
    {
        if (ProviderHealthPolicy.IsBackedOff(_health.Health(provider), Now()))
        {
            throw new GatewayException(StatusCodes.Status503ServiceUnavailable, unavailable);
        }
    }

    private void RecordProviderOutcome(
        string provider,
        HttpStatusCode status,
        long? upstreamRetryAt)
    {
        // MDBList answers 401/403 for private lists, which says nothing about
        // the administrator key; TMDB uses them only for a rejected key.
        var rejectionInvalidatesKey = provider != RatingProviders.MdbList;
        var next = ProviderHealthPolicy.AfterRequest(
            _health.Health(provider),
            status,
            upstreamRetryAt,
            Now(),
            rejectionInvalidatesKey);
        if (next is null)
        {
            return;
        }
        if (rejectionInvalidatesKey && ProviderHealthPolicy.IsRejection(status))
        {
            _validatedThisRun.TryRemove(provider, out _);
        }
        _health.SetHealth(provider, next);
    }

    private static long Now() => DateTimeOffset.UtcNow.ToUnixTimeSeconds();

    private IReadOnlyDictionary<string, string> BaseQuery()
        => new Dictionary<string, string> { ["language"] = _defaultLanguage };

    private static string DiscoverPath(string mediaType, JsonObject? parameters)
    {
        if (JsonRead.String(parameters, "feed")?.Equals("trending", StringComparison.OrdinalIgnoreCase) == true)
        {
            var window = JsonRead.String(parameters, "window") == "day" ? "day" : "week";
            return $"3/trending/{(mediaType == "series" ? "tv" : "movie")}/{window}";
        }
        return $"3/discover/{(mediaType == "series" ? "tv" : "movie")}";
    }

    private Dictionary<string, string> DiscoverQuery(JsonObject? parameters, string mediaType)
    {
        var query = new Dictionary<string, string>(BaseQuery())
        {
            ["include_adult"] = "false",
            ["sort_by"] = JsonRead.String(parameters, "sortBy") ?? "popularity.desc"
        };
        Copy(parameters, query, "genre", "with_genres");
        Copy(parameters, query, "primaryReleaseYear", "primary_release_year");
        Copy(parameters, query, "voteCountGte", "vote_count.gte");
        Copy(parameters, query, "language", "language");
        Copy(parameters, query, "region", "region");
        Copy(parameters, query, "region", "watch_region");
        Copy(parameters, query, "withKeywords", "with_keywords");
        Copy(parameters, query, "originalLanguage", "with_original_language");
        if (JsonRead.String(parameters, "watchProvider") is { } provider
            && WatchProvider(provider) is { } providerId)
        {
            query["with_watch_providers"] = providerId;
            query.TryAdd("watch_region", _defaultRegion);
            query["with_watch_monetization_types"] = "flatrate|free|ads";
        }
        ApplyReleaseWindow(parameters, query, mediaType);
        return query;
    }

    private static string? WatchProvider(string value)
        => value.ToLowerInvariant() switch
        {
            "netflix" => "8",
            "prime-video" => "9",
            "disney-plus" => "337",
            "apple-tv-plus" or "apple-tv" => "350",
            "max" => "1899",
            _ => null
        };

    private static string? SafeLanguage(string? value)
    {
        value = value?.Trim();
        return value is { Length: >= 2 and <= 16 }
            && value.All(character => char.IsAsciiLetter(character) || character == '-')
                ? value
                : null;
    }

    private static string? SafeRegion(string? value)
    {
        value = value?.Trim().ToUpperInvariant();
        return value is { Length: 2 } && value.All(char.IsAsciiLetter) ? value : null;
    }

    private static void ApplyReleaseWindow(
        JsonObject? parameters,
        IDictionary<string, string> query,
        string mediaType)
    {
        var window = JsonRead.String(parameters, "releaseWindow");
        if (window is null)
        {
            return;
        }
        var today = DateOnly.FromDateTime(DateTime.UtcNow);
        var (start, end) = window switch
        {
            "recent" => (today.AddDays(-120), today),
            "theaters" => (today.AddDays(-45), today),
            "opening-soon" => (today.AddDays(1), today.AddDays(60)),
            "upcoming" => (today.AddDays(1), today.AddYears(1)),
            "on-air" => (today.AddDays(-30), today),
            "airing-soon" => (today.AddDays(1), today.AddDays(60)),
            _ => (today, today)
        };
        if (window is not ("recent" or "theaters" or "opening-soon" or "upcoming" or "on-air" or "airing-soon"))
        {
            return;
        }
        var field = mediaType == "series" ? "first_air_date" : "primary_release_date";
        query[field + ".gte"] = start.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
        query[field + ".lte"] = end.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
    }

    private static void Copy(JsonObject? source, IDictionary<string, string> target, string from, string to)
    {
        if (source?[from] is { } value)
        {
            target[to] = value.ToString();
        }
    }

    private static IReadOnlyList<NormalizedProviderTitle> NormalizeTmdbRows(
        JsonArray? rows,
        string mediaType)
    {
        if (rows is null)
        {
            return [];
        }
        var result = new List<NormalizedProviderTitle>();
        foreach (var row in rows.OfType<JsonObject>())
        {
            var id = JsonRead.Positive(row["id"]);
            if (id is null || JsonRead.Flag(row["adult"]) == true)
            {
                continue;
            }
            var type = mediaType == "mixed"
                ? NormalizeMediaType(JsonRead.String(row, "media_type") ?? string.Empty)
                : mediaType;
            var release = JsonRead.String(row, type == "series" ? "first_air_date" : "release_date");
            result.Add(new NormalizedProviderTitle(
                type,
                id.Value,
                JsonRead.String(row, type == "series" ? "name" : "title") ?? $"Title {id}",
                JsonRead.String(row, type == "series" ? "original_name" : "original_title"),
                JsonRead.Year(release),
                JsonRead.String(row, "overview") ?? string.Empty,
                release,
                result.Count,
                JsonRead.String(row, "poster_path"),
                JsonRead.String(row, "backdrop_path"),
                false));
        }
        return result;
    }

    private static NormalizedMdbListPage NormalizeMdbListRows(
        JsonNode? body,
        int offset,
        string requestedMediaType)
    {
        var rows = new List<(JsonNode? Row, string? MediaType)>();
        switch (body)
        {
            case JsonArray array:
                rows.AddRange(array.Select(row => (row, (string?)null)));
                break;
            case JsonObject detail when detail["items"] is JsonArray items:
                rows.AddRange(items.Select(row => (row, (string?)null)));
                break;
            case JsonObject detail:
                rows.AddRange(BucketedMdbListRows(detail));
                break;
        }
        var result = new List<NormalizedProviderTitle>();
        foreach (var (node, bucketMediaType) in rows)
        {
            if (node is not JsonObject row)
            {
                continue;
            }
            var id = JsonRead.Positive(row["id"]) ?? JsonRead.Positive(row["tmdbid"])
                ?? JsonRead.Positive((row["ids"] as JsonObject)?["tmdb"]);
            if (id is null || JsonRead.Flag(row["adult"]) == true)
            {
                continue;
            }
            var type = bucketMediaType switch
            {
                "movie" => "movie",
                "series" => "series",
                _ => NormalizeMediaType(
                    JsonRead.String(row, "mediatype") ?? JsonRead.String(row, "type") ?? "movie")
            };
            if (requestedMediaType != "mixed" && type != requestedMediaType)
            {
                continue;
            }
            var release = JsonRead.String(row, "released") ?? JsonRead.String(row, "release_date");
            result.Add(new NormalizedProviderTitle(
                type,
                id.Value,
                JsonRead.String(row, "title") ?? JsonRead.String(row, "name") ?? $"Title {id}",
                null,
                JsonRead.Year(release),
                JsonRead.String(row, "description") ?? string.Empty,
                release,
                offset + result.Count,
                JsonRead.String(row, "poster"),
                JsonRead.String(row, "backdrop"),
                false));
        }
        return new NormalizedMdbListPage(result, rows.Count);
    }

    private static IEnumerable<(JsonNode? Row, string? MediaType)> BucketedMdbListRows(
        JsonObject detail)
    {
        if (detail["movies"] is JsonArray movies)
        {
            foreach (var row in movies)
            {
                yield return (row, "movie");
            }
        }
        if (detail["shows"] is JsonArray shows)
        {
            foreach (var row in shows)
            {
                yield return (row, "series");
            }
        }
    }

    /// <summary>
    /// A `private` marker that is not clearly false hides the list, so an
    /// unreadable marker fails closed rather than exposing a private list.
    /// </summary>
    internal static bool IsPrivateList(JsonObject detail)
        => (detail["private"] is { } marker && JsonRead.Flag(marker) != false)
            || JsonRead.String(detail, "privacy")?.Trim()
                .Equals("private", StringComparison.OrdinalIgnoreCase) == true;

    /// <summary>
    /// `includeUnreleased` is client input: it must be a JSON boolean when
    /// present, and anything else is a request error rather than a crash.
    /// </summary>
    internal static bool IncludeUnreleased(JsonObject source)
    {
        var node = source["includeUnreleased"];
        if (node is null)
        {
            return false;
        }

        return node is JsonValue value && value.TryGetValue<bool>(out var include)
            ? include
            : throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "includeUnreleased must be true or false");
    }

    private static IReadOnlyList<PublicListSummary> NormalizePublicLists(JsonNode? body)
    {
        var rows = body switch
        {
            JsonArray array => array,
            JsonObject detail when detail["lists"] is JsonArray lists => lists,
            JsonObject detail when detail["results"] is JsonArray results => results,
            _ => null
        };
        if (rows is null)
        {
            return [];
        }
        return rows.OfType<JsonObject>().Select(row =>
        {
            var id = JsonRead.Positive(row["id"]) ?? JsonRead.Positive(row["listid"]);
            return id is null ? null : new PublicListSummary(
                id.Value.ToString(CultureInfo.InvariantCulture),
                JsonRead.String(row, "name") ?? JsonRead.String(row, "title") ?? $"List {id}",
                JsonRead.String(row, "username") ?? JsonRead.String(row["user"] as JsonObject, "username"));
        }).OfType<PublicListSummary>().Take(20).ToArray();
    }

    private static CollectionProviderResult Result(
        IEnumerable<NormalizedProviderTitle> source,
        int total,
        string? sourceIdentity = null)
    {
        var seen = new HashSet<(string Type, long Id)>();
        var items = source
            .Where(item => !item.Adult && seen.Add((item.MediaType, item.TmdbId)))
            .Select((item, index) => item with { SourceOrder = index })
            .ToArray();
        return new CollectionProviderResult(
            items,
            Math.Max(total, items.Length),
            items.Count(item => item.MediaType == "movie"),
            items.Count(item => item.MediaType == "series"),
            sourceIdentity);
    }

    private static void ValidateRequest(CollectionProviderRequest request)
    {
        _ = NormalizeMediaType(request.MediaType);
        _ = RequestedLimit(request.Limit);
    }

    private static int RequestedLimit(CollectionResultLimit limit)
        => limit.Kind.ToLowerInvariant() switch
        {
            "all" => MaximumAllResults,
            "maximum" when limit.Count is >= 1 and <= 500 => limit.Count.Value,
            _ => throw new GatewayException(StatusCodes.Status400BadRequest, "Invalid result limit")
        };

    private static int EffectiveFetchLimit(int requestedLimit, int? maximumItems)
        => maximumItems is { } maximum ? Math.Min(requestedLimit, maximum) : requestedLimit;

    private static string NormalizeMediaType(string value)
        => value.Trim().ToLowerInvariant() switch
        {
            "movie" => "movie",
            "series" or "tv" or "show" => "series",
            "mixed" => "mixed",
            _ => throw new GatewayException(StatusCodes.Status400BadRequest, "Invalid media type")
        };

    private static bool SafeListSelector(string value)
    {
        if (value.Length is 0 or > 160 || value.Contains("share", StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }
        var segments = value.Split('/');
        return segments.Length is 1 or 2
            && segments.All(segment => segment.Length > 0
                && segment.All(character => char.IsAsciiLetterOrDigit(character)
                    || character is '-' or '_'));
    }

    private static string? NormalizeListSelector(string value)
    {
        value = value.Trim().ToLowerInvariant();
        if (SafeListSelector(value))
        {
            return value;
        }
        if (!Uri.TryCreate(value, UriKind.Absolute, out var uri)
            || uri.Scheme != Uri.UriSchemeHttps
            || uri.Host is not ("mdblist.com" or "www.mdblist.com")
            || uri.Query.Length > 0
            || uri.Fragment.Length > 0
            || uri.AbsolutePath.EndsWith('/'))
        {
            return null;
        }
        var segments = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries);
        return segments is ["lists", var owner, var name]
            && SafeListSelector(owner + "/" + name)
                ? owner + "/" + name
                : null;
    }

    private static (string MediaType, string Provider, string ProviderId)? ExternalIdentity(
        ExternalIdentityRequest request)
    {
        var mediaType = NormalizeMediaType(request.MediaType);
        if (mediaType == "mixed")
        {
            return null;
        }
        if (ImdbIds.Normalize(request.ImdbId) is { } imdb)
        {
            return (mediaType, "imdb", imdb);
        }
        if (mediaType == "series" && request.TvdbId is { } tvdb
            && tvdb.Length is >= 1 and <= 20 && tvdb.All(char.IsAsciiDigit))
        {
            return (mediaType, "tvdb", tvdb);
        }
        return null;
    }

    private sealed record CachedResult(
        DateTimeOffset StoredAt,
        string Provider,
        CollectionProviderResult Result);

    private sealed record NormalizedMdbListPage(
        IReadOnlyList<NormalizedProviderTitle> Items,
        int SourceCount);
}
