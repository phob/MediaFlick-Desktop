using System.Globalization;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal static partial class RatingsContract
{
    public const int BoundaryVersion = 1;
    public const int MaxRequestItems = 500;
    public const int MaxUpstreamBatchItems = 100;
    public const string Origin = "server_mdblist";

    public static IReadOnlyList<RatingSourceResponse> SourceCatalog { get; } =
    [
        new("mdblist_score", "MDBList Score", "MDB", 100, "percent", true),
        new("mdblist_score_average", "MDBList Score Average", "AVG", 100, "percent", true),
        new("imdb", "IMDb", "IMDb", 10, "decimal", true),
        new("trakt", "Trakt", "Trakt", 100, "percent", true),
        new("tmdb", "TMDB", "TMDB", 10, "decimal", true),
        new("letterboxd", "Letterboxd", "LB", 5, "stars", true),
        new("tomatoes", "Rotten Tomatoes Critics", "RT", 100, "percent", true),
        new("popcorn", "Rotten Tomatoes Audience", "RT A", 100, "percent", true),
        new("metacritic", "Metacritic Critics", "MC", 100, "integer", true),
        new("metacriticuser", "Metacritic Users", "MC U", 10, "decimal", true),
        new("rogerebert", "Roger Ebert", "Ebert", 4, "stars", true),
        new("myanimelist", "MyAnimeList", "MAL", 10, "decimal", true)
    ];

    public static IReadOnlyList<RatingTargetRequest> Validate(RatingBatchRequest? request)
    {
        if (request is null)
        {
            throw new RatingRequestException("a JSON request body is required");
        }

        if (request.BoundaryVersion != BoundaryVersion)
        {
            throw new RatingContractVersionException(request.BoundaryVersion);
        }

        if (request.Items is null || request.Items.Count == 0)
        {
            throw new RatingRequestException("items must contain at least one media identity");
        }

        if (request.Items.Count > MaxRequestItems)
        {
            throw new RatingRequestException(
                $"items is limited to {MaxRequestItems.ToString(CultureInfo.InvariantCulture)} entries");
        }

        var result = new List<RatingTargetRequest>(request.Items.Count);
        var itemIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in request.Items)
        {
            var itemId = item.ItemId?.Trim() ?? string.Empty;
            var provider = item.Provider?.Trim().ToLowerInvariant() ?? string.Empty;
            var providerId = item.ProviderId?.Trim() ?? string.Empty;
            var mediaType = item.MediaType?.Trim().ToLowerInvariant() ?? string.Empty;
            var kind = item.Kind?.Trim() ?? string.Empty;
            if (!ValidItemId().IsMatch(itemId))
            {
                throw new RatingRequestException("itemId contains an invalid Jellyfin item identifier");
            }

            if (!itemIds.Add(itemId))
            {
                throw new RatingRequestException("itemId values must be unique within a batch");
            }

            if (mediaType is not ("movie" or "show"))
            {
                throw new RatingRequestException("mediaType must be movie or show");
            }

            if (provider == "tmdb")
            {
                if (!TmdbId().IsMatch(providerId)
                    || !long.TryParse(providerId, NumberStyles.None, CultureInfo.InvariantCulture, out var number)
                    || number <= 0)
                {
                    throw new RatingRequestException("TMDB providerId must be a positive numeric identifier");
                }
            }
            else if (provider == "imdb")
            {
                providerId = providerId.ToLowerInvariant();
                if (!ImdbId().IsMatch(providerId))
                {
                    throw new RatingRequestException("IMDb providerId must be a stable tt identifier");
                }
            }
            else
            {
                throw new RatingRequestException("provider must be tmdb or imdb");
            }

            if (kind.Length is 0 or > 32 || kind.Any(char.IsControl))
            {
                throw new RatingRequestException("kind is invalid");
            }

            result.Add(new RatingTargetRequest(itemId, kind, mediaType, provider, providerId));
        }

        return result;
    }

    public static JsonArray NormalizeMedia(JsonNode item)
    {
        var bySource = new SortedDictionary<string, JsonObject>(StringComparer.Ordinal);
        if (item["ratings"] is JsonArray ratings)
        {
            foreach (var candidate in ratings.OfType<JsonObject>())
            {
                var rawSource = String(candidate["source"])
                    ?? String(candidate["sourceId"]);
                if (string.IsNullOrWhiteSpace(rawSource))
                {
                    continue;
                }

                AddRating(bySource, rawSource, Number(candidate["value"] ?? candidate["rating"]),
                    Number(candidate["score"]), Integer(candidate["votes"]));
            }
        }

        AddRating(bySource, "mdblist_score", Number(item["score"]), Number(item["score"]), null);
        AddRating(
            bySource,
            "mdblist_score_average",
            Number(item["score_average"] ?? item["scoreAverage"]),
            Number(item["score_average"] ?? item["scoreAverage"]),
            null);
        return new JsonArray(bySource.Values.Select(value => (JsonNode)value).ToArray());
    }

    public static string CanonicalSource(string source)
    {
        var compact = source.Trim().ToLowerInvariant().Replace(' ', '_').Replace('-', '_');
        return compact switch
        {
            "mal" => "myanimelist",
            "audience" or "popcorn" or "tomatoesaudience" or "tomatoes_audience"
                or "rtaudience" or "rt_audience" => "popcorn",
            "tomato" or "tomatometer" or "rtomatoes" or "rt_critic" => "tomatoes",
            "score" or "mdblist" or "mdblist_score" => "mdblist_score",
            "scoreaverage" or "score_average" or "mdblist_score_average" =>
                "mdblist_score_average",
            _ => SafeSource(compact)
        };
    }

    public static bool ValidTmdbKeyShape(string value)
        => (value.Length == 32 && value.All(Uri.IsHexDigit))
            || (value.StartsWith("eyJ", StringComparison.Ordinal)
                && value.Count(character => character == '.') == 2
                && value.Length <= 2048);

    public static string StableKey(RatingTargetRequest target)
        => string.Join('|', target.Provider, target.MediaType, target.ProviderId);

    private static void AddRating(
        IDictionary<string, JsonObject> ratings,
        string rawSource,
        double? rawValue,
        double? score,
        long? votes)
    {
        var source = CanonicalSource(rawSource);
        var maximum = ScaleMax(source);
        var value = rawValue is { } raw && raw >= 0 && raw <= maximum
            ? raw
            : score is { } normalized && normalized >= 0 && normalized <= 100
                ? normalized * maximum / 100
                : (double?)null;
        if (value is null)
        {
            return;
        }

        var rating = new JsonObject
        {
            // `source` keeps Desktop v1's normalizer on its established path;
            // sourceId is the canonical server-side catalog identity.
            ["source"] = rawSource,
            ["sourceId"] = source,
            ["rawSource"] = rawSource,
            ["value"] = value,
            ["scaleMax"] = maximum
        };
        if (score is { } scoreValue && scoreValue >= 0 && scoreValue <= 100)
        {
            rating["score"] = scoreValue;
        }

        if (votes is >= 0)
        {
            rating["votes"] = votes;
        }

        ratings[source] = rating;
    }

    private static double ScaleMax(string source)
        => source switch
        {
            "imdb" or "tmdb" or "metacriticuser" or "myanimelist" => 10,
            "letterboxd" => 5,
            "rogerebert" => 4,
            _ => 100
        };

    private static string SafeSource(string source)
    {
        var safe = new string(source
            .Where(character => char.IsAsciiLetterLower(character)
                || char.IsAsciiDigit(character)
                || character is '_' or '-')
            .Take(64)
            .ToArray());
        return safe.Length == 0 ? "unknown" : safe;
    }

    private static string? String(JsonNode? node)
    {
        try
        {
            return node?.GetValue<string>();
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    private static double? Number(JsonNode? node)
    {
        if (node is null)
        {
            return null;
        }

        try
        {
            if (node is JsonValue jsonValue
                && node.GetValueKind() == System.Text.Json.JsonValueKind.Number
                && jsonValue.TryGetValue<double>(out var number)
                && double.IsFinite(number))
            {
                return number;
            }

            var text = node.GetValue<string>();
            return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out number)
                && double.IsFinite(number)
                    ? number
                    : null;
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    private static long? Integer(JsonNode? node)
    {
        var value = Number(node);
        return value is { } number && number >= 0 && number <= long.MaxValue
            ? Convert.ToInt64(Math.Truncate(number))
            : null;
    }

    [GeneratedRegex("^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$", RegexOptions.CultureInvariant)]
    private static partial Regex ValidItemId();

    [GeneratedRegex("^[1-9][0-9]{0,18}$", RegexOptions.CultureInvariant)]
    private static partial Regex TmdbId();

    [GeneratedRegex("^tt[0-9]{5,12}$", RegexOptions.CultureInvariant)]
    private static partial Regex ImdbId();
}

internal sealed class RatingRequestException(string message) : Exception(message);

internal sealed class RatingContractVersionException(int requestedVersion)
    : Exception($"ratings boundary version {requestedVersion} is not supported")
{
    public int RequestedVersion { get; } = requestedVersion;
}
