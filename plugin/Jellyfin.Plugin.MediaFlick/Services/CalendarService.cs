using System.Globalization;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CalendarService
{
    private readonly CompanionHttpClient _http;
    private readonly CalendarCache _cache;
    private readonly ServiceHealthStore _health;

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
        var plugin = Plugin.Instance
            ?? throw new InvalidOperationException("The MediaFlick plugin is not initialized");
        var configuration = plugin.Configuration;
        var today = DateOnly.FromDateTime(DateTime.UtcNow);
        var start = today.AddDays(-7);
        var end = today.AddDays(60);
        var completed = 0;

        await RefreshSourceAsync(
            "sonarr",
            configuration.Sonarr,
            start,
            end,
            ParseSonarr,
            cancellationToken).ConfigureAwait(false);
        progress?.Report(++completed * 50);

        await RefreshSourceAsync(
            "radarr",
            configuration.Radarr,
            start,
            end,
            ParseRadarr,
            cancellationToken).ConfigureAwait(false);
        progress?.Report(++completed * 50);
    }

    public CalendarResponse Get(DateOnly? requestedStart, DateOnly? requestedEnd)
    {
        var snapshot = _cache.Snapshot();
        var start = requestedStart ?? snapshot.WindowStart;
        var end = requestedEnd ?? snapshot.WindowEnd;
        if (end < start)
        {
            throw new GatewayException(StatusCodes.Status400BadRequest, "end must not precede start");
        }

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
        DateOnly start,
        DateOnly end,
        Func<JsonNode?, IReadOnlyList<CalendarEntry>> parse,
        CancellationToken cancellationToken)
    {
        if (!configuration.Enabled)
        {
            _cache.MarkFailed(sourceName, false, "disabled");
            return;
        }

        var query = string.Create(
            CultureInfo.InvariantCulture,
            $"api/v3/calendar?start={start:yyyy-MM-dd}&end={end:yyyy-MM-dd}&includeSeries=true");
        try
        {
            var response = await _http.SendAsync(
                sourceName,
                configuration,
                HttpMethod.Get,
                query,
                null,
                null,
                cancellationToken).ConfigureAwait(false);
            var refreshedAt = DateTimeOffset.UtcNow;
            _cache.ReplaceSource(sourceName, parse(response), start, end, refreshedAt);
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
                null));
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
        foreach (var movie in movies.OfType<JsonObject>())
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
