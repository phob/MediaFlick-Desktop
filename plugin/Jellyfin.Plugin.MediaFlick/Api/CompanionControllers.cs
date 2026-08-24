using System.Reflection;
using System.Security.Claims;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Common.Api;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace Jellyfin.Plugin.MediaFlick.Api;

[ApiController]
[Authorize]
[Route("MediaFlick")]
public sealed class InfoController : ControllerBase
{
    private readonly ServiceHealthStore _health;
    private readonly RatingsService _ratings;

    public InfoController(ServiceHealthStore health, RatingsService ratings)
    {
        _health = health;
        _ratings = ratings;
    }

    [HttpGet("info")]
    [ProducesResponseType<PluginInfoResponse>(StatusCodes.Status200OK)]
    public ActionResult<PluginInfoResponse> GetInfo()
    {
        var configuration = Plugin.Instance?.Configuration ?? new PluginConfiguration();
        var ratings = _ratings.Capability();
        var capabilities = Capabilities(configuration, ratings.Available);

        var version = typeof(Plugin).Assembly.GetName().Version?.ToString(3) ?? "0.2.0";
        var info = new PluginInfoResponse(
            version,
            1,
            capabilities,
            new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase)
            {
                ["sonarr"] = IsConfigured(configuration.Sonarr) && _health.IsHealthy("sonarr"),
                ["radarr"] = IsConfigured(configuration.Radarr) && _health.IsHealthy("radarr"),
                ["seerr"] = IsConfigured(configuration.Seerr) && _health.IsHealthy("seerr"),
                ["mdblist"] = ratings.Available,
                ["tmdb"] = ratings.Tmdb.Configured && ratings.Tmdb.Valid
            },
            ratings);
        return new JsonResult(info, CompanionJson.CamelCase);
    }

    internal static IReadOnlyList<string> Capabilities(
        PluginConfiguration configuration,
        bool ratingsAvailable)
    {
        // This names a versioned contract implemented by this plugin build.
        // Definitions and service configuration can change while Desktop is
        // signed in, so they must not make the capability itself disappear.
        var capabilities = new List<string> { "collections-curated-v1" };
        if (IsConfigured(configuration.Sonarr) || IsConfigured(configuration.Radarr))
        {
            capabilities.Add("calendar");
        }

        if (IsConfigured(configuration.Seerr))
        {
            capabilities.Add("seerr");
            capabilities.Add("seerr-person-discovery");
            capabilities.Add("seerr-discovery-v2");
            capabilities.Add("seerr-discovery-v4");
            capabilities.Add("seerr-request-profiles");
            capabilities.Add("collections-v1");
            if (configuration.NativeCollections)
            {
                capabilities.Add("collections-v2");
            }
        }

        if (ratingsAvailable)
        {
            capabilities.Add("ratings-v1");
        }

        return capabilities;
    }

    private static bool IsConfigured(ServiceConfiguration service)
        => service.Enabled
            && !string.IsNullOrWhiteSpace(service.ApiKey)
            && Uri.TryCreate(service.BaseUrl, UriKind.Absolute, out var uri)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps);
}

[ApiController]
[Authorize]
[Route("MediaFlick/calendar")]
public sealed class CalendarController : ControllerBase
{
    private readonly CalendarService _calendar;

    public CalendarController(CalendarService calendar)
    {
        _calendar = calendar;
    }

    [HttpGet]
    [ProducesResponseType<CalendarResponse>(StatusCodes.Status200OK)]
    public ActionResult<CalendarResponse> GetCalendar(
        [FromQuery] DateOnly? start,
        [FromQuery] DateOnly? end)
    {
        try
        {
            return new JsonResult(_calendar.Get(start, end), CompanionJson.CamelCase);
        }
        catch (GatewayException exception)
        {
            return StatusCode(exception.StatusCode, new { error = exception.Message });
        }
    }
}

[ApiController]
[Authorize]
[Route("MediaFlick/seerr")]
public sealed class SeerrController : ControllerBase
{
    private readonly SeerrGateway _gateway;

    public SeerrController(SeerrGateway gateway)
    {
        _gateway = gateway;
    }

    [HttpGet("status")]
    public Task<IActionResult> Status(CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.StatusAsync(userId, cancellationToken));

    [HttpGet("search")]
    public Task<IActionResult> Search(
        [FromQuery] string query,
        [FromQuery] int page = 1,
        CancellationToken cancellationToken = default)
        => RunAsync(userId => _gateway.SearchAsync(userId, query, page, cancellationToken));

    [HttpGet("person/{tmdbId:int}/credits")]
    public Task<IActionResult> PersonCredits(
        int tmdbId,
        CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.PersonCreditsAsync(
            userId,
            tmdbId,
            cancellationToken));

    [HttpGet("discover/{kind}")]
    public Task<IActionResult> Discover(
        string kind,
        [FromQuery] int page = 1,
        [FromQuery] int? genre = null,
        [FromQuery] string? sortBy = null,
        [FromQuery] int? voteAverageGte = null,
        [FromQuery] int? releaseDecade = null,
        [FromQuery] string? mediaType = null,
        [FromQuery] string? timeWindow = null,
        CancellationToken cancellationToken = default)
        => RunAsync(userId => _gateway.DiscoverAsync(
            userId,
            kind,
            page,
            genre,
            sortBy,
            voteAverageGte,
            releaseDecade,
            mediaType,
            timeWindow,
            cancellationToken));

    [HttpGet("genres/{mediaType}")]
    public Task<IActionResult> Genres(string mediaType, CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.GenresAsync(userId, mediaType, cancellationToken));

    [HttpGet("media/{mediaType}/{tmdbId:int}")]
    public Task<IActionResult> Media(
        string mediaType,
        int tmdbId,
        CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.MediaAsync(
            userId,
            mediaType,
            tmdbId,
            cancellationToken));

    [HttpGet("request-options/{mediaType}")]
    public Task<IActionResult> RequestOptions(
        string mediaType,
        [FromQuery] bool is4k = false,
        CancellationToken cancellationToken = default)
        => RunAsync(userId => _gateway.RequestOptionsAsync(
            userId,
            mediaType,
            is4k,
            cancellationToken));

    [HttpPost("request")]
    public Task<IActionResult> RequestMedia(
        [FromBody] SeerrRequestBody body,
        CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.RequestAsync(userId, body, cancellationToken));

    [HttpGet("requests")]
    public Task<IActionResult> Requests(
        [FromQuery] int take = 40,
        [FromQuery] int skip = 0,
        [FromQuery] string filter = "all",
        CancellationToken cancellationToken = default)
        => RunAsync(userId => _gateway.RequestsAsync(
            userId,
            take,
            skip,
            filter,
            cancellationToken));

    [HttpDelete("request/{requestId:int}")]
    public Task<IActionResult> Cancel(int requestId, CancellationToken cancellationToken)
        => RunAsync(userId => _gateway.CancelAsync(userId, requestId, cancellationToken));

    private async Task<IActionResult> RunAsync(Func<Guid, Task<JsonNode>> action)
    {
        var userId = CurrentUserId(User);
        if (userId == Guid.Empty)
        {
            return Unauthorized(new { error = "the Jellyfin user identity is missing" });
        }

        try
        {
            return new JsonResult(await action(userId).ConfigureAwait(false));
        }
        catch (GatewayException exception)
        {
            // Authentication already succeeded at Jellyfin's middleware. A
            // 401 from here is Seerr's overloaded permission response, not a
            // reason for the desktop to discard its Jellyfin session.
            var status = exception.StatusCode == StatusCodes.Status401Unauthorized
                ? StatusCodes.Status403Forbidden
                : exception.StatusCode;
            return StatusCode(status, new { error = exception.Message });
        }
    }

    private static Guid CurrentUserId(ClaimsPrincipal principal)
    {
        var value = principal.Claims.FirstOrDefault(claim =>
            claim.Type.Equals("Jellyfin-UserId", StringComparison.OrdinalIgnoreCase))?.Value;
        return Guid.TryParse(value, out var parsed) ? parsed : Guid.Empty;
    }
}

[ApiController]
[Authorize]
[Route("MediaFlick/collections")]
public sealed class CollectionsController : ControllerBase
{
    private readonly CollectionsService _collections;
    private readonly SeerrGateway _seerr;
    private readonly CuratedCollectionResolver _curated;

    public CollectionsController(
        CollectionsService collections,
        SeerrGateway seerr,
        CuratedCollectionResolver curated)
    {
        _collections = collections;
        _seerr = seerr;
        _curated = curated;
    }

    [HttpGet]
    public Task<IActionResult> List(CancellationToken cancellationToken)
        => RunAsync(userId => _collections.SummaryAsync(userId, cancellationToken));

    /// <summary>The collection one TMDB movie belongs to, for detail-page links.</summary>
    [HttpGet("movie/{tmdbId:int}")]
    public Task<IActionResult> ForMovie(int tmdbId, CancellationToken cancellationToken)
        => RunAsync(userId => _collections.ForMovieAsync(userId, tmdbId, cancellationToken));

    [HttpGet("{collectionId:int}")]
    public Task<IActionResult> Detail(int collectionId, CancellationToken cancellationToken)
        => RunAsync(userId => _collections.DetailAsync(userId, collectionId, cancellationToken));

    /// <summary>
    /// One administrator-defined curated collection's parts. Definition order
    /// is the curation, so it is preserved rather than re-sorted.
    /// </summary>
    [HttpGet("curated/{definitionId}")]
    public async Task<IActionResult> Curated(string definitionId, CancellationToken cancellationToken)
    {
        var definition = (Plugin.Instance?.Configuration ?? new PluginConfiguration())
            .CuratedCollections
            .FirstOrDefault(def => def.Id == definitionId);
        if (definition is null)
        {
            return NotFound(new { error = "that curated collection does not exist" });
        }

        return await RunAsync(async userId =>
        {
            var items = await _curated.ResolveAsync(
                definition.TmdbIds,
                definition.MdbListSource,
                cancellationToken).ConfigureAwait(false);
            return await _seerr.CuratedCollectionAsync(
                userId,
                definition.Id,
                definition.Name,
                items,
                cancellationToken).ConfigureAwait(false);
        }).ConfigureAwait(false);
    }

    private async Task<IActionResult> RunAsync(Func<Guid, Task<JsonNode>> action)
    {
        var userId = CurrentUserId(User);
        if (userId == Guid.Empty)
        {
            return Unauthorized(new { error = "the Jellyfin user identity is missing" });
        }

        try
        {
            return new JsonResult(await action(userId).ConfigureAwait(false));
        }
        catch (GatewayException exception)
        {
            // Same posture as the Seerr controller: a 401 from Seerr is its
            // overloaded permission response, never a reason for the desktop
            // to discard its Jellyfin session.
            var status = exception.StatusCode == StatusCodes.Status401Unauthorized
                ? StatusCodes.Status403Forbidden
                : exception.StatusCode;
            return StatusCode(status, new { error = exception.Message });
        }
    }

    private static Guid CurrentUserId(ClaimsPrincipal principal)
    {
        var value = principal.Claims.FirstOrDefault(claim =>
            claim.Type.Equals("Jellyfin-UserId", StringComparison.OrdinalIgnoreCase))?.Value;
        return Guid.TryParse(value, out var parsed) ? parsed : Guid.Empty;
    }
}

[ApiController]
[Authorize(Policy = Policies.RequiresElevation)]
[Route("MediaFlick/admin")]
public sealed class AdminController : ControllerBase
{
    private readonly CompanionHttpClient _http;
    private readonly CuratedCollectionResolver _curated;

    public AdminController(CompanionHttpClient http, CuratedCollectionResolver curated)
    {
        _http = http;
        _curated = curated;
    }

    [HttpGet("config")]
    public IActionResult GetConfiguration()
    {
        var configuration = Plugin.Instance?.Configuration ?? new PluginConfiguration();
        return Ok(new
        {
            sonarr = Redact(configuration.Sonarr),
            radarr = Redact(configuration.Radarr),
            seerr = Redact(configuration.Seerr),
            autoImportSeerrUsers = configuration.AutoImportSeerrUsers,
            nativeCollections = configuration.NativeCollections,
            curatedCollections = configuration.CuratedCollections.Select(def => new
            {
                id = def.Id,
                name = def.Name,
                tmdbIds = def.TmdbIds,
                mdbListSource = def.MdbListSource
            }).ToArray()
        });
    }

    [HttpPost("config")]
    public IActionResult UpdateConfiguration([FromBody] CompanionConfigurationUpdate update)
    {
        var plugin = Plugin.Instance;
        if (plugin is null)
        {
            return StatusCode(StatusCodes.Status503ServiceUnavailable, new { error = "plugin unavailable" });
        }

        plugin.MutateConfiguration(current => new PluginConfiguration
        {
            Sonarr = Merge(current.Sonarr, update.Sonarr),
            Radarr = Merge(current.Radarr, update.Radarr),
            Seerr = Merge(current.Seerr, update.Seerr),
            AutoImportSeerrUsers = update.AutoImportSeerrUsers,
            NativeCollections = update.NativeCollections,
            CuratedCollections = update.CuratedCollections
                .Where(def => !string.IsNullOrWhiteSpace(def.Name))
                .Select(def => new CuratedCollectionDefinition
                {
                    Id = string.IsNullOrWhiteSpace(def.Id) ? Guid.NewGuid().ToString("N") : def.Id,
                    Name = def.Name.Trim(),
                    TmdbIds = def.TmdbIds,
                    MdbListSource = def.MdbListSource?.Trim() ?? string.Empty
                })
                .ToList(),
            ProtectedMdbListApiKey = current.ProtectedMdbListApiKey,
            ProtectedTmdbApiKey = current.ProtectedTmdbApiKey
        });
        // The config page reads the response as JSON, and an empty 204 body
        // would make that read fail after a successful save.
        return Ok(new { saved = true });
    }

    /// <summary>
    /// Resolves one MDBList source reference without saving anything, so the
    /// dashboard can prove a list exists and report its movie and series
    /// counts before the administrator commits the definition.
    /// </summary>
    [HttpPost("test/curated-source")]
    public async Task<IActionResult> TestCuratedSource(
        [FromBody] CuratedSourceTestRequest request,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(request.Source))
        {
            return Ok(new { ok = false, error = "enter an MDBList source first" });
        }

        try
        {
            var items = await _curated.ResolveAsync(string.Empty, request.Source, cancellationToken)
                .ConfigureAwait(false);
            return Ok(new
            {
                ok = true,
                items = items.Count,
                movies = items.Count(item => item.Kind == CuratedMediaKind.Movie),
                series = items.Count(item => item.Kind == CuratedMediaKind.Series)
            });
        }
        catch (GatewayException exception)
        {
            return Ok(new { ok = false, error = exception.Message });
        }
    }

    [HttpPost("test/{serviceName}")]
    public async Task<IActionResult> TestConnection(
        string serviceName,
        [FromBody] ServiceConfigurationUpdate? candidate,
        CancellationToken cancellationToken)
    {
        var configuration = Plugin.Instance?.Configuration ?? new PluginConfiguration();
        var service = serviceName.ToLowerInvariant() switch
        {
            "sonarr" => configuration.Sonarr,
            "radarr" => configuration.Radarr,
            "seerr" => configuration.Seerr,
            _ => null
        };
        if (service is null)
        {
            return NotFound(new { error = "unknown service" });
        }
        if (candidate is not null)
        {
            service = Merge(service, candidate);
        }

        try
        {
            var version = await _http.TestAsync(
                serviceName.ToLowerInvariant(),
                service,
                cancellationToken).ConfigureAwait(false);
            return new JsonResult(
                new ConnectionTestResponse(serviceName.ToLowerInvariant(), true, version),
                CompanionJson.CamelCase);
        }
        catch (GatewayException exception)
        {
            return StatusCode(exception.StatusCode, new { error = exception.Message });
        }
    }

    private static object Redact(ServiceConfiguration service)
        => new
        {
            // Named explicitly: projected members would serialize with the
            // host's PascalCase and the config page reads camelCase.
            enabled = service.Enabled,
            baseUrl = service.BaseUrl,
            hasApiKey = !string.IsNullOrWhiteSpace(service.ApiKey)
        };

    private static ServiceConfiguration Merge(
        ServiceConfiguration previous,
        ServiceConfigurationUpdate update)
        => new()
        {
            Enabled = update.Enabled,
            BaseUrl = update.BaseUrl.Trim().TrimEnd('/'),
            ApiKey = string.IsNullOrWhiteSpace(update.ApiKey)
                ? previous.ApiKey
                : update.ApiKey.Trim()
        };
}
