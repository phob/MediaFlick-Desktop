using System.Collections.Concurrent;
using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Runs Seerr calls as the Seerr user mapped to the signed-in Jellyfin user
/// and reshapes every answer into the typed Desktop contract.
/// </summary>
public sealed class SeerrGateway
{
    private const ulong Admin = 2;
    private const ulong Request = 32;
    private const ulong AutoApprove = 128;
    private const ulong AutoApproveMovie = 256;
    private const ulong AutoApproveTv = 512;
    private const ulong Request4k = 1024;
    private const ulong Request4kMovie = 2048;
    private const ulong Request4kTv = 4096;
    private const ulong RequestAdvanced = 8192;
    private const ulong AutoApprove4k = 32768;
    private const ulong AutoApprove4kMovie = 65536;
    private const ulong AutoApprove4kTv = 131072;
    private const ulong RequestMovie = 262144;
    private const ulong RequestTv = 524288;
    private const int UserPageSize = 50;
    private const int MaxUserScan = 10_000;
    internal static readonly TimeSpan MappingLifetime = TimeSpan.FromMinutes(10);

    /// <summary>
    /// How long an unmapped Jellyfin user is remembered, so repeated requests
    /// do not rescan every Seerr user. Short enough that an administrator's
    /// import is picked up quickly.
    /// </summary>
    internal static readonly TimeSpan MissingUserLifetime = TimeSpan.FromMinutes(2);

    private readonly ISeerrTransport _seerr;
    private readonly ILogger<SeerrGateway> _logger;
    private readonly TimeProvider _time;
    private readonly ArrFactsLookup? _arr;
    private readonly ConcurrentDictionary<Guid, MappingRecord> _mappings = new();
    private readonly ConcurrentDictionary<Guid, DateTimeOffset> _missing = new();
    private readonly SemaphoreSlim _lookupGate = new(1, 1);

    internal SeerrGateway(
        ISeerrTransport seerr,
        ILogger<SeerrGateway> logger,
        TimeProvider? timeProvider = null,
        ArrFactsLookup? arr = null)
    {
        _seerr = seerr;
        _logger = logger;
        _time = timeProvider ?? TimeProvider.System;
        _arr = arr;
    }

    public async Task<SeerrStatusResponse> StatusAsync(
        Guid jellyfinUserId,
        CancellationToken cancellationToken)
    {
        var seerrUserId = await ResolveUserAsync(jellyfinUserId, cancellationToken)
            .ConfigureAwait(false);
        var user = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/auth/me",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        JsonNode? quota;
        try
        {
            quota = await SendMappedAsync(
                HttpMethod.Get,
                $"api/v1/user/{seerrUserId}/quota",
                null,
                seerrUserId,
                cancellationToken).ConfigureAwait(false);
        }
        catch (GatewayException exception)
        {
            // Permissions remain useful if this optional usage counter is
            // temporarily unavailable. The HTTP client logs upstream outages.
            _logger.LogDebug(
                "Seerr quota lookup failed with status {StatusCode}; returning permissions without quota",
                exception.StatusCode);
            quota = null;
        }
        var settings = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/settings/public",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        return ShapeStatus(jellyfinUserId, seerrUserId, user, settings, quota);
    }

    internal static SeerrStatusResponse ShapeStatus(
        Guid jellyfinUserId,
        int seerrUserId,
        JsonObject? user,
        JsonObject? settings,
        JsonNode? quota)
    {
        var permissions = JsonRead.UInt64(user, "permissions") ?? 0;
        var movie4k = IsTrue(settings, "movie4kEnabled");
        var tv4k = IsTrue(settings, "series4kEnabled");
        return new SeerrStatusResponse(
            true,
            true,
            new SeerrInstanceResponse(
                movie4k,
                tv4k,
                IsTrue(settings, "partialRequestsEnabled")),
            new SeerrUserResponse(
                seerrUserId,
                PreferredUserName(user),
                JsonRead.String(user, "avatar"),
                jellyfinUserId.ToString("N")),
            Capabilities(permissions, movie4k, tv4k),
            ShapeQuota(quota));
    }

    public async Task<SeerrPageResponse<SeerrResultResponse>> SearchAsync(
        Guid jellyfinUserId,
        string query,
        int page,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(query))
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "query is required");
        }

        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var path = string.Create(
            CultureInfo.InvariantCulture,
            $"api/v1/search?query={Uri.EscapeDataString(query.Trim())}&page={Math.Max(1, page)}");
        var response = await SendMappedAsync(
            HttpMethod.Get,
            path,
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeSearchPage(
            response,
            await FactsAsync(Titles((response as JsonObject)?["results"]), cancellationToken)
                .ConfigureAwait(false));
    }

    public async Task<SeerrPageResponse<SeerrResultResponse>> PersonCreditsAsync(
        Guid jellyfinUserId,
        int tmdbId,
        CancellationToken cancellationToken)
    {
        ValidatePositive(tmdbId, "TMDB person id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var credits = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/person/{tmdbId}/combined_credits",
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        var responseId = JsonRead.Int32(credits, "id");
        if (responseId is > 0 && responseId != tmdbId)
        {
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                "Seerr returned credits for a different TMDB person");
        }
        return ShapePersonCredits(
            credits,
            await FactsAsync(Titles(credits?["cast"]), cancellationToken).ConfigureAwait(false));
    }

    public async Task<SeerrPageResponse<SeerrResultResponse>> DiscoverAsync(
        Guid jellyfinUserId,
        string kind,
        int page,
        int? genre,
        string? sortBy,
        int? voteAverageGte,
        int? releaseDecade,
        string? mediaType,
        string? timeWindow,
        CancellationToken cancellationToken)
    {
        var path = BuildDiscoverPath(
            kind,
            page,
            genre,
            sortBy,
            voteAverageGte,
            releaseDecade,
            mediaType,
            timeWindow,
            DateOnly.FromDateTime(_time.GetUtcNow().UtcDateTime));
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            path,
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeSearchPage(
            response,
            await FactsAsync(Titles((response as JsonObject)?["results"]), cancellationToken)
                .ConfigureAwait(false));
    }

    internal static string BuildDiscoverPath(
        string kind,
        int page,
        int? genre,
        string? sortBy,
        int? voteAverageGte,
        int? releaseDecade,
        string? mediaType,
        string? timeWindow,
        DateOnly today)
    {
        var currentDecade = (today.Year / 10) * 10;
        var endpoint = kind.ToLowerInvariant() switch
        {
            "trending" => "trending",
            "movies" => "movies",
            "tv" => "tv",
            "upcoming-movies" => "movies/upcoming",
            "upcoming-tv" => "tv/upcoming",
            _ => throw new GatewayException(StatusCodes.Status404NotFound, "unknown discover kind")
        };
        if (genre is <= 0)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "genre must be positive");
        }
        if (voteAverageGte is < 0 or > 10)
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "minimum rating must be between 0 and 10");
        }
        if (releaseDecade is { } requestedDecade
            && (requestedDecade < 1900
                || requestedDecade > currentDecade
                || requestedDecade % 10 != 0))
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                string.Create(
                    CultureInfo.InvariantCulture,
                    $"release decade must be a ten-year start from 1900 through {currentDecade}"));
        }
        var safeSortBy = sortBy?.ToLowerInvariant() switch
        {
            null or "" => null,
            "popularity.desc" => "popularity.desc",
            "vote_average.desc" => "vote_average.desc",
            "primary_release_date.desc" when endpoint == "movies" => "primary_release_date.desc",
            "first_air_date.desc" when endpoint == "tv" => "first_air_date.desc",
            _ => throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "unknown discovery sort")
        };
        var safeMediaType = mediaType?.ToLowerInvariant() switch
        {
            null or "" => null,
            "all" => "all",
            "movie" => "movie",
            "tv" => "tv",
            _ => throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "unknown trending media type")
        };
        var safeTimeWindow = timeWindow?.ToLowerInvariant() switch
        {
            null or "" => null,
            "day" => "day",
            "week" => "week",
            _ => throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "unknown trending time window")
        };
        var query = new List<string>
        {
            string.Create(CultureInfo.InvariantCulture, $"page={Math.Max(1, page)}")
        };
        if (endpoint is "movies" or "tv")
        {
            if (genre is { } genreId)
            {
                query.Add(string.Create(CultureInfo.InvariantCulture, $"genre={genreId}"));
            }
            if (releaseDecade is { } decade)
            {
                var dateName = endpoint == "movies" ? "primaryReleaseDate" : "firstAirDate";
                var lastDate = decade == currentDecade
                    ? today
                    : new DateOnly(decade + 9, 12, 31);
                query.Add(string.Create(
                    CultureInfo.InvariantCulture,
                    $"{dateName}Gte={decade:D4}-01-01"));
                query.Add(string.Create(
                    CultureInfo.InvariantCulture,
                    $"{dateName}Lte={lastDate:yyyy-MM-dd}"));
            }
            if (safeSortBy is not null)
            {
                query.Add($"sortBy={safeSortBy}");
                if (safeSortBy == "vote_average.desc")
                {
                    query.Add("voteCountGte=50");
                }
            }
            if (voteAverageGte is { } score)
            {
                query.Add(string.Create(CultureInfo.InvariantCulture, $"voteAverageGte={score}"));
            }
        }
        else if (endpoint == "trending")
        {
            if (safeMediaType is not null)
            {
                query.Add($"mediaType={safeMediaType}");
            }
            if (safeTimeWindow is not null)
            {
                query.Add($"timeWindow={safeTimeWindow}");
            }
        }

        return $"api/v1/discover/{endpoint}?{string.Join('&', query)}";
    }

    public async Task<IReadOnlyList<SeerrGenreResponse>> GenresAsync(
        Guid jellyfinUserId,
        string mediaType,
        CancellationToken cancellationToken)
    {
        var type = ValidateMediaType(mediaType);
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/discover/genreslider/{type}",
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeGenres(response);
    }

    public async Task<SeerrMediaDetailResponse> MediaAsync(
        Guid jellyfinUserId,
        string mediaType,
        int tmdbId,
        CancellationToken cancellationToken)
    {
        var type = ValidateMediaType(mediaType);
        ValidatePositive(tmdbId, "TMDB id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/{type}/{tmdbId}",
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        return ShapeMedia(
            response,
            type,
            await FactsAsync([(type, response["mediaInfo"] as JsonObject)], cancellationToken)
                .ConfigureAwait(false));
    }

    public async Task<SeerrRequestOptionsResponse> RequestOptionsAsync(
        Guid jellyfinUserId,
        string mediaType,
        bool is4k,
        CancellationToken cancellationToken)
    {
        var type = ValidateMediaType(mediaType);
        var service = type == "movie" ? "radarr" : "sonarr";
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        await RequireAdvancedRequestAsync(user, cancellationToken).ConfigureAwait(false);
        var servers = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/service/{service}",
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonArray ?? new JsonArray();

        var destinations = new List<SeerrRequestDestinationResponse>();
        foreach (var server in servers
            .OfType<JsonObject>()
            .Where(server => IsTrue(server, "is4k") == is4k)
            .OrderByDescending(server => IsTrue(server, "isDefault"))
            .ThenBy(server => JsonRead.String(server, "name"), StringComparer.OrdinalIgnoreCase))
        {
            if (JsonRead.Int32(server, "id") is not { } serverId || serverId < 0)
            {
                continue;
            }

            var detail = await SendMappedAsync(
                HttpMethod.Get,
                $"api/v1/service/{service}/{serverId}",
                null,
                user,
                cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
            destinations.Add(ShapeRequestDestination(server, detail));
        }

        return new SeerrRequestOptionsResponse(destinations);
    }

    public async Task<SeerrRequestResponse> RequestAsync(
        Guid jellyfinUserId,
        SeerrRequestBody body,
        CancellationToken cancellationToken)
    {
        var type = ValidateMediaType(body.MediaType);
        ValidatePositive(body.TmdbId, "TMDB id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        if (body.ServerId.HasValue != body.ProfileId.HasValue
            || body.ServerId is < 0
            || body.ProfileId is <= 0)
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "the download destination and quality profile must be selected together");
        }
        if (body.ServerId is not null)
        {
            await RequireAdvancedRequestAsync(user, cancellationToken).ConfigureAwait(false);
        }
        var seasons = body.Seasons is { Count: > 0 }
            ? new JsonArray(body.Seasons.Where(static season => season > 0)
                .Distinct()
                .Order()
                .Select(static season => (JsonNode?)JsonValue.Create(season))
                .ToArray())
            : null;
        var request = new JsonObject
        {
            ["mediaType"] = type,
            ["mediaId"] = body.TmdbId,
            ["is4k"] = body.Is4k
        };
        if (body.ServerId is { } serverId && body.ProfileId is { } profileId)
        {
            request["serverId"] = serverId;
            request["profileId"] = profileId;
        }
        if (type == "tv")
        {
            request["seasons"] = seasons ?? (JsonNode?)JsonValue.Create("all");
        }
        var response = await SendMappedAsync(
            HttpMethod.Post,
            "api/v1/request",
            request,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        return ShapeRequest(
            response,
            await FactsAsync(RequestTitles([response]), cancellationToken).ConfigureAwait(false));
    }

    public async Task<SeerrPageResponse<SeerrRequestResponse>> RequestsAsync(
        Guid jellyfinUserId,
        int take,
        int skip,
        string filter,
        CancellationToken cancellationToken)
    {
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var safeFilter = filter.ToLowerInvariant() switch
        {
            "all" or "pending" or "approved" or "processing" or "available" or "failed" =>
                filter.ToLowerInvariant(),
            _ => "all"
        };
        var path = string.Create(
            CultureInfo.InvariantCulture,
            $"api/v1/request?take={Math.Clamp(take, 1, 100)}&skip={Math.Max(0, skip)}&filter={safeFilter}&sort=added&requestedBy={user}");
        var response = await SendMappedAsync(
            HttpMethod.Get,
            path,
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        var pageInfo = response?["pageInfo"] as JsonObject;
        var requests = Objects(response?["results"]).ToArray();
        var facts = await FactsAsync(RequestTitles(requests), cancellationToken).ConfigureAwait(false);
        var results = requests.Select(request => ShapeRequest(request, facts)).ToArray();
        return new SeerrPageResponse<SeerrRequestResponse>(
            JsonRead.Int32(pageInfo, "page") ?? 1,
            JsonRead.Int32(pageInfo, "pages") ?? 1,
            JsonRead.Int32(pageInfo, "results") ?? results.Length,
            results);
    }

    public async Task<SeerrCancelResponse> CancelAsync(
        Guid jellyfinUserId,
        int requestId,
        CancellationToken cancellationToken)
    {
        ValidatePositive(requestId, "request id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        await SendMappedAsync(
            HttpMethod.Delete,
            $"api/v1/request/{requestId}",
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return new SeerrCancelResponse(true, requestId);
    }

    private async Task<int> ResolveUserAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        if (CachedMapping(jellyfinUserId) is { } cached)
        {
            return cached;
        }

        // One lookup at a time: concurrent first requests from the same user
        // share a single user scan instead of each paging through Seerr.
        await _lookupGate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (CachedMapping(jellyfinUserId) is { } resolved)
            {
                return resolved;
            }

            var now = _time.GetUtcNow();
            if (_missing.TryGetValue(jellyfinUserId, out var missingSince)
                && now - missingSince < MissingUserLifetime)
            {
                throw NotImported();
            }

            var mapped = await FindUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
            if (mapped is null && _seerr.AutoImportUsers)
            {
                await _seerr.SendAsync(
                    HttpMethod.Post,
                    "api/v1/user/import-from-jellyfin",
                    new JsonObject
                    {
                        // Seerr matches these against the ids Jellyfin's API
                        // serializes, which are dashless ("N" format).
                        ["jellyfinUserIds"] = new JsonArray(JsonValue.Create(jellyfinUserId.ToString("N")))
                    },
                    null,
                    cancellationToken).ConfigureAwait(false);
                mapped = await FindUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
            }

            now = _time.GetUtcNow();
            if (mapped is null)
            {
                ForgetExpiredMisses(now);
                _missing[jellyfinUserId] = now;
                throw NotImported();
            }

            _missing.TryRemove(jellyfinUserId, out _);
            _mappings[jellyfinUserId] = new MappingRecord(mapped.Value, now);
            return mapped.Value;
        }
        finally
        {
            _lookupGate.Release();
        }
    }

    private int? CachedMapping(Guid jellyfinUserId)
        => _mappings.TryGetValue(jellyfinUserId, out var cached)
            && _time.GetUtcNow() - cached.CachedAt < MappingLifetime
                ? cached.SeerrUserId
                : null;

    private void ForgetExpiredMisses(DateTimeOffset now)
    {
        foreach (var (userId, since) in _missing)
        {
            if (now - since >= MissingUserLifetime)
            {
                _missing.TryRemove(userId, out _);
            }
        }
    }

    private static GatewayException NotImported()
        => new(
            StatusCodes.Status409Conflict,
            "your Jellyfin account has not been imported into Seerr; ask your administrator to import it");

    private async Task<int?> FindUserAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        for (var skip = 0; skip < MaxUserScan; skip += UserPageSize)
        {
            var response = await _seerr.SendAsync(
                HttpMethod.Get,
                string.Create(
                    CultureInfo.InvariantCulture,
                    $"api/v1/user?take={UserPageSize}&skip={skip}&sort=created"),
                null,
                null,
                cancellationToken).ConfigureAwait(false) as JsonObject;
            var users = response?["results"] as JsonArray;
            foreach (var user in Objects(users))
            {
                if (Guid.TryParse(JsonRead.String(user, "jellyfinUserId"), out var candidate)
                    && candidate == jellyfinUserId
                    && JsonRead.Int32(user, "id") is { } id)
                {
                    return id;
                }
            }

            if (users is null || users.Count < UserPageSize)
            {
                break;
            }
        }

        return null;
    }

    private Task<JsonNode?> SendMappedAsync(
        HttpMethod method,
        string path,
        JsonNode? body,
        int seerrUserId,
        CancellationToken cancellationToken)
        => _seerr.SendAsync(method, path, body, seerrUserId, cancellationToken);

    /// <summary>
    /// Radarr and Sonarr facts for the processing titles among
    /// <paramref name="titles"/>, which pair a media type with Seerr's media row.
    /// </summary>
    private async Task<ArrFacts> FactsAsync(
        IEnumerable<(string? MediaType, JsonObject? MediaInfo)> titles,
        CancellationToken cancellationToken)
    {
        if (_arr is null)
        {
            return ArrFacts.None;
        }

        var wanted = titles
            .Where(static title => MediaActivity.NeedsFacts(title.MediaInfo))
            .ToArray();
        return await _arr.LookupAsync(
            wanted
                .Where(static title => title.MediaType == "movie")
                .Select(static title => JsonRead.Int32(title.MediaInfo, "tmdbId") ?? 0),
            wanted
                .Where(static title => title.MediaType == "tv")
                .Select(static title => JsonRead.Int32(title.MediaInfo, "tvdbId") ?? 0),
            cancellationToken).ConfigureAwait(false);
    }

    private static IEnumerable<(string? MediaType, JsonObject? MediaInfo)> Titles(JsonNode? results)
        => Objects(results).Select(static result => (
            JsonRead.String(result, "mediaType"),
            result["mediaInfo"] as JsonObject));

    private static IEnumerable<(string? MediaType, JsonObject? MediaInfo)> RequestTitles(
        IEnumerable<JsonObject> requests)
        => requests.Select(static request =>
        {
            var media = request["media"] as JsonObject;
            return (JsonRead.String(request, "type") ?? JsonRead.String(media, "mediaType"), media);
        });

    private async Task RequireAdvancedRequestAsync(
        int seerrUserId,
        CancellationToken cancellationToken)
    {
        var user = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/auth/me",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        var permissions = JsonRead.UInt64(user, "permissions") ?? 0;
        if (!HasPermission(permissions, RequestAdvanced))
        {
            throw new GatewayException(
                StatusCodes.Status403Forbidden,
                "your Seerr account is not allowed to choose download quality profiles");
        }
    }

    internal static SeerrPageResponse<SeerrResultResponse> ShapeSearchPage(
        JsonNode? node,
        ArrFacts? facts = null)
    {
        var page = node as JsonObject;
        var results = Objects(page?["results"])
            .Where(static result => JsonRead.String(result, "mediaType") is "movie" or "tv")
            .Select(result => ShapeSearchResult(result, facts ?? ArrFacts.None))
            .ToArray();
        return new SeerrPageResponse<SeerrResultResponse>(
            JsonRead.Int32(page, "page") ?? 1,
            JsonRead.Int32(page, "totalPages") ?? 1,
            JsonRead.Int32(page, "totalResults") ?? results.Length,
            results);
    }

    internal static SeerrPageResponse<SeerrResultResponse> ShapePersonCredits(
        JsonNode? node,
        ArrFacts? facts = null)
    {
        var results = Objects((node as JsonObject)?["cast"])
            .Where(static credit =>
                JsonRead.String(credit, "mediaType") is "movie" or "tv"
                && (JsonRead.Int32(credit, "id") ?? 0) > 0
                && JsonRead.Flag(credit["adult"]) != true
                && !string.Equals(
                    JsonRead.String(credit, "character")?.Trim(),
                    "Thanks",
                    StringComparison.OrdinalIgnoreCase))
            .GroupBy(
                static credit => $"{JsonRead.String(credit, "mediaType")}:{JsonRead.Int32(credit, "id")}",
                StringComparer.Ordinal)
            // Preserve request state when only a duplicate character row
            // happens to carry Seerr's mediaInfo object.
            .Select(group => ShapeSearchResult(
                group.FirstOrDefault(static credit => credit["mediaInfo"] is JsonObject)
                ?? group.First(),
                facts ?? ArrFacts.None))
            .ToArray();
        return new SeerrPageResponse<SeerrResultResponse>(
            1,
            results.Length > 0 ? 1 : 0,
            results.Length,
            results);
    }

    internal static IReadOnlyList<SeerrGenreResponse> ShapeGenres(JsonNode? node)
        => Objects(node)
            .Select(static genre => (
                Id: JsonRead.Int32(genre, "id") ?? 0,
                Name: JsonRead.String(genre, "name"),
                Genre: genre))
            .Where(static genre => genre.Id > 0 && !string.IsNullOrWhiteSpace(genre.Name))
            .Select(static genre => new SeerrGenreResponse(
                genre.Id,
                genre.Name!,
                Strings(genre.Genre["backdrops"]).ToArray()))
            .ToArray();

    private static SeerrResultResponse ShapeSearchResult(JsonObject result, ArrFacts facts)
    {
        var mediaInfo = result["mediaInfo"] as JsonObject;
        var mediaType = JsonRead.String(result, "mediaType") ?? string.Empty;
        var status = JsonRead.Int32(mediaInfo, "status");
        var status4k = JsonRead.Int32(mediaInfo, "status4k");
        return new SeerrResultResponse(
            mediaType,
            JsonRead.Int32(result, "id"),
            JsonRead.String(result, mediaType == "movie" ? "title" : "name") ?? "Untitled",
            JsonRead.Year(JsonRead.String(
                result,
                mediaType == "movie" ? "releaseDate" : "firstAirDate")),
            JsonRead.String(result, "overview"),
            JsonRead.String(result, "posterPath"),
            JsonRead.String(result, "backdropPath"),
            JsonRead.Number(result["voteAverage"]),
            StatusName(status ?? 1),
            StatusName(status4k ?? 1),
            Activity: MediaActivity.Resolve(mediaType, mediaInfo, status, false, null, facts),
            Activity4k: MediaActivity.Resolve(mediaType, mediaInfo, status4k, true, null, facts));
    }

    internal static SeerrRequestDestinationResponse ShapeRequestDestination(
        JsonObject server,
        JsonObject detail)
    {
        var activeProfile = JsonRead.Int32(server, "activeProfileId") ?? 0;
        var profiles = Objects(detail["profiles"])
            .Select(static profile => (
                Id: JsonRead.Int32(profile, "id") ?? 0,
                Name: JsonRead.String(profile, "name")))
            .Where(static profile => profile.Id > 0 && !string.IsNullOrWhiteSpace(profile.Name))
            .OrderBy(static profile => profile.Name, StringComparer.OrdinalIgnoreCase)
            .Select(profile => new SeerrQualityProfileResponse(
                profile.Id,
                profile.Name!,
                profile.Id == activeProfile))
            .ToArray();
        return new SeerrRequestDestinationResponse(
            JsonRead.Int32(server, "id") ?? 0,
            JsonRead.String(server, "name") ?? "Download service",
            IsTrue(server, "isDefault"),
            profiles);
    }

    internal static SeerrMediaDetailResponse ShapeMedia(
        JsonObject detail,
        string mediaType,
        ArrFacts? facts = null)
    {
        var movie = mediaType == "movie";
        var arr = facts ?? ArrFacts.None;
        var mediaInfo = detail["mediaInfo"] as JsonObject;
        var status = JsonRead.Int32(mediaInfo, "status");
        var status4k = JsonRead.Int32(mediaInfo, "status4k");
        var knownSeasons = Objects(mediaInfo?["seasons"]).ToArray();
        var seasons = Objects(detail["seasons"])
            .Where(static season => (JsonRead.Int32(season, "seasonNumber") ?? 0) > 0)
            .Select(season =>
            {
                var number = JsonRead.Int32(season, "seasonNumber") ?? 0;
                var known = knownSeasons.FirstOrDefault(
                    item => JsonRead.Int32(item, "seasonNumber") == number);
                var seasonStatus = JsonRead.Int32(known, "status");
                var seasonStatus4k = JsonRead.Int32(known, "status4k");
                return new SeerrSeasonResponse(
                    number,
                    JsonRead.String(season, "name"),
                    JsonRead.Int32(season, "episodeCount") ?? 0,
                    JsonRead.String(season, "airDate"),
                    StatusName(seasonStatus ?? 1),
                    StatusName(seasonStatus4k ?? 1),
                    MediaActivity.Resolve(mediaType, mediaInfo, seasonStatus, false, number, arr),
                    MediaActivity.Resolve(mediaType, mediaInfo, seasonStatus4k, true, number, arr));
            })
            .ToArray();
        var runtime = JsonRead.Int32(detail, "runtime")
            ?? (detail["episodeRunTime"] as JsonArray)?
                .Select(JsonRead.Int32)
                .FirstOrDefault(static value => value is > 0);
        var externalIds = detail["externalIds"] as JsonObject;
        var tvdb = JsonRead.Int32(externalIds, "tvdbId");

        return new SeerrMediaDetailResponse(
            mediaType,
            JsonRead.Int32(detail, "id"),
            JsonRead.String(detail, movie ? "title" : "name") ?? "Untitled",
            JsonRead.String(detail, movie ? "originalTitle" : "originalName"),
            JsonRead.Year(JsonRead.String(detail, movie ? "releaseDate" : "firstAirDate")),
            JsonRead.String(detail, "overview"),
            JsonRead.String(detail, "tagline"),
            JsonRead.String(detail, "posterPath"),
            JsonRead.String(detail, "backdropPath"),
            JsonRead.Number(detail["voteAverage"]),
            JsonRead.Integer(detail["voteCount"]),
            StatusName(status ?? 1),
            StatusName(status4k ?? 1),
            null,
            runtime,
            Objects(detail["genres"])
                .Select(static genre => JsonRead.String(genre, "name") ?? string.Empty)
                .ToArray(),
            seasons,
            JsonRead.String(detail, movie ? "releaseDate" : "firstAirDate"),
            JsonRead.String(detail, "firstAirDate"),
            JsonRead.String(detail, "lastAirDate"),
            JsonRead.String(detail, "status"),
            JsonRead.Bool(detail, "inProduction"),
            JsonRead.String(detail, "type"),
            JsonRead.Int32(detail, "numberOfSeasons"),
            JsonRead.Int32(detail, "numberOfEpisodes"),
            JsonRead.String(detail, "originalLanguage"),
            JsonRead.String(detail, "homepage"),
            new SeerrExternalIdsResponse(
                ImdbIds.Normalize(JsonRead.String(externalIds, "imdbId"))
                    ?? ImdbIds.Normalize(JsonRead.String(detail, "imdbId")),
                tvdb is > 0 ? tvdb : null),
            JsonRead.Positive(detail["budget"]),
            JsonRead.Positive(detail["revenue"]),
            Names(detail["productionCompanies"]).ToArray(),
            Names(detail["networks"]).ToArray(),
            UniqueStrings(Names(detail["createdBy"]).Concat(CrewNames(detail, "Creator", null))),
            UniqueStrings(CrewNames(detail, "Director", null)),
            UniqueStrings(CrewNames(detail, null, "Writing")),
            ShapeCodeNames(detail["productionCountries"], "iso_3166_1", "name"),
            ShapeCodeNames(detail["spokenLanguages"], "iso_639_1", "englishName"),
            ShapeCast(detail),
            ShapeTrailer(detail),
            ShapeReleaseDates(detail),
            ShapeContentRatings(detail),
            ShapeNextEpisode(detail),
            MediaActivity.Resolve(mediaType, mediaInfo, status, false, null, arr),
            MediaActivity.Resolve(mediaType, mediaInfo, status4k, true, null, arr));
    }

    internal static SeerrQuotaResponse? ShapeQuota(JsonNode? node)
    {
        var quota = node as JsonObject;
        return quota?["movie"] is JsonObject movie && quota["tv"] is JsonObject tv
            ? new SeerrQuotaResponse(ShapeQuotaLimit(movie), ShapeQuotaLimit(tv))
            : null;
    }

    private static SeerrQuotaLimitResponse ShapeQuotaLimit(JsonObject limit)
        => new(
            JsonRead.Int32(limit, "days"),
            JsonRead.Int32(limit, "limit"),
            JsonRead.Int32(limit, "used") ?? 0,
            JsonRead.Int32(limit, "remaining"),
            IsTrue(limit, "restricted"));

    private static IEnumerable<JsonObject> Objects(JsonNode? node)
        => (node as JsonArray)?.OfType<JsonObject>() ?? [];

    private static IEnumerable<string> Strings(JsonNode? node)
        => (node as JsonArray)?.Select(JsonRead.String).OfType<string>() ?? [];

    private static bool IsTrue(JsonObject? value, string name) => JsonRead.Bool(value, name) == true;

    private static IEnumerable<string> Names(JsonNode? node)
        => Objects(node)
            .Select(static value => JsonRead.String(value, "name")?.Trim())
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!);

    private static IEnumerable<string> CrewNames(
        JsonObject detail,
        string? job,
        string? department)
        => Objects((detail["credits"] as JsonObject)?["crew"])
            .Where(person =>
                (job is null || JsonRead.String(person, "job") == job)
                && (department is null || JsonRead.String(person, "department") == department))
            .Select(static person => JsonRead.String(person, "name")?.Trim())
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!);

    private static string[] UniqueStrings(IEnumerable<string> values)
        => values
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

    private static SeerrCodeNameResponse[] ShapeCodeNames(
        JsonNode? node,
        string codeName,
        string preferredName)
        => Objects(node)
            .Where(value =>
                !string.IsNullOrWhiteSpace(JsonRead.String(value, codeName))
                || !string.IsNullOrWhiteSpace(JsonRead.String(value, "name")))
            .Select(value => new SeerrCodeNameResponse(
                JsonRead.String(value, codeName),
                JsonRead.String(value, preferredName) ?? JsonRead.String(value, "name")))
            .ToArray();

    private static SeerrCastMemberResponse[] ShapeCast(JsonObject detail)
        => Objects((detail["credits"] as JsonObject)?["cast"])
            .Where(static person => !string.IsNullOrWhiteSpace(JsonRead.String(person, "name")))
            .Take(20)
            .Select(static person => new SeerrCastMemberResponse(
                JsonRead.Int32(person, "id"),
                JsonRead.String(person, "name"),
                JsonRead.String(person, "character"),
                JsonRead.String(person, "profilePath")))
            .ToArray();

    private static SeerrTrailerResponse? ShapeTrailer(JsonObject detail)
    {
        var trailer = Objects(detail["relatedVideos"])
            .Where(static video =>
                JsonRead.String(video, "site") == "YouTube"
                && JsonRead.String(video, "type") == "Trailer"
                && IsYoutubeKey(JsonRead.String(video, "key")))
            .OrderByDescending(static video => JsonRead.Int32(video, "size") ?? 0)
            .FirstOrDefault();
        return trailer is null
            ? null
            : new SeerrTrailerResponse(
                JsonRead.String(trailer, "name") ?? "Trailer",
                JsonRead.String(trailer, "key"));
    }

    private static bool IsYoutubeKey(string? value)
        => value is { Length: 11 }
            && value.All(static character => char.IsAsciiLetterOrDigit(character)
                || character is '-' or '_');

    private static SeerrReleaseDateResponse[] ShapeReleaseDates(JsonObject detail)
    {
        var releases = new List<SeerrReleaseDateResponse>();
        foreach (var country in Objects((detail["releases"] as JsonObject)?["results"]))
        {
            var region = JsonRead.String(country, "iso_3166_1");
            if (string.IsNullOrWhiteSpace(region))
            {
                continue;
            }
            foreach (var release in Objects(country["release_dates"]))
            {
                var type = (JsonRead.Int32(release, "type") ?? 0) switch
                {
                    1 => "premiere",
                    2 => "limited-cinema",
                    3 => "cinema",
                    4 => "digital",
                    5 => "physical",
                    6 => "tv",
                    _ => null
                };
                var date = JsonRead.String(release, "release_date");
                if (type is null || string.IsNullOrWhiteSpace(date))
                {
                    continue;
                }
                releases.Add(new SeerrReleaseDateResponse(
                    region,
                    type,
                    date,
                    JsonRead.String(release, "certification")));
            }
        }
        return releases.ToArray();
    }

    private static SeerrContentRatingResponse[] ShapeContentRatings(JsonObject detail)
        => Objects((detail["contentRatings"] as JsonObject)?["results"])
            .Where(static rating =>
                !string.IsNullOrWhiteSpace(JsonRead.String(rating, "iso_3166_1"))
                && !string.IsNullOrWhiteSpace(JsonRead.String(rating, "rating")))
            .Select(static rating => new SeerrContentRatingResponse(
                JsonRead.String(rating, "iso_3166_1"),
                JsonRead.String(rating, "rating")))
            .ToArray();

    private static SeerrNextEpisodeResponse? ShapeNextEpisode(JsonObject detail)
        => detail["nextEpisodeToAir"] is JsonObject episode
            ? new SeerrNextEpisodeResponse(
                JsonRead.String(episode, "name"),
                JsonRead.String(episode, "airDate"),
                JsonRead.Int32(episode, "seasonNumber"),
                JsonRead.Int32(episode, "episodeNumber"))
            : null;

    internal static SeerrRequestResponse ShapeRequest(JsonObject request, ArrFacts? facts = null)
    {
        var media = request["media"] as JsonObject;
        var is4k = IsTrue(request, "is4k");
        var mediaType = JsonRead.String(request, "type")
            ?? JsonRead.String(media, "mediaType")
            ?? string.Empty;
        var mediaStatus = JsonRead.Int32(media, is4k ? "status4k" : "status");
        return new SeerrRequestResponse(
            JsonRead.Int32(request, "id"),
            RequestStatusName(JsonRead.Int32(request, "status") ?? 0),
            mediaType,
            JsonRead.Int32(media, "tmdbId"),
            is4k,
            JsonRead.String(request, "createdAt"),
            JsonRead.String(request, "updatedAt"),
            StatusName(mediaStatus ?? 1),
            Objects(request["seasons"])
                .Select(static season => JsonRead.Int32(season, "seasonNumber") ?? 0)
                .ToArray(),
            MediaActivity: MediaActivity.Resolve(
                mediaType,
                media,
                mediaStatus,
                is4k,
                null,
                facts ?? ArrFacts.None));
    }

    private static SeerrCapabilitiesResponse Capabilities(ulong permissions, bool movie4k, bool tv4k)
    {
        bool Has(ulong mask) => HasPermission(permissions, mask);
        return new SeerrCapabilitiesResponse(
            new(Has(Request | RequestMovie), Has(AutoApprove | AutoApproveMovie)),
            new(Has(Request | RequestTv), Has(AutoApprove | AutoApproveTv)),
            new(
                movie4k && Has(Request4k | Request4kMovie),
                movie4k && Has(AutoApprove4k | AutoApprove4kMovie)),
            new(
                tv4k && Has(Request4k | Request4kTv),
                tv4k && Has(AutoApprove4k | AutoApprove4kTv)),
            Has(RequestAdvanced));
    }

    private static bool HasPermission(ulong permissions, ulong permission)
        => (permissions & Admin) != 0 || (permissions & permission) != 0;

    private static string PreferredUserName(JsonObject? user)
        => new[]
        {
            JsonRead.String(user, "displayName"),
            JsonRead.String(user, "username"),
            JsonRead.String(user, "jellyfinUsername"),
            JsonRead.String(user, "email")
        }.FirstOrDefault(static value => !string.IsNullOrWhiteSpace(value)) ?? "Seerr user";

    private static string ValidateMediaType(string mediaType)
        => mediaType.ToLowerInvariant() switch
        {
            "movie" => "movie",
            "tv" => "tv",
            _ => throw new GatewayException(StatusCodes.Status400BadRequest, "unsupported media type")
        };

    private static void ValidatePositive(int value, string name)
    {
        if (value <= 0)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, $"{name} must be positive");
        }
    }

    private static string StatusName(int status)
        => status switch
        {
            2 => "pending",
            3 => "processing",
            4 => "partial",
            5 => "available",
            6 => "blacklisted",
            _ => "unknown"
        };

    private static string RequestStatusName(int status)
        => status switch
        {
            1 => "pending",
            2 => "approved",
            3 => "declined",
            4 => "failed",
            _ => "unknown"
        };

    private sealed record MappingRecord(int SeerrUserId, DateTimeOffset CachedAt);
}
