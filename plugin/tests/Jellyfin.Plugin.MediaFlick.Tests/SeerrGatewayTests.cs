using System.Collections.Concurrent;
using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class SeerrGatewayTests
{
    private static readonly Guid Mapped = Guid.Parse("7f1c6f0e-8b1a-4b44-9ad6-0b8b6d1f7a01");
    private static readonly Guid Unmapped = Guid.Parse("11111111-2222-3333-4444-555555555555");
    private const int SeerrUserId = 12;

    private readonly FakeSeerr _seerr = new();
    private readonly ManualTime _time = new(new DateTimeOffset(2026, 9, 23, 12, 0, 0, TimeSpan.Zero));
    private readonly SeerrGateway _gateway;

    public SeerrGatewayTests()
    {
        _gateway = new SeerrGateway(_seerr, NullLogger<SeerrGateway>.Instance, _time);
    }

    [Fact]
    public async Task StatusIsTypedAndNeverCarriesTheSeerrAddress()
    {
        _seerr.Handle("api/v1/auth/me", _ => JsonNode.Parse(
            """{"displayName":"Alice","avatar":"/avatar.png","permissions":32}"""));
        _seerr.Handle($"api/v1/user/{SeerrUserId}/quota", _ => JsonNode.Parse(
            """
            {"movie":{"days":7,"limit":10,"used":3,"remaining":7,"restricted":false},
             "tv":{"days":null,"limit":null,"used":0,"remaining":null,"restricted":false},
             "internal":"http://seerr.internal:5055"}
            """));
        _seerr.Handle("api/v1/settings/public", _ => JsonNode.Parse(
            """{"movie4kEnabled":true,"series4kEnabled":false,"partialRequestsEnabled":true,"applicationUrl":"http://seerr.internal:5055"}"""));

        var status = await _gateway.StatusAsync(Mapped, TestContext.Current.CancellationToken);
        var json = JsonNode.Parse(JsonSerializer.Serialize(status, CompanionJson.CamelCase))!.AsObject();

        Assert.DoesNotContain("seerr.internal", json.ToJsonString(), StringComparison.Ordinal);
        Assert.True(json["instance"]!["movie4kEnabled"]!.GetValue<bool>());
        Assert.True(json["instance"]!["partialRequestsEnabled"]!.GetValue<bool>());
        Assert.Equal("Alice", json["user"]!["name"]!.GetValue<string>());
        Assert.Equal(Mapped.ToString("N"), json["user"]!["jellyfinUserId"]!.GetValue<string>());
        Assert.True(json["capabilities"]!["movie"]!["request"]!.GetValue<bool>());
        Assert.False(json["capabilities"]!["movie4k"]!["request"]!.GetValue<bool>());
        Assert.False(json["capabilities"]!["advancedRequest"]!.GetValue<bool>());
        Assert.Equal(3, json["quota"]!["movie"]!["used"]!.GetValue<int>());
        Assert.Null(json["quota"]!["tv"]!["limit"]);
        Assert.All(
            _seerr.Calls.Where(call => !call.Path.StartsWith("api/v1/user?", StringComparison.Ordinal)),
            call => Assert.Equal(SeerrUserId, call.SeerrUserId));
    }

    [Fact]
    public async Task AnUnavailableQuotaStillReturnsPermissions()
    {
        _seerr.Handle("api/v1/auth/me", _ => JsonNode.Parse("""{"username":"bob","permissions":2}"""));
        _seerr.Handle($"api/v1/user/{SeerrUserId}/quota", _ => throw new GatewayException(
            StatusCodes.Status503ServiceUnavailable,
            "Seerr is unavailable (HTTP 503)"));
        _seerr.Handle("api/v1/settings/public", _ => new JsonObject());

        var status = await _gateway.StatusAsync(Mapped, TestContext.Current.CancellationToken);

        Assert.Null(status.Quota);
        Assert.Equal("bob", status.User.Name);
        Assert.True(status.Capabilities.AdvancedRequest);
    }

    [Fact]
    public async Task MappingsAreCachedAndMissingUsersAreNotRescannedOnEveryRequest()
    {
        _seerr.Handle("api/v1/search", _ => JsonNode.Parse("""{"results":[]}"""));

        await _gateway.SearchAsync(Mapped, "matrix", 1, TestContext.Current.CancellationToken);
        await _gateway.SearchAsync(Mapped, "matrix", 2, TestContext.Current.CancellationToken);
        Assert.Equal(1, _seerr.UserScans);

        for (var attempt = 0; attempt < 3; attempt++)
        {
            var error = await Assert.ThrowsAsync<GatewayException>(() =>
                _gateway.SearchAsync(Unmapped, "matrix", 1, TestContext.Current.CancellationToken));
            Assert.Equal(StatusCodes.Status409Conflict, error.StatusCode);
        }
        Assert.Equal(2, _seerr.UserScans);

        _time.Advance(SeerrGateway.MissingUserLifetime);
        _seerr.Users.Add(new JsonObject { ["id"] = 99, ["jellyfinUserId"] = Unmapped.ToString("N") });
        await _gateway.SearchAsync(Unmapped, "matrix", 1, TestContext.Current.CancellationToken);
        Assert.Equal(3, _seerr.UserScans);
        Assert.Equal(99, _seerr.Calls.Last().SeerrUserId);
    }

    [Fact]
    public async Task AutoImportRunsOncePerMissingWindow()
    {
        _seerr.AutoImportUsers = true;

        await Assert.ThrowsAsync<GatewayException>(() =>
            _gateway.SearchAsync(Unmapped, "matrix", 1, TestContext.Current.CancellationToken));
        await Assert.ThrowsAsync<GatewayException>(() =>
            _gateway.SearchAsync(Unmapped, "matrix", 1, TestContext.Current.CancellationToken));

        var import = Assert.Single(_seerr.Calls, call => call.Path == "api/v1/user/import-from-jellyfin");
        Assert.Null(import.SeerrUserId);
        Assert.Equal(Unmapped.ToString("N"), import.Body?["jellyfinUserIds"]?[0]?.GetValue<string>());
    }

    [Fact]
    public async Task ConcurrentFirstRequestsShareOneUserScan()
    {
        _seerr.Handle("api/v1/search", _ => JsonNode.Parse("""{"results":[]}"""));
        _seerr.UserScanDelay = TimeSpan.FromMilliseconds(50);

        await Task.WhenAll(Enumerable.Range(0, 5).Select(_ =>
            _gateway.SearchAsync(Mapped, "matrix", 1, TestContext.Current.CancellationToken)));

        Assert.Equal(1, _seerr.UserScans);
    }

    [Fact]
    public async Task RequestsAreSentAsTheMappedUserWithANormalizedBody()
    {
        _seerr.Handle("api/v1/request", call => call.Method == HttpMethod.Post
            ? JsonNode.Parse(
                """{"id":5,"status":1,"type":"tv","is4k":false,"media":{"tmdbId":1396,"status":3},"seasons":[{"seasonNumber":1},{"seasonNumber":2}]}""")
            : JsonNode.Parse("""{"pageInfo":{"page":1,"pages":1,"results":1},"results":[{"id":5,"status":2,"media":{"mediaType":"movie","tmdbId":603,"status":5}}]}"""));

        var created = await _gateway.RequestAsync(
            Mapped,
            new SeerrRequestBody("tv", 1396, [2, 1, 2], false, null, null),
            TestContext.Current.CancellationToken);
        var page = await _gateway.RequestsAsync(Mapped, 500, -3, "PENDING", TestContext.Current.CancellationToken);

        var post = Assert.Single(_seerr.Calls, call => call.Method == HttpMethod.Post);
        Assert.Equal(SeerrUserId, post.SeerrUserId);
        Assert.True(
            JsonNode.DeepEquals(
                JsonNode.Parse("""{"mediaType":"tv","mediaId":1396,"is4k":false,"seasons":[1,2]}"""),
                post.Body),
            post.Body?.ToJsonString());
        Assert.Equal("pending", created.Status);
        Assert.Equal("tv", created.MediaType);
        Assert.Equal("processing", created.MediaStatus);
        Assert.Equal([1, 2], created.Seasons);
        Assert.Contains(
            _seerr.Calls,
            call => call.Path.StartsWith("api/v1/request?", StringComparison.Ordinal)
                && new[] { "take=100", "skip=0", "filter=pending", $"requestedBy={SeerrUserId}" }
                    .All(part => call.Path.Contains(part, StringComparison.Ordinal)));
        var listed = Assert.Single(page.Results);
        Assert.Equal("approved", listed.Status);
        Assert.Equal("available", listed.MediaStatus);
        Assert.Equal(603, listed.TmdbId);
    }

    [Fact]
    public async Task QualityProfilesRequireTheAdvancedRequestPermission()
    {
        _seerr.Handle("api/v1/auth/me", _ => JsonNode.Parse("""{"permissions":32}"""));

        var error = await Assert.ThrowsAsync<GatewayException>(() => _gateway.RequestOptionsAsync(
            Mapped,
            "movie",
            false,
            TestContext.Current.CancellationToken));

        Assert.Equal(StatusCodes.Status403Forbidden, error.StatusCode);
        Assert.DoesNotContain(_seerr.Calls, call => call.Path.StartsWith("api/v1/service", StringComparison.Ordinal));
    }

    [Fact]
    public void MalformedQuotaAndMediaFieldsAreDroppedInsteadOfRelayed()
    {
        Assert.Null(SeerrGateway.ShapeQuota(JsonNode.Parse("""{"movie":"lots","tv":{}}""")));
        var detail = SeerrGateway.ShapeMedia(
            Assert.IsType<JsonObject>(JsonNode.Parse(
                """{"id":1,"title":"Big","overview":{"html":"<b>x</b>"},"revenue":2923706026,"inProduction":"no"}""")),
            "movie");

        Assert.Null(detail.Overview);
        Assert.Equal(2_923_706_026L, detail.Revenue);
        Assert.Null(detail.InProduction);
    }

    [Fact]
    public async Task TheCompanionTransportUsesTheConfiguredSeerrServiceOnly()
    {
        var handler = new StubHandler
        {
            Respond = request =>
            {
                Assert.Equal("http://seerr.internal.example:5055/api/v1/status", request.RequestUri?.ToString());
                Assert.Equal("7", request.Headers.GetValues("X-API-User").Single());
                return StubHandler.Json(HttpStatusCode.OK, """{"version":"2.0.0"}""");
            }
        };
        var configuration = new PluginConfiguration
        {
            Seerr = new ServiceConfiguration
            {
                Enabled = true,
                BaseUrl = "http://seerr.internal.example:5055",
                ApiKey = "seerr-key"
            }
        };
        var transport = new CompanionSeerrTransport(
            new CompanionHttpClient(
                new StubFactory(handler),
                new ServiceHealthStore(),
                NullLogger<CompanionHttpClient>.Instance),
            () => configuration);

        var response = await transport.SendAsync(
            HttpMethod.Get,
            "api/v1/status",
            null,
            7,
            TestContext.Current.CancellationToken);

        Assert.Equal("2.0.0", response?["version"]?.GetValue<string>());

        var uninitialized = new CompanionSeerrTransport(
            new CompanionHttpClient(
                new StubFactory(handler),
                new ServiceHealthStore(),
                NullLogger<CompanionHttpClient>.Instance),
            () => null);
        var error = await Assert.ThrowsAsync<GatewayException>(() => uninitialized.SendAsync(
            HttpMethod.Get,
            "api/v1/status",
            null,
            null,
            TestContext.Current.CancellationToken));
        Assert.Equal(StatusCodes.Status503ServiceUnavailable, error.StatusCode);
    }

    [Fact]
    public async Task CompanySearchIsBoundedEscapedAndTyped()
    {
        _seerr.Handle("api/v1/search/company", _ => JsonNode.Parse(
            """
            {"page":2,"totalPages":3,"totalResults":41,"results":[
              {"id":420,"name":" Marvel Studios ","logo_path":"/hUzeosd33nzE5MCNsZxCGEKTXaQ.png","origin_country":"US"},
              {"id":0,"name":"Zero"},
              {"id":7,"name":"  "},
              {"name":"No id"},
              {"id":19551,"name":"Marvel Enterprises","internal":"http://seerr.internal:5055"}
            ]}
            """));

        var page = await _gateway.CompaniesAsync(
            Mapped,
            " marvel&page=9/../#x ",
            2,
            TestContext.Current.CancellationToken);

        var call = Assert.Single(_seerr.Calls, call => call.Path.StartsWith("api/v1/search/company?", StringComparison.Ordinal));
        Assert.Equal("api/v1/search/company?query=marvel%26page%3D9%2F..%2F%23x&page=2", call.Path);
        Assert.Equal(SeerrUserId, call.SeerrUserId);
        Assert.Equal(["420:Marvel Studios", "19551:Marvel Enterprises"], page.Results.Select(company => $"{company.Id}:{company.Name}"));
        Assert.Equal((2, 3, 41), (page.Page, page.TotalPages, page.TotalResults));
        var json = JsonSerializer.Serialize(page, CompanionJson.CamelCase);
        Assert.DoesNotContain("logo", json, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("seerr.internal", json, StringComparison.Ordinal);
        Assert.DoesNotContain("origin", json, StringComparison.OrdinalIgnoreCase);
    }

    [Theory]
    [InlineData("", 1)]
    [InlineData("   ", 1)]
    [InlineData("marvel", 0)]
    [InlineData("marvel", 501)]
    public async Task InvalidCompanySearchesNeverReachSeerr(string query, int page)
    {
        var error = await Assert.ThrowsAsync<GatewayException>(() =>
            _gateway.CompaniesAsync(Mapped, query, page, TestContext.Current.CancellationToken));

        Assert.Equal(StatusCodes.Status400BadRequest, error.StatusCode);
        Assert.Empty(_seerr.Calls);
    }

    [Fact]
    public async Task OverlongCompanySearchesNeverReachSeerr()
    {
        var error = await Assert.ThrowsAsync<GatewayException>(() => _gateway.CompaniesAsync(
            Mapped,
            new string('a', 101),
            1,
            TestContext.Current.CancellationToken));

        Assert.Equal(StatusCodes.Status400BadRequest, error.StatusCode);
        Assert.Empty(_seerr.Calls);
    }

    [Fact]
    public void TheNetworkCatalogHasUniquePositiveIdsAndNames()
    {
        Assert.InRange(SeerrNetworks.All.Count, 20, 40);
        Assert.All(SeerrNetworks.All, network =>
        {
            Assert.True(network.Id > 0);
            Assert.False(string.IsNullOrWhiteSpace(network.Name));
        });
        Assert.Equal(SeerrNetworks.All.Count, SeerrNetworks.All.Select(network => network.Id).Distinct().Count());
        Assert.Contains(SeerrNetworks.All, network => network is { Id: 213, Name: "Netflix" });
    }

    [Fact]
    public void DetailKeepsStudioAndNetworkNamesAndAddsTheirIds()
    {
        var detail = SeerrGateway.ShapeMedia(
            Assert.IsType<JsonObject>(JsonNode.Parse(
                """
                {"id":1399,"name":"Game of Thrones",
                 "productionCompanies":[{"id":76043,"name":"Revolution Sun Studios","logoPath":"/x.png"},{"id":0,"name":"Zero"},{"id":5,"name":" "},{"name":"No id"}],
                 "networks":[{"id":49,"name":"HBO","originCountry":"US"}]}
                """)),
            "tv");

        Assert.Equal(["Revolution Sun Studios", "Zero", "No id"], detail.Studios);
        Assert.Equal(["HBO"], detail.Networks);
        Assert.Equal(["76043:Revolution Sun Studios"], detail.StudioRefs.Select(studio => $"{studio.Id}:{studio.Name}"));
        Assert.Equal(["49:HBO"], detail.NetworkRefs.Select(network => $"{network.Id}:{network.Name}"));
    }

    private sealed record SeerrCall(HttpMethod Method, string Path, JsonNode? Body, int? SeerrUserId);

    private sealed class FakeSeerr : ISeerrTransport
    {
        private readonly ConcurrentQueue<SeerrCall> _calls = new();
        private readonly Dictionary<string, Func<SeerrCall, JsonNode?>> _handlers =
            new(StringComparer.Ordinal);
        private int _userScans;

        public bool AutoImportUsers { get; set; }

        public List<JsonObject> Users { get; } =
        [
            new JsonObject { ["id"] = 3, ["jellyfinUserId"] = null },
            new JsonObject { ["id"] = SeerrUserId, ["jellyfinUserId"] = Mapped.ToString("N") }
        ];

        public TimeSpan UserScanDelay { get; set; }

        public int UserScans => Volatile.Read(ref _userScans);

        public IReadOnlyList<SeerrCall> Calls => _calls.ToArray();

        public void Handle(string path, Func<SeerrCall, JsonNode?> handler) => _handlers[path] = handler;

        public async Task<JsonNode?> SendAsync(
            HttpMethod method,
            string path,
            JsonNode? body,
            int? seerrUserId,
            CancellationToken cancellationToken)
        {
            var call = new SeerrCall(method, path, body?.DeepClone(), seerrUserId);
            _calls.Enqueue(call);
            if (path.StartsWith("api/v1/user?", StringComparison.Ordinal))
            {
                if (path.Contains("skip=0&", StringComparison.Ordinal))
                {
                    Interlocked.Increment(ref _userScans);
                }
                if (UserScanDelay > TimeSpan.Zero)
                {
                    await Task.Delay(UserScanDelay, cancellationToken);
                }
                return new JsonObject
                {
                    ["results"] = new JsonArray(Users.Select(user => (JsonNode)user.DeepClone()).ToArray())
                };
            }
            var route = path.Split('?')[0];
            return _handlers.TryGetValue(route, out var handler)
                ? handler(call)
                : new JsonObject();
        }
    }
}
