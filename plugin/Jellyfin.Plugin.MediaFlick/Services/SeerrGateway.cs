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

    public async Task<JsonNode> DiscoverAsync(
        Guid jellyfinUserId,
        string kind,
        int page,
        CancellationToken cancellationToken)
    {
        var endpoint = kind.ToLowerInvariant() switch
        {
            "trending" => "trending",
            "movies" => "movies",
            "tv" => "tv",
            _ => throw new GatewayException(StatusCodes.Status404NotFound, "unknown discover kind")
        };
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            string.Create(
                CultureInfo.InvariantCulture,
                $"api/v1/discover/{endpoint}?page={Math.Max(1, page)}"),
            null,
            user,
            cancellationToken).ConfigureAwait(false);
        return ShapeSearchPage(response);
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

    public async Task<JsonNode> RequestAsync(
        Guid jellyfinUserId,
        SeerrRequestBody body,
        CancellationToken cancellationToken)
    {
        var type = ValidateMediaType(body.MediaType);
        ValidatePositive(body.TmdbId, "TMDB id");
        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
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

    private static JsonNode ShapeMedia(JsonObject detail, string mediaType)
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
                        ["status"] = StatusName(IntValue(known, "status") ?? 1)
                    };
                })
                .ToArray());
        var runtime = IntValue(detail, "runtime")
            ?? (detail["episodeRunTime"] as JsonArray)?
                .Select(static item => item?.GetValue<int?>())
                .FirstOrDefault(static value => value is > 0);

        return new JsonObject
        {
            ["mediaType"] = mediaType,
            ["tmdbId"] = IntValue(detail, "id"),
            ["title"] = StringValue(detail, mediaType == "movie" ? "title" : "name") ?? "Untitled",
            ["year"] = YearOf(StringValue(
                detail,
                mediaType == "movie" ? "releaseDate" : "firstAirDate")),
            ["overview"] = Clone(detail["overview"]),
            ["posterPath"] = Clone(detail["posterPath"]),
            ["backdropPath"] = Clone(detail["backdropPath"]),
            ["voteAverage"] = Clone(detail["voteAverage"]),
            ["status"] = StatusName(IntValue(mediaInfo, "status") ?? 1),
            ["status4k"] = StatusName(IntValue(mediaInfo, "status4k") ?? 1),
            ["libraryItemId"] = null,
            ["runtimeMinutes"] = runtime,
            ["genres"] = genres,
            ["seasons"] = seasons
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
                tv4k && Has(AutoApprove4k | AutoApprove4kTv))
        };
    }

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
