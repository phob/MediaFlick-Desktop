using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CalendarAndSeerrTests
{
    [Fact]
    public void SonarrEpisodesNormalizeIntoTheSharedCalendarShape()
    {
        var source = JsonNode.Parse(
            """
            [{
              "title":"The We We Are","seasonNumber":1,"episodeNumber":4,
              "airDateUtc":"2026-08-02T01:00:00Z","tvdbId":1234,
              "monitored":true,"hasFile":false,
              "series":{"title":"Severance","tvdbId":371980}
            }]
            """);

        var entry = Assert.Single(CalendarService.ParseSonarr(source));
        Assert.Equal("episode", entry.Kind);
        Assert.Equal("2026-08-02", entry.Date);
        Assert.Equal("air", entry.DateKind);
        Assert.Equal("Severance", entry.SeriesTitle);
        Assert.Equal(1, entry.Season);
        Assert.Equal(4, entry.Episode);
        Assert.Equal(1234, entry.TvdbId);
        Assert.Equal(371980, entry.SeriesTvdbId);
        Assert.False(entry.HasFile);
    }

    [Fact]
    public void RadarrMoviesProduceOneEntryPerKnownReleaseDate()
    {
        var source = JsonNode.Parse(
            """
            [{
              "title":"Mickey 17","tmdbId":696506,"monitored":true,"hasFile":true,
              "inCinemas":"2026-03-07T00:00:00Z",
              "digitalRelease":"2026-04-01T00:00:00Z",
              "physicalRelease":null
            },{
              "title":"Unmonitored","tmdbId":1,"monitored":false,"hasFile":false,
              "digitalRelease":"2026-04-02T00:00:00Z"
            }]
            """);

        var entries = CalendarService.ParseRadarr(source);
        Assert.Equal(2, entries.Count);
        Assert.Equal(["cinema", "digital"], entries.Select(static entry => entry.DateKind));
        Assert.All(entries, static entry => Assert.Equal(696506, entry.TmdbId));
    }

    [Theory]
    [InlineData("https://image.tmdb.org/t/p/original/ve72VxNqjGM69Uky4WTo2bK6rfq.jpg", "/ve72VxNqjGM69Uky4WTo2bK6rfq.jpg")]
    [InlineData("http://image.tmdb.org/t/p/w500/abc_1-2.png", "/abc_1-2.png")]
    [InlineData("https://artworks.thetvdb.com/banners/posters/73871-2.jpg", null)]
    [InlineData("http://radarr:7878/MediaCover/1/poster.jpg", null)]
    [InlineData("/MediaCover/1/poster.jpg", null)]
    [InlineData("not a url", null)]
    [InlineData("", null)]
    [InlineData("https://image.tmdb.org/t/p/original/nested/abc.jpg", null)]
    [InlineData("https://image.tmdb.org/t/p/original/abc.jpg?api_key=secret", null)]
    [InlineData("https://image.tmdb.org/t/p/original/..", null)]
    [InlineData("https://image.tmdb.org/t/p/original/a%20b.jpg", null)]
    [InlineData("https://image.tmdb.org/abc.jpg", null)]
    [InlineData("https://image.tmdb.org.evil.example/t/p/original/abc.jpg", null)]
    public void CalendarPostersAreExposedOnlyAsTmdbArtworkPaths(string remoteUrl, string? expected)
    {
        var source = new JsonArray(new JsonObject
        {
            ["title"] = "Poster",
            ["tmdbId"] = 1,
            ["monitored"] = true,
            ["hasFile"] = false,
            ["digitalRelease"] = "2026-04-01T00:00:00Z",
            ["images"] = new JsonArray(
                new JsonObject { ["coverType"] = "fanart", ["remoteUrl"] = "https://image.tmdb.org/t/p/original/fanart.jpg" },
                new JsonObject { ["coverType"] = "poster", ["url"] = "/MediaCover/1/poster.jpg", ["remoteUrl"] = remoteUrl })
        });

        Assert.Equal(expected, Assert.Single(CalendarService.ParseRadarr(source)).PosterPath);
    }

    [Fact]
    public void CalendarEntriesWithoutAUsablePosterHaveNoPosterPath()
    {
        var radarr = JsonNode.Parse(
            """
            [
              {"title":"No images","tmdbId":1,"monitored":true,"digitalRelease":"2026-04-01T00:00:00Z"},
              {"title":"Null images","tmdbId":2,"monitored":true,"digitalRelease":"2026-04-01T00:00:00Z","images":null},
              {"title":"Object images","tmdbId":3,"monitored":true,"digitalRelease":"2026-04-01T00:00:00Z","images":{"coverType":"poster"}},
              {"title":"Fanart only","tmdbId":4,"monitored":true,"digitalRelease":"2026-04-01T00:00:00Z",
               "images":[1,"poster",{"coverType":"fanart","remoteUrl":"https://image.tmdb.org/t/p/original/fanart.jpg"}]},
              {"title":"Poster without remote","tmdbId":5,"monitored":true,"digitalRelease":"2026-04-01T00:00:00Z",
               "images":[{"coverType":"poster","url":"/MediaCover/5/poster.jpg"}]}
            ]
            """);
        var sonarr = JsonNode.Parse(
            """
            [{
              "title":"Pilot","seasonNumber":1,"episodeNumber":1,"airDate":"2026-08-02","monitored":true,
              "series":{"title":"Futurama","tvdbId":73871,
                "images":[{"coverType":"poster","remoteUrl":"https://artworks.thetvdb.com/banners/posters/73871-2.jpg"}]}
            }]
            """);

        Assert.All(CalendarService.ParseRadarr(radarr), static entry => Assert.Null(entry.PosterPath));
        Assert.Null(Assert.Single(CalendarService.ParseSonarr(sonarr)).PosterPath);
    }

    [Fact]
    public void CompleteCalendarSnapshotsAreCachedForOneDay()
    {
        var attemptedAt = new DateTimeOffset(2026, 8, 24, 12, 0, 0, TimeSpan.Zero);
        var cache = new CalendarCache();
        cache.MarkRefreshAttempt(attemptedAt);
        cache.ReplaceSource(
            "radarr",
            [],
            CalendarService.CalendarStart,
            CalendarService.CalendarEnd,
            attemptedAt);

        Assert.True(CalendarService.IsFresh(cache.Snapshot(), attemptedAt.AddHours(23)));
        Assert.False(CalendarService.IsFresh(cache.Snapshot(), attemptedAt.AddHours(24)));

        cache.MarkFailed("radarr", true, "unreachable");
        Assert.True(CalendarService.IsFresh(cache.Snapshot(), attemptedAt.AddMinutes(14)));
        Assert.False(CalendarService.IsFresh(cache.Snapshot(), attemptedAt.AddMinutes(15)));
    }

    [Fact]
    public void AFailedRefreshKeepsLastGoodEntriesAndMarksTheSourceStale()
    {
        var cache = new CalendarCache();
        var today = new DateOnly(2026, 8, 2);
        var entry = new CalendarEntry(
            "movie",
            "2026-08-02",
            "digital",
            "Film",
            null,
            null,
            null,
            1,
            null,
            true,
            false,
            null);
        cache.ReplaceSource("radarr", [entry], today, today.AddDays(60), DateTimeOffset.UtcNow);
        cache.MarkFailed("radarr", true, "unreachable");

        var snapshot = cache.Snapshot();
        Assert.Single(snapshot.BySource["radarr"]);
        Assert.True(snapshot.Sources["radarr"].Stale);
        Assert.False(snapshot.Sources["radarr"].Available);
    }

    [Fact]
    public void SeerrSearchDropsPeopleAndShapesRequestableMedia()
    {
        var source = JsonNode.Parse(
            """
            {
              "page":1,"totalPages":2,"totalResults":3,
              "results":[
                {"id":603,"mediaType":"movie","title":"The Matrix",
                 "releaseDate":"1999-03-31","mediaInfo":{"status":5,"status4k":1}},
                {"id":7,"mediaType":"person","name":"Someone"}
              ]
            }
            """);

        var page = SeerrGateway.ShapeSearchPage(source);
        var movie = Assert.Single(page.Results);
        Assert.Equal("movie", movie.MediaType);
        Assert.Equal(603, movie.TmdbId);
        Assert.Equal(1999, movie.Year);
        Assert.Equal("available", movie.Status);
        Assert.Equal(2, page.TotalPages);
    }

    [Fact]
    public void SeerrPersonCreditsKeepCastOnlyDeduplicateAndPreserveRequestState()
    {
        var source = JsonNode.Parse(
            """
            {
              "id":6384,
              "cast":[
                {"id":603,"mediaType":"movie","title":"The Matrix","character":"Neo",
                 "adult":false},
                {"id":603,"mediaType":"movie","title":"The Matrix","character":"Thomas",
                 "adult":false,"mediaInfo":{"status":5}},
                {"id":603,"mediaType":"tv","name":"Different namespace","adult":false},
                {"id":7,"mediaType":"movie","title":"Thanks","character":" thanks ","adult":false},
                {"id":8,"mediaType":"movie","title":"Adult","adult":true}
              ],
              "crew":[{"id":9,"mediaType":"movie","title":"Directed"}]
            }
            """);

        var page = SeerrGateway.ShapePersonCredits(source);
        Assert.Equal(2, page.Results.Count);
        Assert.Equal("movie", page.Results[0].MediaType);
        Assert.Equal("available", page.Results[0].Status);
        Assert.Equal("tv", page.Results[1].MediaType);
        Assert.Equal(2, page.TotalResults);
    }

    [Fact]
    public void SeerrGenresDropMalformedRowsAndKeepBackdropChoices()
    {
        var source = JsonNode.Parse(
            """
            [
              {"id":18,"name":"Drama","backdrops":["/one.jpg","/two.jpg"]},
              {"id":0,"name":"Broken","backdrops":[]},
              {"id":35,"name":"","backdrops":[]}
            ]
            """);

        var drama = Assert.Single(SeerrGateway.ShapeGenres(source));
        Assert.Equal(18, drama.Id);
        Assert.Equal("Drama", drama.Name);
        Assert.Equal(["/one.jpg", "/two.jpg"], drama.Backdrops);
    }

    [Fact]
    public void SeerrDiscoveryPathsPreserveTheAllowlistedDesktopFilters()
    {
        var today = new DateOnly(2026, 8, 1);
        var movies = SeerrGateway.BuildDiscoverPath(
            "movies", 4, 18, "vote_average.desc", 7, 1990, null, null, today);
        Assert.StartsWith("api/v1/discover/movies?", movies, StringComparison.Ordinal);
        Assert.All(
            ["page=4", "genre=18", "primaryReleaseDateGte=1990-01-01", "primaryReleaseDateLte=1999-12-31", "sortBy=vote_average.desc", "voteCountGte=50", "voteAverageGte=7"],
            part => Assert.Contains(part, movies, StringComparison.Ordinal));
        var trending = SeerrGateway.BuildDiscoverPath(
            "trending", -1, null, null, null, null, "tv", "week", today);
        Assert.StartsWith("api/v1/discover/trending?", trending, StringComparison.Ordinal);
        Assert.All(
            ["page=1", "mediaType=tv", "timeWindow=week"],
            part => Assert.Contains(part, trending, StringComparison.Ordinal));
        Assert.Equal(
            "api/v1/discover/tv/upcoming?page=2",
            SeerrGateway.BuildDiscoverPath(
                "upcoming-tv", 2, null, null, null, null, null, null, today));
        var tv = SeerrGateway.BuildDiscoverPath(
            "tv", 1, 18, null, null, 2020, null, null, today);
        Assert.StartsWith("api/v1/discover/tv?", tv, StringComparison.Ordinal);
        Assert.All(
            ["page=1", "genre=18", "firstAirDateGte=2020-01-01", "firstAirDateLte=2026-08-01"],
            part => Assert.Contains(part, tv, StringComparison.Ordinal));
        var earliestDecade = SeerrGateway.BuildDiscoverPath(
            "movies", 1, null, "popularity.desc", null, 1900, null, null, today);
        Assert.StartsWith("api/v1/discover/movies?", earliestDecade, StringComparison.Ordinal);
        Assert.All(
            ["page=1", "primaryReleaseDateGte=1900-01-01", "primaryReleaseDateLte=1909-12-31", "sortBy=popularity.desc"],
            part => Assert.Contains(part, earliestDecade, StringComparison.Ordinal));
        Assert.Throws<GatewayException>(() =>
            SeerrGateway.BuildDiscoverPath(
                "movies", 1, null, null, null, 1995, null, null, today));
        Assert.Throws<GatewayException>(() =>
            SeerrGateway.BuildDiscoverPath(
                "movies", 1, null, null, null, 1890, null, null, today));
        Assert.Throws<GatewayException>(() =>
            SeerrGateway.BuildDiscoverPath(
                "movies", 1, null, null, null, 2030, null, null, today));
    }

    [Fact]
    public void SeerrMediaKeepsRichDetailAndRejectsUnsafeTrailerKeys()
    {
        var source = Assert.IsType<JsonObject>(JsonNode.Parse(
            """
            {
              "id":603,"title":"The Matrix",
              "imdbId":"not-an-imdb-id",
              "externalIds":{"imdbId":"tt0133093","tvdbId":-1},
              "credits":{"crew":[
                {"id":1,"name":"Lana Wachowski","job":"Director","department":"Directing"},
                {"id":2,"name":"Screenwriter","job":"Screenplay","department":"Writing"}
              ]},
              "relatedVideos":[
                {"site":"YouTube","type":"Trailer","key":"abcdefghijk","name":"Official Trailer","size":1080},
                {"site":"YouTube","type":"Trailer","key":"not/a/key","name":"Unsafe","size":2160}
              ],
              "releases":{"results":[{"iso_3166_1":"US","release_dates":[
                {"type":4,"release_date":"1999-09-21T00:00:00.000Z"}
              ]}]}
            }
            """));

        var detail = SeerrGateway.ShapeMedia(source, "movie");
        Assert.Equal(["Lana Wachowski"], detail.Directors);
        Assert.Equal("tt0133093", detail.ExternalIds.Imdb);
        Assert.Null(detail.ExternalIds.Tvdb);
        Assert.Equal("abcdefghijk", detail.Trailer?.Key);
        Assert.Equal("digital", detail.ReleaseDates[0].Type);

        var seriesSource = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"id":95396,"name":"Severance","externalIds":{"imdbId":"tt11280740","tvdbId":371980}}"""));
        var series = SeerrGateway.ShapeMedia(seriesSource, "tv");
        Assert.Equal("tt11280740", series.ExternalIds.Imdb);
        Assert.Equal(371980, series.ExternalIds.Tvdb);
    }

    [Fact]
    public void SeerrRequestDestinationsMarkTheActiveQualityProfile()
    {
        var server = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"id":0,"name":"Movies","isDefault":true,"activeProfileId":2}"""));
        var detail = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"profiles":[{"id":2,"name":"HD-1080p"},{"id":1,"name":"Any"}]}"""));

        var destination = SeerrGateway.ShapeRequestDestination(server, detail);
        Assert.True(destination.Profiles[1].IsDefault);
    }
}
