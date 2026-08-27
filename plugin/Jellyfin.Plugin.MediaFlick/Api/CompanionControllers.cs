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
    private readonly CollectionProviderService _collections;

    public InfoController(
        ServiceHealthStore health,
        RatingsService ratings,
        CollectionProviderService collections)
    {
        _health = health;
        _ratings = ratings;
        _collections = collections;
    }

    [HttpGet("info")]
    [ProducesResponseType<PluginInfoResponse>(StatusCodes.Status200OK)]
    public async Task<ActionResult<PluginInfoResponse>> GetInfo(
        CancellationToken cancellationToken)
    {
        await _collections.RefreshReadinessAsync(cancellationToken).ConfigureAwait(false);
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
                ["mdblist"] = _collections.MdbListReady,
                ["tmdb"] = _collections.TmdbReady
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
        var capabilities = new List<string> { "collection-experience-v1" };
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
    public async Task<ActionResult<CalendarResponse>> GetCalendar(
        [FromQuery] DateOnly? start,
        [FromQuery] DateOnly? end,
        CancellationToken cancellationToken)
    {
        try
        {
            var response = await _calendar.GetAsync(start, end, cancellationToken)
                .ConfigureAwait(false);
            return new JsonResult(response, CompanionJson.CamelCase);
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
[Route("MediaFlick/collection-experience/v1")]
public sealed class CollectionExperienceController : ControllerBase
{
    private readonly CollectionProviderService _collections;

    public CollectionExperienceController(CollectionProviderService collections)
    {
        _collections = collections;
    }

    [HttpPost("preview")]
    public Task<IActionResult> Preview(
        [FromBody] CollectionProviderRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.PreviewAsync(request, cancellationToken));

    [HttpPost("results")]
    public Task<IActionResult> Results(
        [FromBody] CollectionProviderRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.ResultsAsync(request, cancellationToken));

    [HttpPost("franchises")]
    public Task<IActionResult> Franchises(
        [FromBody] FranchiseResolveRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.FranchisesAsync(request, cancellationToken));

    [HttpPost("mdblist/search")]
    public Task<IActionResult> SearchPublicLists(
        [FromBody] PublicListSearchRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.SearchPublicListsAsync(request, cancellationToken));

    [HttpPost("mdblist/validate")]
    public Task<IActionResult> ValidatePublicList(
        [FromBody] PublicListSelectorRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.ValidatePublicListAsync(request, cancellationToken));

    [HttpPost("identities")]
    public Task<IActionResult> ResolveIdentities(
        [FromBody] IdentityResolveRequest request,
        CancellationToken cancellationToken)
        => RunAsync(() => _collections.ResolveIdentitiesAsync(request, cancellationToken));

    [HttpGet("artwork")]
    public async Task<IActionResult> Artwork(
        [FromQuery] string size,
        [FromQuery] string path,
        CancellationToken cancellationToken)
    {
        try
        {
            var response = await _collections.ArtworkAsync(size, path, cancellationToken)
                .ConfigureAwait(false);
            return File(response.Body, response.ContentType);
        }
        catch (GatewayException exception)
        {
            return StatusCode(exception.StatusCode, new { error = exception.Message });
        }
    }

    private async Task<IActionResult> RunAsync<T>(Func<Task<T>> action)
    {
        try
        {
            return new JsonResult(
                await action().ConfigureAwait(false),
                CompanionJson.CamelCase);
        }
        catch (GatewayException exception)
        {
            return StatusCode(exception.StatusCode, new { error = exception.Message });
        }
    }
}

[ApiController]
[Authorize(Policy = Policies.RequiresElevation)]
[Route("MediaFlick/admin")]
public sealed class AdminController : ControllerBase
{
    private readonly CompanionHttpClient _http;

    public AdminController(CompanionHttpClient http)
    {
        _http = http;
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
            autoImportSeerrUsers = configuration.AutoImportSeerrUsers
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
            ProtectedMdbListApiKey = current.ProtectedMdbListApiKey,
            ProtectedTmdbApiKey = current.ProtectedTmdbApiKey
        });
        // The config page reads the response as JSON, and an empty 204 body
        // would make that read fail after a successful save.
        return Ok(new { saved = true });
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
