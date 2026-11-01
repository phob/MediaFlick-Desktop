using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Discover catalogs and their filters: the allowlisted discovery parameters
/// and the company search behind the movie catalog's studio filter.
/// </summary>
public sealed partial class SeerrGateway
{
    private const int MaxCompanyQueryLength = 100;

    /// <summary>TMDB serves at most 500 pages of any search.</summary>
    private const int MaxCompanyPage = 500;

    public async Task<SeerrPageResponse<SeerrResultResponse>> DiscoverAsync(
        Guid jellyfinUserId,
        string kind,
        int page,
        int? genre,
        string? sortBy,
        int? voteAverageGte,
        int? releaseDecade,
        int? studio,
        int? network,
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
            studio,
            network,
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
        int? studio,
        int? network,
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
        // Seerr filters the movie catalog by TMDB company and the TV catalog
        // by TMDB network; neither applies to any other endpoint.
        if (studio is { } studioId && (studioId <= 0 || endpoint != "movies"))
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "studio must be a positive TMDB company id on the movie catalog");
        }
        if (network is { } networkId && (networkId <= 0 || endpoint != "tv"))
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "network must be a positive TMDB network id on the TV catalog");
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
            if (studio is { } studioFilter)
            {
                query.Add(string.Create(CultureInfo.InvariantCulture, $"studio={studioFilter}"));
            }
            if (network is { } networkFilter)
            {
                query.Add(string.Create(CultureInfo.InvariantCulture, $"network={networkFilter}"));
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

    /// <summary>
    /// TMDB companies by name, for the movie catalog's studio filter. The
    /// query and page are bounded before Seerr sees them.
    /// </summary>
    public async Task<SeerrPageResponse<SeerrCompanyResponse>> CompaniesAsync(
        Guid jellyfinUserId,
        string? query,
        int page,
        CancellationToken cancellationToken)
    {
        var term = query?.Trim();
        if (string.IsNullOrEmpty(term) || term.Length > MaxCompanyQueryLength)
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                string.Create(
                    CultureInfo.InvariantCulture,
                    $"query must be 1 to {MaxCompanyQueryLength} characters"));
        }
        if (page is < 1 or > MaxCompanyPage)
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                string.Create(CultureInfo.InvariantCulture, $"page must be 1 to {MaxCompanyPage}"));
        }

        var user = await ResolveUserAsync(jellyfinUserId, cancellationToken).ConfigureAwait(false);
        var response = await SendMappedAsync(
            HttpMethod.Get,
            string.Create(
                CultureInfo.InvariantCulture,
                $"api/v1/search/company?query={Uri.EscapeDataString(term)}&page={page}"),
            null,
            user,
            cancellationToken).ConfigureAwait(false) as JsonObject;
        var results = Companies(response?["results"]);
        return new SeerrPageResponse<SeerrCompanyResponse>(
            JsonRead.Int32(response, "page") ?? page,
            JsonRead.Int32(response, "totalPages") ?? 0,
            JsonRead.Int32(response, "totalResults") ?? results.Length,
            results);
    }

    private static SeerrCompanyResponse[] Companies(JsonNode? node)
        => Objects(node)
            .Select(static value => (
                Id: JsonRead.Int32(value, "id"),
                Name: JsonRead.String(value, "name")?.Trim()))
            .Where(static value => value.Id is > 0 && !string.IsNullOrWhiteSpace(value.Name))
            .Select(static value => new SeerrCompanyResponse(value.Id!.Value, value.Name!))
            .ToArray();
}
