using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CalendarService
{
    // Radarr can return its complete movie list in one request. Sonarr's
    // calendar requires dates, so this bounded century covers real TV history
    // and every plausible announced episode without per-series requests.
    internal static readonly DateOnly CalendarStart = new(1900, 1, 1);
    internal static readonly DateOnly CalendarEnd = new(2100, 1, 1);
    internal static readonly TimeSpan CacheLifetime = TimeSpan.FromHours(24);
    internal static readonly TimeSpan FailureRetryInterval = TimeSpan.FromMinutes(15);
    internal const string RadarrPath = "api/v3/movie?excludeLocalCovers=true";

    private readonly CompanionHttpClient _http;
    private readonly CalendarCache _cache;
    private readonly ServiceHealthStore _health;
    private readonly SemaphoreSlim _refreshGate = new(1, 1);

    public CalendarService(
        CompanionHttpClient http,
        CalendarCache cache,
        ServiceHealthStore health)
    {
        _http = http;
        _cache = cache;
        _health = health;
    }

    public async Task RefreshAsync(
        IProgress<double>? progress,
        CancellationToken cancellationToken)
    {
        await _refreshGate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var now = DateTimeOffset.UtcNow;
            if (IsFresh(_cache.Snapshot(), now))
            {
                progress?.Report(100);
                return;
            }

            var plugin = Plugin.Instance
                ?? throw new InvalidOperationException("The MediaFlick plugin is not initialized");
            var configuration = plugin.Configuration;
            var completed = 0;

            await RefreshSourceAsync(
                "sonarr",
                configuration.Sonarr,
                SonarrPath(),
                ParseSonarr,
                cancellationToken).ConfigureAwait(false);
            progress?.Report(++completed * 50);

            await RefreshSourceAsync(
                "radarr",
                configuration.Radarr,
                RadarrPath,
                ParseRadarr,
                cancellationToken).ConfigureAwait(false);
            progress?.Report(++completed * 50);
            _cache.MarkRefreshAttempt(DateTimeOffset.UtcNow);
        }
        finally
        {
            _refreshGate.Release();
        }
    }

    public async Task<CalendarResponse> GetAsync(
        DateOnly? requestedStart,
        DateOnly? requestedEnd,
        CancellationToken cancellationToken)
    {
        var start = requestedStart ?? CalendarStart;
        var end = requestedEnd ?? CalendarEnd;
        if (end < start)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "end must not precede start");
        }

        await RefreshAsync(null, cancellationToken).ConfigureAwait(false);
        return GetCached(start, end);
    }

    internal static bool IsFresh(CalendarCache.CalendarState snapshot, DateTimeOffset now)
    {
        var retryAfter = snapshot.Sources.Values.Any(static source =>
            source.Enabled && !source.Available)
            ? FailureRetryInterval
            : CacheLifetime;
        return snapshot.LastAttemptAt is { } attemptedAt
            && now - attemptedAt < retryAfter;
    }

    internal static string SonarrPath()
        => string.Create(
            CultureInfo.InvariantCulture,
            $"api/v3/calendar?start={CalendarStart:yyyy-MM-dd}&end={CalendarEnd:yyyy-MM-dd}&unmonitored=false&includeSeries=true");

    private CalendarResponse GetCached(DateOnly start, DateOnly end)
    {
        var snapshot = _cache.Snapshot();
        var entries = snapshot.BySource.Values
            .SelectMany(static entries => entries)
            .Where(entry => DateOnly.TryParseExact(
                entry.Date,
                "yyyy-MM-dd",
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out var date)
                && date >= start
                && date <= end)
            .OrderBy(static entry => entry.Date, StringComparer.Ordinal)
            .ThenBy(static entry => entry.SeriesTitle ?? entry.Title, StringComparer.OrdinalIgnoreCase)
            .ThenBy(static entry => entry.Season)
            .ThenBy(static entry => entry.Episode)
            .ToArray();

        return new CalendarResponse(
            entries,
            snapshot.RefreshedAt,
            snapshot.Sources,
            snapshot.WindowStart.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture),
            snapshot.WindowEnd.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture),
            "plugin");
    }

    private async Task RefreshSourceAsync(
        string sourceName,
        ServiceConfiguration configuration,
        string path,
        Func<JsonNode?, IReadOnlyList<CalendarEntry>> parse,
        CancellationToken cancellationToken)
    {
        if (!configuration.Enabled)
        {
            _cache.MarkFailed(sourceName, false, "disabled");
            return;
        }

        try
        {
            var response = await _http.SendAsync(
                sourceName,
                configuration,
                HttpMethod.Get,
                path,
                null,
                null,
                cancellationToken).ConfigureAwait(false);
            var refreshedAt = DateTimeOffset.UtcNow;
            _cache.ReplaceSource(
                sourceName,
                parse(response),
                CalendarStart,
                CalendarEnd,
                refreshedAt);
        }
        catch (GatewayException exception)
        {
            _health.Failure(sourceName, exception.Message);
            _cache.MarkFailed(sourceName, true, exception.Message);
        }
    }

    internal static IReadOnlyList<CalendarEntry> ParseSonarr(JsonNode? root)
    {
        if (root is not JsonArray episodes)
        {
            return Array.Empty<CalendarEntry>();
        }

        var result = new List<CalendarEntry>(episodes.Count);
        foreach (var episode in episodes.OfType<JsonObject>())
        {
            var date = DateValue(episode, "airDate", "airDateUtc");
            if (date is null)
            {
                continue;
            }

            var series = episode["series"] as JsonObject;
            result.Add(new CalendarEntry(
                "episode",
                date,
                "air",
                StringValue(episode, "title") ?? "Untitled episode",
                StringValue(series, "title"),
                IntValue(episode, "seasonNumber"),
                IntValue(episode, "episodeNumber"),
                IntValue(episode, "tmdbId"),
                IntValue(episode, "tvdbId"),
                BoolValue(episode, "monitored", true),
                BoolValue(episode, "hasFile", false),
                null,
                SeriesTmdbId: IntValue(series, "tmdbId"),
                SeriesTvdbId: IntValue(series, "tvdbId")));
        }

        return result;
    }

    internal static IReadOnlyList<CalendarEntry> ParseRadarr(JsonNode? root)
    {
        if (root is not JsonArray movies)
        {
            return Array.Empty<CalendarEntry>();
        }

        var result = new List<CalendarEntry>(movies.Count * 2);
        foreach (var movie in movies
            .OfType<JsonObject>()
            .Where(static movie => BoolValue(movie, "monitored", true)))
        {
            foreach (var (property, kind) in new[]
            {
                ("inCinemas", "cinema"),
                ("digitalRelease", "digital"),
                ("physicalRelease", "physical")
            })
            {
                var date = DateValue(movie, property);
                if (date is null)
                {
                    continue;
                }

                result.Add(new CalendarEntry(
                    "movie",
                    date,
                    kind,
                    StringValue(movie, "title") ?? "Untitled movie",
                    null,
                    null,
                    null,
                    IntValue(movie, "tmdbId"),
                    IntValue(movie, "tvdbId"),
                    BoolValue(movie, "monitored", true),
                    BoolValue(movie, "hasFile", false),
                    null));
            }
        }

        return result;
    }

    private static string? DateValue(JsonObject? value, params string[] names)
    {
        foreach (var name in names)
        {
            var raw = StringValue(value, name);
            if (raw is not null && DateTimeOffset.TryParse(
                raw,
                CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeUniversal,
                out var parsed))
            {
                return parsed.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
            }
        }

        return null;
    }

    internal static string? StringValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<string>(out var parsed)
            ? parsed
            : null;

    internal static int? IntValue(JsonObject? value, string name)
        => value?[name] is JsonValue node && node.TryGetValue<int>(out var parsed)
            ? parsed
            : null;

    internal static bool BoolValue(JsonObject? value, string name, bool fallback)
        => value?[name] is JsonValue node && node.TryGetValue<bool>(out var parsed)
            ? parsed
            : fallback;
}
