using System.Collections.Concurrent;
using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

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
    private static readonly TimeSpan MappingLifetime = TimeSpan.FromMinutes(10);
    private readonly CompanionHttpClient _http;
    private readonly ConcurrentDictionary<Guid, MappingRecord> _mappings = new();

    public SeerrGateway(CompanionHttpClient http)
    {
        _http = http;
    }

    public async Task<JsonNode> StatusAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        var configuration = Configuration();
        var seerrUserId = await ResolveUserAsync(jellyfinUserId, cancellationToken)
            .ConfigureAwait(false);
        var user = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/auth/me",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
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
        catch (GatewayException)
        {
            // Permissions remain useful if this optional usage counter is
            // temporarily unavailable.
            quota = null;
        }
        var settings = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/settings/public",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        var permissions = ULongValue(user, "permissions") ?? 0;
        var movie4k = BoolValue(settings, "movie4kEnabled");
        var tv4k = BoolValue(settings, "series4kEnabled");

        return new JsonObject
        {
            ["configured"] = configuration.Enabled,
            ["linked"] = true,
            ["expired"] = false,
            ["serverUrl"] = null,
            ["mapped"] = true,
            ["instance"] = new JsonObject
            {
                ["movie4kEnabled"] = movie4k,
                ["series4kEnabled"] = tv4k,
                ["partialRequestsEnabled"] = BoolValue(settings, "partialRequestsEnabled")
            },
            ["user"] = new JsonObject
            {
                ["id"] = seerrUserId,
                ["name"] = PreferredUserName(user),
                ["avatar"] = Clone(user["avatar"]),
                ["jellyfinUserId"] = jellyfinUserId.ToString("N")
            },
            ["capabilities"] = Capabilities(permissions, movie4k, tv4k),
            ["quota"] = Clone(quota)
        };
    }

    public async Task<JsonNode> SearchAsync(
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
        return ShapeSearchPage(response);
    }

    public async Task<JsonNode> PersonCreditsAsync(
        Guid jellyfinUserId,
        int tmdbId,
        CancellationToken cancellationToken)
    {
        ValidatePositive(tmdbId, "TMDB person id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/person/{tmdbId}/combined_credits",
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        var credits = response as JsonObject ?? new JsonObject();
        var responseId = IntValue(credits, "id");
        if (responseId is > 0 && responseId != tmdbId)
        {
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                "Seerr returned credits for a different TMDB person");
        }
        return ShapePersonCredits(credits);
    }

    public async Task<JsonNode> DiscoverAsync(
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
            timeWindow);
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            path,
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeSearchPage(response);
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
        DateOnly? currentDate = null)
    {
        var today = currentDate ?? DateOnly.FromDateTime(DateTime.UtcNow);
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

    public async Task<JsonNode> GenresAsync(
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

    public async Task<JsonNode> MediaAsync(
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
        return ShapeMedia(response, type);
    }

    public async Task<JsonNode> RequestOptionsAsync(
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

        var destinations = new List<JsonNode>();
        foreach (var server in servers
            .OfType<JsonObject>()
            .Where(server => BoolValue(server, "is4k") == is4k)
            .OrderByDescending(server => BoolValue(server, "isDefault"))
            .ThenBy(server => StringValue(server, "name"), StringComparer.OrdinalIgnoreCase))
        {
            var serverId = IntValue(server, "id");
            if (serverId is null or < 0)
            {
                continue;
            }

            var detail = await SendMappedAsync(
                HttpMethod.Get,
                $"api/v1/service/{service}/{serverId.Value}",
                null,
                user,
                cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
            destinations.Add(ShapeRequestDestination(server, detail));
        }

        return new JsonObject
        {
            ["destinations"] = new JsonArray(destinations.ToArray())
        };
    }

    public async Task<JsonNode> RequestAsync(
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
            cancellationToken).ConfigureAwait(false);
        return ShapeRequest(response as JsonObject ?? new JsonObject());
    }

    public async Task<JsonNode> RequestsAsync(
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
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        var pageInfo = response["pageInfo"] as JsonObject;
        var results = new JsonArray(
            (response["results"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Select(ShapeRequest)
                .ToArray());
        return new JsonObject
        {
            ["page"] = IntValue(pageInfo, "page") ?? 1,
            ["totalPages"] = IntValue(pageInfo, "pages") ?? 1,
            ["totalResults"] = IntValue(pageInfo, "results") ?? results.Count,
            ["results"] = results
        };
    }

    public async Task<JsonNode> CancelAsync(
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
        return new JsonObject { ["cancelled"] = true, ["id"] = requestId };
    }

    /// <summary>
    /// The TMDB collection one movie belongs to, or `null`. Overseerr maps
    /// TMDB's snake_case `belongs_to_collection` into this camelCase object.
    /// </summary>
    public async Task<JsonNode> MovieCollectionAsync(
        Guid jellyfinUserId,
        int tmdbId,
        CancellationToken cancellationToken)
    {
        ValidatePositive(tmdbId, "TMDB movie id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/movie/{tmdbId}",
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        return new JsonObject
        {
            ["tmdbId"] = tmdbId,
            ["collection"] = Clone(response["collection"])
        };
    }

    /// <summary>
    /// One TMDB collection with its movie parts. Parts reuse the shared search
    /// result shape so Desktop's local-library join applies unchanged.
    /// </summary>
    public async Task<JsonNode> CollectionAsync(
        Guid jellyfinUserId,
        int collectionId,
        CancellationToken cancellationToken)
    {
        ValidatePositive(collectionId, "collection id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            $"api/v1/collection/{collectionId}",
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeCollection(response);
    }

    /// <summary>
    /// One curated collection's parts, composed from bounded concurrent
    /// Seerr lookups in definition order. Movies and series keep separate TMDB
    /// identities, and a title Seerr no longer knows does not hide the rest.
    /// </summary>
    internal const int MaxCuratedParts = 500;

    internal async Task<JsonNode> CuratedCollectionAsync(
        Guid jellyfinUserId,
        string definitionId,
        string name,
        IReadOnlyList<CuratedItem> items,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(definitionId))
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "that is not a curated collection id");
        }

        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var wanted = items
            .Where(static item => item.TmdbId > 0)
            .Distinct()
            .Take(MaxCuratedParts)
            .ToArray();
        var parts = new JsonNode?[wanted.Length];
        var failures = new ConcurrentQueue<GatewayException>();
        using var throttler = new SemaphoreSlim(8);
        var lookups = wanted.Select(async (item, index) =>
        {
            await throttler.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                var response = await SendMappedAsync(
                    HttpMethod.Get,
                    $"api/v1/{item.SeerrMediaType}/{item.TmdbId}",
                    null,
                    user,
                    cancellationToken).ConfigureAwait(false) as JsonObject;
                if (response is null || IntValue(response, "id") != item.TmdbId)
                {
                    return;
                }

                response["mediaType"] = item.SeerrMediaType;
                parts[index] = ShapeSearchResult(response);
            }
            catch (GatewayException exception) when (
                exception.StatusCode == StatusCodes.Status404NotFound)
            {
                // One removed TMDB title is not a failure of the definition.
            }
            catch (GatewayException exception) when (
                exception.StatusCode is StatusCodes.Status500InternalServerError
                    or StatusCodes.Status502BadGateway
                    or StatusCodes.Status504GatewayTimeout)
            {
                // A transient per-title failure should not discard successful
                // siblings. If every lookup fails, the endpoint reports the
                // upstream error instead of claiming an empty collection.
                failures.Enqueue(exception);
            }
            finally
            {
                throttler.Release();
            }
        });
        await Task.WhenAll(lookups).ConfigureAwait(false);

        var resolved = parts.OfType<JsonObject>().ToArray();
        if (resolved.Length == 0 && failures.TryPeek(out var failure))
        {
            throw failure;
        }

        return new JsonObject
        {
            ["id"] = definitionId,
            ["name"] = name,
            ["overview"] = null,
            ["posterPath"] = null,
            ["backdropPath"] = null,
            ["totalParts"] = wanted.Length,
            ["unresolvedParts"] = wanted.Length - resolved.Length,
            ["parts"] = new JsonArray(resolved)
        };
    }

    internal static JsonNode ShapeCollection(JsonNode? node)
    {
        var detail = node as JsonObject ?? new JsonObject();
        var parts = new JsonArray(
            (detail["parts"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static part => (IntValue(part, "id") ?? 0) > 0 && !BoolValue(part, "adult"))
                .Select(static part =>
                {
                    // TMDB parts are always movies; tolerate upstreams that omit
                    // the mediaType marker ShapeSearchResult keys off.
                    if (part["mediaType"] is null)
                    {
                        part["mediaType"] = "movie";
                    }

                    return ShapeSearchResult(part);
                })
                .ToArray());
        return new JsonObject
        {
            ["id"] = IntValue(detail, "id"),
            ["name"] = StringValue(detail, "name"),
            ["overview"] = Clone(detail["overview"]),
            ["posterPath"] = Clone(detail["posterPath"]),
            ["backdropPath"] = Clone(detail["backdropPath"]),
            ["parts"] = parts
        };
    }

    private async Task<int> ResolveUserAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        if (_mappings.TryGetValue(jellyfinUserId, out var cached)
            && DateTimeOffset.UtcNow - cached.CachedAt < MappingLifetime)
        {
            return cached.SeerrUserId;
        }

        var mapped = await FindUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        if (mapped is null && Plugin.Instance?.Configuration.AutoImportSeerrUsers == true)
        {
            await _http.SendAsync(
                "seerr",
                Configuration(),
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

        if (mapped is null)
        {
            throw new GatewayException(
                StatusCodes.Status409Conflict,
                "your Jellyfin account has not been imported into Seerr; ask your administrator to import it");
        }

        _mappings[jellyfinUserId] = new MappingRecord(mapped.Value, DateTimeOffset.UtcNow);
        return mapped.Value;
    }

    private async Task<int?> FindUserAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        const int take = 50;
        for (var skip = 0; skip < 10_000; skip += take)
        {
            var response = await _http.SendAsync(
                "seerr",
                Configuration(),
                HttpMethod.Get,
                $"api/v1/user?take={take}&skip={skip}&sort=created",
                null,
                null,
                cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
            var users = response["results"] as JsonArray ?? new JsonArray();
            foreach (var user in users.OfType<JsonObject>())
            {
                if (Guid.TryParse(StringValue(user, "jellyfinUserId"), out var candidate)
                    && candidate == jellyfinUserId
                    && IntValue(user, "id") is { } id)
                {
                    return id;
                }
            }

            if (users.Count < take)
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
        => _http.SendAsync(
            "seerr",
            Configuration(),
            method,
            path,
            body,
            seerrUserId,
            cancellationToken);

    private async Task RequireAdvancedRequestAsync(
        int seerrUserId,
        CancellationToken cancellationToken)
    {
        var user = await SendMappedAsync(
            HttpMethod.Get,
            "api/v1/auth/me",
            null,
            seerrUserId,
            cancellationToken).ConfigureAwait(false) as JsonObject ?? new JsonObject();
        var permissions = ULongValue(user, "permissions") ?? 0;
        if (!HasPermission(permissions, RequestAdvanced))
        {
            throw new GatewayException(
                StatusCodes.Status403Forbidden,
                "your Seerr account is not allowed to choose download quality profiles");
        }
    }

    private static Configuration.ServiceConfiguration Configuration()
        => Plugin.Instance?.Configuration.Seerr
            ?? throw new GatewayException(
                StatusCodes.Status503ServiceUnavailable,
                "the plugin is not initialized");

    internal static JsonNode ShapeSearchPage(JsonNode? node)
    {
        var page = node as JsonObject ?? new JsonObject();
        var results = new JsonArray(
            (page["results"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static result => StringValue(result, "mediaType") is "movie" or "tv")
                .Select(ShapeSearchResult)
                .ToArray());
        return new JsonObject
        {
            ["page"] = IntValue(page, "page") ?? 1,
            ["totalPages"] = IntValue(page, "totalPages") ?? 1,
            ["totalResults"] = IntValue(page, "totalResults") ?? results.Count,
            ["results"] = results
        };
    }

    internal static JsonNode ShapePersonCredits(JsonNode? node)
    {
        var credits = node as JsonObject ?? new JsonObject();
        var results = new JsonArray(
            (credits["cast"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static credit =>
                    StringValue(credit, "mediaType") is "movie" or "tv"
                    && (IntValue(credit, "id") ?? 0) > 0
                    && !BoolValue(credit, "adult")
                    && !string.Equals(
                        StringValue(credit, "character")?.Trim(),
                        "Thanks",
                        StringComparison.OrdinalIgnoreCase))
                .GroupBy(
                    static credit => $"{StringValue(credit, "mediaType")}:{IntValue(credit, "id")}",
                    StringComparer.Ordinal)
                // Preserve request state when only a duplicate character row
                // happens to carry Seerr's mediaInfo object.
                .Select(static group => ShapeSearchResult(
                    group.FirstOrDefault(static credit => credit["mediaInfo"] is JsonObject)
                    ?? group.First()))
                .ToArray());
        return new JsonObject
        {
            ["page"] = 1,
            ["totalPages"] = results.Count > 0 ? 1 : 0,
            ["totalResults"] = results.Count,
            ["results"] = results
        };
    }

    internal static JsonNode ShapeGenres(JsonNode? node)
        => new JsonArray(
            (node as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static genre =>
                    (IntValue(genre, "id") ?? 0) > 0
                    && !string.IsNullOrWhiteSpace(StringValue(genre, "name")))
                .Select(genre => (JsonNode)new JsonObject
                {
                    ["id"] = IntValue(genre, "id"),
                    ["name"] = StringValue(genre, "name"),
                    ["backdrops"] = new JsonArray(
                        (genre["backdrops"] as JsonArray ?? new JsonArray())
                            .Select(Clone)
                            .Where(static backdrop => backdrop is not null)
                            .ToArray())
                })
                .ToArray());

    private static JsonNode ShapeSearchResult(JsonObject result)
    {
        var mediaInfo = result["mediaInfo"] as JsonObject;
        var mediaType = StringValue(result, "mediaType") ?? string.Empty;
        return new JsonObject
        {
            ["mediaType"] = mediaType,
            ["tmdbId"] = IntValue(result, "id"),
            ["title"] = StringValue(result, mediaType == "movie" ? "title" : "name") ?? "Untitled",
            ["year"] = YearOf(StringValue(
                result,
                mediaType == "movie" ? "releaseDate" : "firstAirDate")),
            ["overview"] = Clone(result["overview"]),
            ["posterPath"] = Clone(result["posterPath"]),
            ["backdropPath"] = Clone(result["backdropPath"]),
            ["voteAverage"] = Clone(result["voteAverage"]),
            ["status"] = StatusName(IntValue(mediaInfo, "status") ?? 1),
            ["status4k"] = StatusName(IntValue(mediaInfo, "status4k") ?? 1),
            ["libraryItemId"] = null
        };
    }

    internal static JsonNode ShapeRequestDestination(JsonObject server, JsonObject detail)
    {
        var activeProfile = IntValue(server, "activeProfileId") ?? 0;
        var profiles = new JsonArray(
            (detail["profiles"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static profile =>
                    (IntValue(profile, "id") ?? 0) > 0
                    && !string.IsNullOrWhiteSpace(StringValue(profile, "name")))
                .OrderBy(profile => StringValue(profile, "name"), StringComparer.OrdinalIgnoreCase)
                .Select(profile => (JsonNode)new JsonObject
                {
                    ["id"] = IntValue(profile, "id"),
                    ["name"] = StringValue(profile, "name"),
                    ["isDefault"] = IntValue(profile, "id") == activeProfile
                })
                .ToArray());
        return new JsonObject
        {
            ["id"] = IntValue(server, "id"),
            ["name"] = StringValue(server, "name") ?? "Download service",
            ["isDefault"] = BoolValue(server, "isDefault"),
            ["profiles"] = profiles
        };
    }

    internal static JsonNode ShapeMedia(JsonObject detail, string mediaType)
    {
        var mediaInfo = detail["mediaInfo"] as JsonObject;
        var genres = new JsonArray(
            (detail["genres"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Select(genre => JsonValue.Create(StringValue(genre, "name") ?? string.Empty))
                .ToArray());
        var seasons = new JsonArray(
            (detail["seasons"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static season => (IntValue(season, "seasonNumber") ?? 0) > 0)
                .Select(season =>
                {
                    var number = IntValue(season, "seasonNumber") ?? 0;
                    var known = (mediaInfo?["seasons"] as JsonArray ?? new JsonArray())
                        .OfType<JsonObject>()
                        .FirstOrDefault(item => IntValue(item, "seasonNumber") == number);
                    return (JsonNode)new JsonObject
                    {
                        ["seasonNumber"] = number,
                        ["name"] = Clone(season["name"]),
                        ["episodeCount"] = IntValue(season, "episodeCount") ?? 0,
                        ["airDate"] = Clone(season["airDate"]),
                        ["status"] = StatusName(IntValue(known, "status") ?? 1),
                        ["status4k"] = StatusName(IntValue(known, "status4k") ?? 1)
                    };
                })
                .ToArray());
        var runtime = IntValue(detail, "runtime")
            ?? (detail["episodeRunTime"] as JsonArray)?
                .Select(static item => item?.GetValue<int?>())
                .FirstOrDefault(static value => value is > 0);
        var externalIds = detail["externalIds"] as JsonObject;

        return new JsonObject
        {
            ["mediaType"] = mediaType,
            ["tmdbId"] = IntValue(detail, "id"),
            ["title"] = StringValue(detail, mediaType == "movie" ? "title" : "name") ?? "Untitled",
            ["originalTitle"] = Clone(
                detail[mediaType == "movie" ? "originalTitle" : "originalName"]),
            ["year"] = YearOf(StringValue(
                detail,
                mediaType == "movie" ? "releaseDate" : "firstAirDate")),
            ["overview"] = Clone(detail["overview"]),
            ["tagline"] = Clone(detail["tagline"]),
            ["posterPath"] = Clone(detail["posterPath"]),
            ["backdropPath"] = Clone(detail["backdropPath"]),
            ["voteAverage"] = Clone(detail["voteAverage"]),
            ["voteCount"] = Clone(detail["voteCount"]),
            ["status"] = StatusName(IntValue(mediaInfo, "status") ?? 1),
            ["status4k"] = StatusName(IntValue(mediaInfo, "status4k") ?? 1),
            ["libraryItemId"] = null,
            ["runtimeMinutes"] = runtime,
            ["genres"] = genres,
            ["seasons"] = seasons,
            ["releaseDate"] = Clone(
                detail[mediaType == "movie" ? "releaseDate" : "firstAirDate"]),
            ["firstAirDate"] = Clone(detail["firstAirDate"]),
            ["lastAirDate"] = Clone(detail["lastAirDate"]),
            ["productionStatus"] = Clone(detail["status"]),
            ["inProduction"] = Clone(detail["inProduction"]),
            ["seriesType"] = Clone(detail["type"]),
            ["numberOfSeasons"] = Clone(detail["numberOfSeasons"]),
            ["numberOfEpisodes"] = Clone(detail["numberOfEpisodes"]),
            ["originalLanguage"] = Clone(detail["originalLanguage"]),
            ["homepage"] = Clone(detail["homepage"]),
            ["externalIds"] = new JsonObject
            {
                ["imdb"] = ImdbTitleId(detail, externalIds),
                ["tvdb"] = PositiveInt(externalIds, "tvdbId")
            },
            ["budget"] = PositiveInt(detail, "budget"),
            ["revenue"] = PositiveInt(detail, "revenue"),
            ["studios"] = StringArray(Names(detail["productionCompanies"])),
            ["networks"] = StringArray(Names(detail["networks"])),
            ["creators"] = StringArray(UniqueStrings(
                Names(detail["createdBy"]).Concat(CrewNames(detail, "Creator", null)))),
            ["directors"] = StringArray(UniqueStrings(CrewNames(detail, "Director", null))),
            ["writers"] = StringArray(UniqueStrings(CrewNames(detail, null, "Writing"))),
            ["productionCountries"] = ShapeNamedCodes(
                detail["productionCountries"],
                "iso_3166_1"),
            ["spokenLanguages"] = ShapeLanguages(detail["spokenLanguages"]),
            ["cast"] = ShapeCast(detail),
            ["trailer"] = ShapeTrailer(detail),
            ["releaseDates"] = ShapeReleaseDates(detail),
            ["contentRatings"] = ShapeContentRatings(detail),
            ["nextEpisode"] = ShapeNextEpisode(detail)
        };
    }

    private static JsonNode? PositiveInt(JsonObject? value, string name)
    {
        var number = IntValue(value, name);
        return number is > 0 ? JsonValue.Create(number.Value) : null;
    }

    private static string? ImdbTitleId(JsonObject detail, JsonObject? externalIds)
    {
        foreach (var candidate in new[]
        {
            StringValue(externalIds, "imdbId"),
            StringValue(detail, "imdbId")
        })
        {
            if (IsImdbTitleId(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    private static bool IsImdbTitleId(string? value)
    {
        if (value is not { Length: > 2 and <= 32 }
            || !value.StartsWith("tt", StringComparison.Ordinal))
        {
            return false;
        }

        for (var index = 2; index < value.Length; index++)
        {
            if (value[index] is < '0' or > '9')
            {
                return false;
            }
        }

        return true;
    }

    private static IEnumerable<string> Names(JsonNode? node)
        => (node as JsonArray ?? new JsonArray())
            .OfType<JsonObject>()
            .Select(value => StringValue(value, "name")?.Trim())
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!);

    private static IEnumerable<string> CrewNames(
        JsonObject detail,
        string? job,
        string? department)
    {
        var credits = detail["credits"] as JsonObject;
        return (credits?["crew"] as JsonArray ?? new JsonArray())
            .OfType<JsonObject>()
            .Where(person =>
                (job is null || StringValue(person, "job") == job)
                && (department is null || StringValue(person, "department") == department))
            .Select(person => StringValue(person, "name")?.Trim())
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Select(static value => value!);
    }

    private static IEnumerable<string> UniqueStrings(IEnumerable<string> values)
        => values
            .Where(static value => !string.IsNullOrWhiteSpace(value))
            .Distinct(StringComparer.OrdinalIgnoreCase);

    private static JsonArray StringArray(IEnumerable<string> values)
        => new(values.Select(static value => (JsonNode?)JsonValue.Create(value)).ToArray());

    private static JsonNode ShapeNamedCodes(JsonNode? node, string codeName)
        => new JsonArray(
            (node as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(value =>
                    !string.IsNullOrWhiteSpace(StringValue(value, codeName))
                    || !string.IsNullOrWhiteSpace(StringValue(value, "name")))
                .Select(value => (JsonNode)new JsonObject
                {
                    ["code"] = StringValue(value, codeName),
                    ["name"] = StringValue(value, "name")
                })
                .ToArray());

    private static JsonNode ShapeLanguages(JsonNode? node)
        => new JsonArray(
            (node as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(value =>
                    !string.IsNullOrWhiteSpace(StringValue(value, "iso_639_1"))
                    || !string.IsNullOrWhiteSpace(StringValue(value, "name")))
                .Select(value => (JsonNode)new JsonObject
                {
                    ["code"] = StringValue(value, "iso_639_1"),
                    ["name"] = StringValue(value, "englishName")
                        ?? StringValue(value, "name")
                })
                .ToArray());

    private static JsonNode ShapeCast(JsonObject detail)
    {
        var credits = detail["credits"] as JsonObject;
        return new JsonArray(
            (credits?["cast"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>()
                .Where(static person => !string.IsNullOrWhiteSpace(StringValue(person, "name")))
                .Take(20)
                .Select(person => (JsonNode)new JsonObject
                {
                    ["id"] = IntValue(person, "id"),
                    ["name"] = StringValue(person, "name"),
                    ["character"] = StringValue(person, "character"),
                    ["profilePath"] = Clone(person["profilePath"])
                })
                .ToArray());
    }

    private static JsonNode? ShapeTrailer(JsonObject detail)
    {
        var trailer = (detail["relatedVideos"] as JsonArray ?? new JsonArray())
            .OfType<JsonObject>()
            .Where(static video =>
                StringValue(video, "site") == "YouTube"
                && StringValue(video, "type") == "Trailer"
                && IsYoutubeKey(StringValue(video, "key")))
            .OrderByDescending(static video => IntValue(video, "size") ?? 0)
            .FirstOrDefault();
        return trailer is null
            ? null
            : new JsonObject
            {
                ["name"] = StringValue(trailer, "name") ?? "Trailer",
                ["key"] = StringValue(trailer, "key")
            };
    }

    private static bool IsYoutubeKey(string? value)
        => value is { Length: 11 }
            && value.All(static character => char.IsAsciiLetterOrDigit(character)
                || character is '-' or '_');

    private static JsonNode ShapeReleaseDates(JsonObject detail)
    {
        var results = (detail["releases"] as JsonObject)?["results"] as JsonArray
            ?? new JsonArray();
        var releases = new List<JsonNode>();
        foreach (var country in results.OfType<JsonObject>())
        {
            var region = StringValue(country, "iso_3166_1");
            if (string.IsNullOrWhiteSpace(region))
            {
                continue;
            }
            foreach (var release in (country["release_dates"] as JsonArray ?? new JsonArray())
                .OfType<JsonObject>())
            {
                var type = (IntValue(release, "type") ?? 0) switch
                {
                    1 => "premiere",
                    2 => "limited-cinema",
                    3 => "cinema",
                    4 => "digital",
                    5 => "physical",
                    6 => "tv",
                    _ => null
                };
                var date = StringValue(release, "release_date");
                if (type is null || string.IsNullOrWhiteSpace(date))
                {
                    continue;
                }
                releases.Add(new JsonObject
                {
                    ["region"] = region,
                    ["type"] = type,
                    ["date"] = date,
                    ["certification"] = StringValue(release, "certification")
                });
            }
        }
        return new JsonArray(releases.ToArray());
    }

    private static JsonNode ShapeContentRatings(JsonObject detail)
    {
        var results = (detail["contentRatings"] as JsonObject)?["results"] as JsonArray
            ?? new JsonArray();
        return new JsonArray(
            results
                .OfType<JsonObject>()
                .Where(static rating =>
                    !string.IsNullOrWhiteSpace(StringValue(rating, "iso_3166_1"))
                    && !string.IsNullOrWhiteSpace(StringValue(rating, "rating")))
                .Select(rating => (JsonNode)new JsonObject
                {
                    ["region"] = StringValue(rating, "iso_3166_1"),
                    ["rating"] = StringValue(rating, "rating")
                })
                .ToArray());
    }

    private static JsonNode? ShapeNextEpisode(JsonObject detail)
    {
        var episode = detail["nextEpisodeToAir"] as JsonObject;
        return episode is null
            ? null
            : new JsonObject
            {
                ["name"] = StringValue(episode, "name"),
                ["airDate"] = Clone(episode["airDate"]),
                ["seasonNumber"] = IntValue(episode, "seasonNumber"),
                ["episodeNumber"] = IntValue(episode, "episodeNumber")
            };
    }

    private static JsonNode ShapeRequest(JsonObject request)
    {
        var media = request["media"] as JsonObject ?? new JsonObject();
        var is4k = BoolValue(request, "is4k");
        var mediaType = StringValue(request, "type")
            ?? StringValue(media, "mediaType")
            ?? string.Empty;
        return new JsonObject
        {
            ["id"] = IntValue(request, "id"),
            ["status"] = RequestStatusName(IntValue(request, "status") ?? 0),
            ["mediaType"] = mediaType,
            ["tmdbId"] = IntValue(media, "tmdbId"),
            ["is4k"] = is4k,
            ["createdAt"] = Clone(request["createdAt"]),
            ["updatedAt"] = Clone(request["updatedAt"]),
            ["mediaStatus"] = StatusName(
                IntValue(media, is4k ? "status4k" : "status") ?? 1),
            ["seasons"] = new JsonArray(
                (request["seasons"] as JsonArray ?? new JsonArray())
                    .OfType<JsonObject>()
                    .Select(season => JsonValue.Create(IntValue(season, "seasonNumber") ?? 0))
                    .ToArray()),
            ["libraryItemId"] = null
        };
    }

    private static JsonNode Capabilities(ulong permissions, bool movie4k, bool tv4k)
    {
        var admin = (permissions & Admin) != 0;
        bool Has(ulong mask) => admin || (permissions & mask) != 0;
        JsonNode Capability(bool request, bool approve) => new JsonObject
        {
            ["request"] = request,
            ["autoApprove"] = approve
        };

        return new JsonObject
        {
            ["movie"] = Capability(
                Has(Request | RequestMovie),
                Has(AutoApprove | AutoApproveMovie)),
            ["tv"] = Capability(
                Has(Request | RequestTv),
                Has(AutoApprove | AutoApproveTv)),
            ["movie4k"] = Capability(
                movie4k && Has(Request4k | Request4kMovie),
                movie4k && Has(AutoApprove4k | AutoApprove4kMovie)),
            ["tv4k"] = Capability(
                tv4k && Has(Request4k | Request4kTv),
                tv4k && Has(AutoApprove4k | AutoApprove4kTv)),
            ["advancedRequest"] = Has(RequestAdvanced)
        };
    }

    private static bool HasPermission(ulong permissions, ulong permission)
        => (permissions & Admin) != 0 || (permissions & permission) != 0;

    private static string PreferredUserName(JsonObject user)
        => new[]
        {
            StringValue(user, "displayName"),
            StringValue(user, "username"),
            StringValue(user, "jellyfinUsername"),
            StringValue(user, "email")
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

    private static int? YearOf(string? date)
        => date is { Length: >= 4 }
            && int.TryParse(date.AsSpan(0, 4), NumberStyles.None, CultureInfo.InvariantCulture, out var year)
                ? year
                : null;

    private static string? StringValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<string>(out var parsed)
            ? parsed
            : null;

    private static int? IntValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<int>(out var parsed)
            ? parsed
            : null;

    private static ulong? ULongValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<ulong>(out var parsed)
            ? parsed
            : null;

    private static bool BoolValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<bool>(out var parsed) && parsed;

    private static JsonNode? Clone(JsonNode? node) => node?.DeepClone();

    private sealed record MappingRecord(int SeerrUserId, DateTimeOffset CachedAt);
}
