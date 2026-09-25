using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>What Radarr knows about one movie.</summary>
internal sealed record RadarrMovieFacts(string? Status, bool IsAvailable, bool HasFile);

/// <summary>
/// Sonarr's monitored-episode counts for a series or one season.
/// <see cref="Aired"/> counts monitored episodes that have aired or already
/// have a file, which is Sonarr's own <c>episodeCount</c>.
/// </summary>
internal sealed record SonarrEpisodeFacts(int Aired, int Files, bool MoreScheduled);

internal sealed record SonarrSeriesFacts(
    SonarrEpisodeFacts Series,
    IReadOnlyDictionary<int, SonarrEpisodeFacts> Seasons);

/// <summary>The Radarr and Sonarr answers gathered for one Seerr response.</summary>
internal sealed record ArrFacts(
    IReadOnlyDictionary<int, RadarrMovieFacts> MoviesByTmdb,
    IReadOnlyDictionary<int, SonarrSeriesFacts> SeriesByTvdb)
{
    public static ArrFacts None { get; } = new(
        new Dictionary<int, RadarrMovieFacts>(),
        new Dictionary<int, SonarrSeriesFacts>());
}

/// <summary>
/// Explains a Seerr <c>processing</c> title. Seerr uses that one state for
/// everything between an approved request and an imported file, so on its own
/// it cannot tell a download from a movie that is still in cinemas. The live
/// download queue Seerr attaches to its media rows wins; otherwise Radarr's
/// release state or Sonarr's episode counts say why nothing is downloading.
/// Null means none of those facts is available.
/// </summary>
internal static class MediaActivity
{
    public const string Downloading = "downloading";
    public const string Searching = "searching";
    public const string InCinemas = "in-cinemas";
    public const string Unreleased = "unreleased";
    public const string AwaitingEpisodes = "awaiting-episodes";

    /// <summary>Seerr's MediaStatus.PROCESSING.</summary>
    public const int ProcessingStatus = 3;

    /// <summary>
    /// Whether an arr lookup could add anything: the title, or one of its
    /// seasons, is processing without Seerr's queue already showing a download.
    /// </summary>
    public static bool NeedsFacts(JsonObject? mediaInfo)
        => Waiting(mediaInfo, false)
            || Waiting(mediaInfo, true)
            || ((mediaInfo?["seasons"] as JsonArray)?
                .OfType<JsonObject>()
                .Any(static season => JsonRead.Int32(season, "status") == ProcessingStatus
                    || JsonRead.Int32(season, "status4k") == ProcessingStatus)
                ?? false);

    private static bool Waiting(JsonObject? mediaInfo, bool is4k)
        => JsonRead.Int32(mediaInfo, is4k ? "status4k" : "status") == ProcessingStatus
            && !IsDownloading(mediaInfo, is4k, null);

    public static string? Resolve(
        string mediaType,
        JsonObject? mediaInfo,
        int? seerrStatus,
        bool is4k,
        int? seasonNumber,
        ArrFacts facts)
    {
        if (seerrStatus != ProcessingStatus)
        {
            return null;
        }

        if (IsDownloading(mediaInfo, is4k, seasonNumber))
        {
            return Downloading;
        }

        if (mediaType == "movie")
        {
            return JsonRead.Int32(mediaInfo, "tmdbId") is { } tmdbId
                && facts.MoviesByTmdb.TryGetValue(tmdbId, out var movie)
                    ? ForMovie(movie, is4k)
                    : null;
        }

        if (JsonRead.Int32(mediaInfo, "tvdbId") is not { } tvdbId
            || !facts.SeriesByTvdb.TryGetValue(tvdbId, out var series))
        {
            return null;
        }

        var episodes = seasonNumber is { } number
            ? series.Seasons.GetValueOrDefault(number)
            : series.Series;
        return episodes is null ? null : ForEpisodes(episodes, is4k);
    }

    private static string? ForMovie(RadarrMovieFacts movie, bool is4k)
    {
        // A file Seerr has not synced yet is neither searching nor
        // unreleased. The Companion's Radarr is the regular-quality one, so
        // its file says nothing about a 4K request.
        if (movie.HasFile && !is4k)
        {
            return null;
        }

        if (movie.IsAvailable)
        {
            return Searching;
        }

        return movie.Status switch
        {
            "inCinemas" => InCinemas,
            "announced" or "tba" => Unreleased,
            _ => null
        };
    }

    private static string? ForEpisodes(SonarrEpisodeFacts episodes, bool is4k)
    {
        if (episodes.Aired == 0)
        {
            return Unreleased;
        }

        // File counts come from the regular-quality Sonarr.
        if (is4k)
        {
            return null;
        }

        if (episodes.Files < episodes.Aired)
        {
            return Searching;
        }

        return episodes.MoreScheduled ? AwaitingEpisodes : null;
    }

    private static bool IsDownloading(JsonObject? mediaInfo, bool is4k, int? seasonNumber)
        => (mediaInfo?[is4k ? "downloadStatus4k" : "downloadStatus"] as JsonArray)?
            .OfType<JsonObject>()
            .Any(item => seasonNumber is null
                || JsonRead.Int32(item["episode"] as JsonObject, "seasonNumber") == seasonNumber)
            ?? false;
}
