using System.Globalization;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The only shape allowed to cross from MDBList data into the desktop-facing
/// ratings boundary. Upstream JSON is data, never a response DTO: every field
/// below is reconstructed from a fixed catalog and bounded primitive values.
/// </summary>
internal static partial class RatingsContract
{
    public const int BoundaryVersion = 1;
    public const int MaxRequestItems = 500;
    public const int MaxUpstreamBatchItems = 100;
    public const string Origin = "server_mdblist";
    public const long MaxVotes = 1_000_000_000_000;
    public const long MaxQuotaValue = 1_000_000_000_000;
    public const long MaxUnixTimestamp = 4_102_444_800; // 2100-01-01 UTC

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

    private static readonly IReadOnlyDictionary<string, double> SourceScales =
        SourceCatalog.ToDictionary(source => source.Id, source => source.ScaleMax, StringComparer.Ordinal);

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

    /// <summary>
    /// Rebuilds ratings from the fixed public source catalog. Unknown source
    /// labels, arbitrary upstream fields, and original strings are discarded.
    /// The legacy source/rawSource keys remain for Desktop v1 compatibility,
    /// but carry the canonical catalog id rather than upstream text.
    /// </summary>
    public static JsonArray NormalizeMedia(JsonNode? item)
    {
        var bySource = new SortedDictionary<string, JsonObject>(StringComparer.Ordinal);
        if (item?["ratings"] is JsonArray ratings)
        {
            foreach (var candidate in ratings.OfType<JsonObject>())
            {
                AddRating(
                    bySource,
                    NodeString(candidate["source"]) ?? NodeString(candidate["sourceId"]),
                    Number(candidate["value"] ?? candidate["rating"]),
                    Number(candidate["score"]),
                    Integer(candidate["votes"]));
            }
        }

        AddRating(bySource, "mdblist_score", Number(item?["score"]), Number(item?["score"]), null);
        AddRating(
            bySource,
            "mdblist_score_average",
            Number(item?["score_average"] ?? item?["scoreAverage"]),
            Number(item?["score_average"] ?? item?["scoreAverage"]),
            null);
        return new JsonArray(bySource.Values.Select(value => (JsonNode)value).ToArray());
    }

    /// <summary>
    /// Revalidates persisted records before they can be returned. This repairs
    /// cache records from earlier releases as well as rejecting any tampering.
    /// </summary>
    public static JsonArray NormalizeCachedRatings(JsonArray? ratings)
    {
        var bySource = new SortedDictionary<string, JsonObject>(StringComparer.Ordinal);
        if (ratings is not null)
        {
            foreach (var candidate in ratings.OfType<JsonObject>())
            {
                // Read only sourceId from a cache record. Older raw `source` /
                // `rawSource` values are deliberately never re-emitted.
                AddRating(
                    bySource,
                    NodeString(candidate["sourceId"]),
                    Number(candidate["value"]),
                    Number(candidate["score"]),
                    Integer(candidate["votes"]));
            }
        }

        return new JsonArray(bySource.Values.Select(value => (JsonNode)value).ToArray());
    }

    public static string? NormalizeSourceUpdatedAt(JsonNode? value)
    {
        var text = NodeString(value)?.Trim();
        if (string.IsNullOrEmpty(text)
            || text.Length > 40
            || text.Any(character => !(char.IsAsciiDigit(character)
                || character is '-' or ':' or 'T' or 'Z' or '.' or '+')))
        {
            return null;
        }

        return DateTimeOffset.TryParse(
            text,
            CultureInfo.InvariantCulture,
            DateTimeStyles.AllowWhiteSpaces | DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal,
            out var parsed)
            ? parsed.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture)
            : null;
    }

    public static RatingQuotaResponse NormalizeQuota(RatingQuotaResponse quota)
        => new(
            BoundedNonNegative(quota.Limit, MaxQuotaValue),
            BoundedNonNegative(quota.Remaining, MaxQuotaValue),
            BoundedNonNegative(quota.ResetAt, MaxUnixTimestamp));

    public static long? NormalizeTimestamp(long? value)
        => BoundedNonNegative(value, MaxUnixTimestamp);

    public static string NormalizeValidation(string? validation)
        => validation?.Trim().ToLowerInvariant() switch
        {
            "valid" => "valid",
            "invalid" => "invalid",
            "offline" => "offline",
            "rate_limited" => "rate_limited",
            "unavailable" => "unavailable",
            "saved" => "saved",
            "unchecked" => "unchecked",
            _ => "unchecked"
        };

    public static string? StatusDetail(string validation, string provider)
        => validation switch
        {
            "valid" when provider == RatingProviders.MdbList => "Valid MDBList credential.",
            "valid" => "Credential is valid.",
            "invalid" when provider == RatingProviders.MdbList => "MDBList rejected the saved API key.",
            "invalid" => "The saved credential is invalid.",
            "offline" or "unavailable" => "MDBList is temporarily unavailable; cached ratings remain available.",
            "rate_limited" => "MDBList quota is exhausted; cached ratings remain available.",
            "saved" => "Saved for future TMDB features. Rating retrieval does not use this key.",
            _ => null
        };

    public static bool ValidTmdbKeyShape(string value)
        => (value.Length == 32 && value.All(Uri.IsHexDigit))
            || (value.StartsWith("eyJ", StringComparison.Ordinal)
                && value.Count(character => character == '.') == 2
                && value.Length <= 2048);

    public static string StableKey(RatingTargetRequest target)
        => string.Join('|', target.Provider, target.MediaType, target.ProviderId);

    private static void AddRating(
        IDictionary<string, JsonObject> ratings,
        string? rawSource,
        double? rawValue,
        double? score,
        long? votes)
    {
        if (!TryCanonicalSource(rawSource, out var source)
            || !SourceScales.TryGetValue(source, out var maximum))
        {
            return;
        }

        var safeScore = score is { } scoreValue && scoreValue is >= 0 and <= 100
            ? scoreValue
            : (double?)null;
        var value = rawValue is { } raw && raw >= 0 && raw <= maximum
            ? raw
            : safeScore is { } normalized
                ? normalized * maximum / 100
                : (double?)null;
        if (value is null)
        {
            return;
        }

        var rating = new JsonObject
        {
            // These are all catalog constants; no upstream string survives.
            ["source"] = source,
            ["sourceId"] = source,
            ["rawSource"] = source,
            ["value"] = value,
            ["scaleMax"] = maximum
        };
        if (safeScore is { } safeScoreValue)
        {
            rating["score"] = safeScoreValue;
        }

        if (votes is { } voteCount && voteCount <= MaxVotes)
        {
            rating["votes"] = voteCount;
        }

        ratings[source] = rating;
    }

    private static bool TryCanonicalSource(string? rawSource, out string source)
    {
        source = string.Empty;
        if (string.IsNullOrWhiteSpace(rawSource) || rawSource.Length > 128)
        {
            return false;
        }

        var compact = rawSource.Trim().ToLowerInvariant().Replace(' ', '_').Replace('-', '_');
        source = compact switch
        {
            "mal" => "myanimelist",
            "audience" or "popcorn" or "tomatoesaudience" or "tomatoes_audience"
                or "rtaudience" or "rt_audience" => "popcorn",
            "tomato" or "tomatometer" or "rtomatoes" or "rt_critic" => "tomatoes",
            "score" or "mdblist" or "mdblist_score" => "mdblist_score",
            "scoreaverage" or "score_average" or "mdblist_score_average" =>
                "mdblist_score_average",
            _ => compact
        };
        return SourceScales.ContainsKey(source);
    }

    private static long? BoundedNonNegative(long? value, long maximum)
        => value is { } number && number >= 0 && number <= maximum ? number : null;

    private static string? NodeString(JsonNode? node)
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
            return text.Length <= 32
                && double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out number)
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
        return value is { } number && number >= 0 && number <= MaxVotes
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
