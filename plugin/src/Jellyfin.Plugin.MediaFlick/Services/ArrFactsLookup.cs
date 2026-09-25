using System.Collections.Concurrent;
using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The Radarr and Sonarr calls <see cref="ArrFactsLookup"/> makes, so the
/// lookup can be driven without live services.
/// </summary>
internal interface IArrTransport
{
    bool IsConfigured(string service);

    Task<JsonNode?> SendAsync(string service, string path, CancellationToken cancellationToken);
}

/// <summary>Sends Radarr and Sonarr calls through the shared Companion HTTP client.</summary>
internal sealed class CompanionArrTransport : IArrTransport
{
    private readonly CompanionHttpClient _http;

    public CompanionArrTransport(CompanionHttpClient http)
    {
        _http = http;
    }

    public bool IsConfigured(string service)
        => Configuration(service) is { Enabled: true } configuration
            && !string.IsNullOrWhiteSpace(configuration.ApiKey)
            && Uri.TryCreate(configuration.BaseUrl, UriKind.Absolute, out var uri)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps);

    public Task<JsonNode?> SendAsync(string service, string path, CancellationToken cancellationToken)
        => _http.SendAsync(
            service,
            Configuration(service)
                ?? throw new GatewayException(
                    StatusCodes.Status503ServiceUnavailable,
                    "the plugin is not initialized",
                    ServiceFailure.NotConfigured),
            HttpMethod.Get,
            path,
            null,
            null,
            cancellationToken);

    private static ServiceConfiguration? Configuration(string service)
        => service switch
        {
            "radarr" => Plugin.Instance?.Configuration.Radarr,
            "sonarr" => Plugin.Instance?.Configuration.Sonarr,
            _ => null
        };
}

/// <summary>
/// Looks up the Radarr movies and Sonarr series behind Seerr titles that are
/// processing. Each lookup is one small by-id request, cached briefly so
/// repeated pages do not repeat it. The facts only refine a status badge, so
/// every failure degrades to "no facts" and never fails the Seerr response.
/// </summary>
public sealed class ArrFactsLookup
{
    internal static readonly TimeSpan FactsLifetime = TimeSpan.FromMinutes(5);

    /// <summary>How long a Seerr page may wait for the arr answers.</summary>
    internal static readonly TimeSpan LookupBudget = TimeSpan.FromSeconds(3);

    /// <summary>How long a failing service is skipped before it is asked again.</summary>
    internal static readonly TimeSpan OutageBackoff = TimeSpan.FromMinutes(1);

    internal const int CacheCapacity = 2_000;

    private readonly IArrTransport _transport;
    private readonly ILogger<ArrFactsLookup> _logger;
    private readonly TimeProvider _time;
    private readonly BoundedCache<int, RadarrMovieFacts?> _movies;
    private readonly BoundedCache<int, SonarrSeriesFacts?> _series;
    private readonly ConcurrentDictionary<string, DateTimeOffset> _skipUntil =
        new(StringComparer.Ordinal);

    internal ArrFactsLookup(
        IArrTransport transport,
        ILogger<ArrFactsLookup> logger,
        TimeProvider? timeProvider = null)
    {
        _transport = transport;
        _logger = logger;
        _time = timeProvider ?? TimeProvider.System;
        _movies = new BoundedCache<int, RadarrMovieFacts?>(CacheCapacity, FactsLifetime, _time);
        _series = new BoundedCache<int, SonarrSeriesFacts?>(CacheCapacity, FactsLifetime, _time);
    }

    internal async Task<ArrFacts> LookupAsync(
        IEnumerable<int> movieTmdbIds,
        IEnumerable<int> seriesTvdbIds,
        CancellationToken cancellationToken)
    {
        var movieIds = movieTmdbIds.Where(static id => id > 0).Distinct().ToArray();
        var seriesIds = seriesTvdbIds.Where(static id => id > 0).Distinct().ToArray();
        if (movieIds.Length == 0 && seriesIds.Length == 0)
        {
            return ArrFacts.None;
        }

        using var budget = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        budget.CancelAfter(LookupBudget);
        var movies = LookupAllAsync(
            "radarr",
            movieIds,
            _movies,
            id => string.Create(
                CultureInfo.InvariantCulture,
                $"api/v3/movie?tmdbId={id}&excludeLocalCovers=true"),
            ParseRadarrMovie,
            budget.Token);
        var series = LookupAllAsync(
            "sonarr",
            seriesIds,
            _series,
            id => string.Create(CultureInfo.InvariantCulture, $"api/v3/series?tvdbId={id}"),
            ParseSonarrSeries,
            budget.Token);
        await Task.WhenAll(movies, series).ConfigureAwait(false);
        cancellationToken.ThrowIfCancellationRequested();
        return new ArrFacts(await movies.ConfigureAwait(false), await series.ConfigureAwait(false));
    }

    private async Task<IReadOnlyDictionary<int, TFacts>> LookupAllAsync<TFacts>(
        string service,
        IReadOnlyList<int> ids,
        BoundedCache<int, TFacts?> cache,
        Func<int, string> path,
        Func<JsonNode?, int, TFacts?> parse,
        CancellationToken cancellationToken)
        where TFacts : class
    {
        var found = new Dictionary<int, TFacts>();
        var missing = new List<int>();
        foreach (var id in ids)
        {
            if (!cache.TryGet(id, out var cached))
            {
                missing.Add(id);
            }
            else if (cached is not null)
            {
                found[id] = cached;
            }
        }

        if (missing.Count == 0 || !Available(service))
        {
            return found;
        }

        var answers = await Task.WhenAll(missing.Select(async id =>
        {
            try
            {
                var response = await _transport.SendAsync(service, path(id), cancellationToken)
                    .ConfigureAwait(false);
                var facts = parse(response, id);
                cache.Set(id, facts);
                return (Id: id, Facts: facts);
            }
            catch (GatewayException exception) when (exception.Failure == ServiceFailure.RequestFailed)
            {
                // The service answered; it just does not know this title.
                cache.Set(id, null);
                return (Id: id, Facts: (TFacts?)null);
            }
            catch (GatewayException exception)
            {
                // CompanionHttpClient has already logged and recorded the outage.
                _skipUntil[service] = _time.GetUtcNow() + OutageBackoff;
                _logger.LogDebug(
                    "{Service} lookup failed ({Failure}); showing Seerr status without it",
                    service,
                    exception.Failure);
                return (Id: id, Facts: (TFacts?)null);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                _logger.LogDebug(
                    "{Service} lookup exceeded its {Budget} second budget",
                    service,
                    LookupBudget.TotalSeconds);
                return (Id: id, Facts: (TFacts?)null);
            }
        })).ConfigureAwait(false);

        foreach (var (id, facts) in answers)
        {
            if (facts is not null)
            {
                found[id] = facts;
            }
        }

        return found;
    }

    private bool Available(string service)
        => _transport.IsConfigured(service)
            && !(_skipUntil.TryGetValue(service, out var until) && _time.GetUtcNow() < until);

    internal static RadarrMovieFacts? ParseRadarrMovie(JsonNode? response, int tmdbId)
        => (response as JsonArray)?
            .OfType<JsonObject>()
            .Where(movie => JsonRead.Int32(movie, "tmdbId") == tmdbId)
            .Select(static movie => new RadarrMovieFacts(
                JsonRead.String(movie, "status"),
                JsonRead.Bool(movie, "isAvailable") == true,
                JsonRead.Bool(movie, "hasFile") == true))
            .FirstOrDefault();

    internal static SonarrSeriesFacts? ParseSonarrSeries(JsonNode? response, int tvdbId)
    {
        var series = (response as JsonArray)?
            .OfType<JsonObject>()
            .FirstOrDefault(series => JsonRead.Int32(series, "tvdbId") == tvdbId);
        if (series is null)
        {
            return null;
        }

        var seasons = new Dictionary<int, SonarrEpisodeFacts>();
        foreach (var season in (series["seasons"] as JsonArray)?.OfType<JsonObject>() ?? [])
        {
            if (JsonRead.Int32(season, "seasonNumber") is { } number and > 0)
            {
                var statistics = season["statistics"] as JsonObject;
                seasons[number] = Episodes(
                    statistics,
                    JsonRead.String(statistics, "nextAiring") is not null);
            }
        }

        return new SonarrSeriesFacts(
            Episodes(
                series["statistics"] as JsonObject,
                JsonRead.String(series, "nextAiring") is not null
                    || JsonRead.String(series, "status") == "upcoming"),
            seasons);
    }

    private static SonarrEpisodeFacts Episodes(JsonObject? statistics, bool moreScheduled)
        => new(
            JsonRead.Int32(statistics, "episodeCount") ?? 0,
            JsonRead.Int32(statistics, "episodeFileCount") ?? 0,
            moreScheduled);
}
