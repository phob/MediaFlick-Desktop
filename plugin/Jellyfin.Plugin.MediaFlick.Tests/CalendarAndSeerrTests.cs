using System.Text.Json;
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
            }]
            """);

        var entries = CalendarService.ParseRadarr(source);
        Assert.Equal(2, entries.Count);
        Assert.Equal(["cinema", "digital"], entries.Select(static entry => entry.DateKind));
        Assert.All(entries, static entry => Assert.Equal(696506, entry.TmdbId));
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
    public void TypedResponsesSerializeCamelCaseRegardlessOfHostDefaults()
    {
        // Jellyfin's MVC pipeline serializes PascalCase; the MediaFlick
        // contract is camelCase, so the controllers pass these options
        // explicitly and this test pins the wire shape.
        var info = new PluginInfoResponse(
            "0.1.0",
            1,
            ["calendar"],
            new Dictionary<string, bool> { ["sonarr"] = true });
        var json = JsonSerializer.Serialize(info, CompanionJson.CamelCase);
        Assert.Contains("\"pluginVersion\":\"0.1.0\"", json);
        Assert.Contains("\"apiVersion\":1", json);
        Assert.Contains("\"sonarr\":true", json);

        var calendar = new CalendarResponse(
            [new CalendarEntry("movie", "2026-08-02", "digital", "Film", null, null, null, 1, null, true, false, null)],
            null,
            new Dictionary<string, SourceStatus>
            {
                ["radarr"] = new(true, true, false, null, null)
            },
            "2026-07-26",
            "2026-10-01",
            "plugin");
        json = JsonSerializer.Serialize(calendar, CompanionJson.CamelCase);
        Assert.Contains("\"dateKind\":\"digital\"", json);
        Assert.Contains("\"windowStart\":\"2026-07-26\"", json);
        Assert.Contains("\"radarr\":", json);
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
        var results = Assert.IsType<JsonArray>(page["results"]);
        var movie = Assert.IsType<JsonObject>(Assert.Single(results));
        Assert.Equal("movie", movie["mediaType"]?.GetValue<string>());
        Assert.Equal(603, movie["tmdbId"]?.GetValue<int>());
        Assert.Equal("available", movie["status"]?.GetValue<string>());
        Assert.Null(movie["libraryItemId"]);
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

        var genres = Assert.IsType<JsonArray>(SeerrGateway.ShapeGenres(source));
        var drama = Assert.IsType<JsonObject>(Assert.Single(genres));
        Assert.Equal(18, drama["id"]?.GetValue<int>());
        Assert.Equal("Drama", drama["name"]?.GetValue<string>());
        Assert.Equal(2, Assert.IsType<JsonArray>(drama["backdrops"]).Count);
    }

    [Fact]
    public void SeerrDiscoveryPathsPreserveTheAllowlistedDesktopFilters()
    {
        Assert.Equal(
            "api/v1/discover/movies?page=4&genre=18&sortBy=vote_average.desc&voteCountGte=50&voteAverageGte=7",
            SeerrGateway.BuildDiscoverPath("movies", 4, 18, "vote_average.desc", 7, null, null));
        Assert.Equal(
            "api/v1/discover/trending?page=1&mediaType=tv&timeWindow=week",
            SeerrGateway.BuildDiscoverPath("trending", -1, null, null, null, "tv", "week"));
        Assert.Equal(
            "api/v1/discover/tv/upcoming?page=2",
            SeerrGateway.BuildDiscoverPath("upcoming-tv", 2, null, null, null, null, null));
    }

    [Fact]
    public void SeerrMediaKeepsRichDetailAndRejectsUnsafeTrailerKeys()
    {
        var source = Assert.IsType<JsonObject>(JsonNode.Parse(
            """
            {
              "id":603,"title":"The Matrix","releaseDate":"1999-03-30",
              "overview":"A hacker discovers the truth.","status":"Released",
              "voteAverage":8.2,"voteCount":26000,
              "productionCompanies":[{"id":79,"name":"Village Roadshow Pictures"}],
              "credits":{
                "cast":[{"id":6384,"name":"Keanu Reeves","character":"Neo"}],
                "crew":[{"id":1,"name":"Lana Wachowski","job":"Director","department":"Directing"}]
              },
              "relatedVideos":[
                {"site":"YouTube","type":"Trailer","key":"abcdefghijk","name":"Official Trailer","size":1080},
                {"site":"YouTube","type":"Trailer","key":"not/a/key","name":"Unsafe","size":2160}
              ],
              "releases":{"results":[{"iso_3166_1":"US","release_dates":[
                {"type":4,"release_date":"1999-09-21T00:00:00.000Z","certification":"R"}
              ]}]},
              "mediaInfo":{"status":5,"status4k":1}
            }
            """));

        var detail = Assert.IsType<JsonObject>(SeerrGateway.ShapeMedia(source, "movie"));
        Assert.Equal("A hacker discovers the truth.", detail["overview"]?.GetValue<string>());
        Assert.Equal("Released", detail["productionStatus"]?.GetValue<string>());
        Assert.Equal("Lana Wachowski", detail["directors"]?[0]?.GetValue<string>());
        Assert.Equal("Neo", detail["cast"]?[0]?["character"]?.GetValue<string>());
        Assert.Equal("abcdefghijk", detail["trailer"]?["key"]?.GetValue<string>());
        Assert.Equal("digital", detail["releaseDates"]?[0]?["type"]?.GetValue<string>());
        Assert.Equal("R", detail["releaseDates"]?[0]?["certification"]?.GetValue<string>());
    }

    [Fact]
    public void SeerrRequestDestinationsMarkAndSortTheActiveQualityProfile()
    {
        var server = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"id":0,"name":"Movies","isDefault":true,"activeProfileId":2}"""));
        var detail = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"profiles":[{"id":2,"name":"HD-1080p"},{"id":1,"name":"Any"}]}"""));

        var destination = Assert.IsType<JsonObject>(
            SeerrGateway.ShapeRequestDestination(server, detail));
        Assert.Equal(0, destination["id"]?.GetValue<int>());
        Assert.Equal("Movies", destination["name"]?.GetValue<string>());
        Assert.Equal("Any", destination["profiles"]?[0]?["name"]?.GetValue<string>());
        Assert.True(destination["profiles"]?[1]?["isDefault"]?.GetValue<bool>());
    }
}
