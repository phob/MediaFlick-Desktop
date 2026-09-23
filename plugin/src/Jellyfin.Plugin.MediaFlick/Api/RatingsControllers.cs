using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Common.Api;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Api;

[ApiController]
[Authorize]
[Route("MediaFlick/ratings/v1")]
public sealed class RatingsController : ControllerBase
{
    private readonly RatingsService _ratings;

    public RatingsController(RatingsService ratings)
    {
        _ratings = ratings;
    }

    [HttpPost("batch")]
    [ProducesResponseType<RatingBatchResponse>(StatusCodes.Status200OK)]
    public async Task<IActionResult> Batch(
        [FromBody] RatingBatchRequest? request,
        CancellationToken cancellationToken)
    {
        try
        {
            var response = await _ratings.BatchAsync(request, cancellationToken).ConfigureAwait(false);
            return new JsonResult(response, CompanionJson.CamelCase);
        }
        catch (RatingContractVersionException exception)
        {
            return Conflict(new
            {
                error = exception.Message,
                requestedBoundary = exception.RequestedVersion,
                supportedBoundary = new
                {
                    min = RatingsContract.BoundaryVersion,
                    max = RatingsContract.BoundaryVersion
                }
            });
        }
        catch (RatingRequestException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (RatingsUnavailableException exception)
        {
            return StatusCode(
                StatusCodes.Status503ServiceUnavailable,
                new { error = exception.Message });
        }
    }
}

/// <summary>
/// Administrator-only secret lifecycle. Jellyfin dashboard requests carry the
/// normal token header rather than ambient cookie authentication, so mutating
/// POST/PUT/DELETE methods plus RequiresElevation provide the host's normal
/// CSRF-safe plugin configuration model.
/// </summary>
[ApiController]
[Authorize(Policy = Policies.RequiresElevation)]
[Route("MediaFlick/admin/ratings")]
public sealed class ProviderCredentialsController : ControllerBase
{
    private const string StorageUnavailableMessage =
        "MediaFlick Companion secure credential storage is unavailable for {Provider}";
    private readonly RatingsService _ratings;
    private readonly CollectionProviderService _collections;
    private readonly ILogger<ProviderCredentialsController> _logger;

    public ProviderCredentialsController(
        RatingsService ratings,
        CollectionProviderService collections,
        ILogger<ProviderCredentialsController> logger)
    {
        _ratings = ratings;
        _collections = collections;
        _logger = logger;
    }

    [HttpGet]
    public ActionResult<RatingAdminStatusResponse> Status()
        => new JsonResult(_ratings.AdminStatus(), CompanionJson.CamelCase);

    [HttpPut("{provider}")]
    public async Task<IActionResult> Save(
        string provider,
        [FromBody] ProviderSecretUpdate? update,
        CancellationToken cancellationToken)
    {
        if (update is null)
        {
            return BadRequest(new { error = "a JSON request body is required" });
        }

        try
        {
            var response = await _ratings.SaveCredentialAsync(
                provider,
                update.ApiKey ?? string.Empty,
                cancellationToken).ConfigureAwait(false);
            return new JsonResult(response, CompanionJson.CamelCase);
        }
        catch (ArgumentException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (RatingRequestException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (InvalidOperationException exception)
        {
            // Storage failures carry fixed plugin text, never the key.
            _logger.LogWarning(exception, StorageUnavailableMessage, provider);
            return StatusCode(
                StatusCodes.Status503ServiceUnavailable,
                new { error = "secure plugin configuration storage is unavailable" });
        }
    }

    [HttpPost("{provider}/validate")]
    public async Task<IActionResult> Validate(
        string provider,
        CancellationToken cancellationToken)
    {
        try
        {
            var response = await _ratings.ValidateCredentialAsync(
                provider,
                cancellationToken).ConfigureAwait(false);
            return new JsonResult(response, CompanionJson.CamelCase);
        }
        catch (ArgumentException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (RatingRequestException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (InvalidOperationException exception)
        {
            _logger.LogWarning(exception, StorageUnavailableMessage, provider);
            return StatusCode(
                StatusCodes.Status503ServiceUnavailable,
                new { error = "secure plugin configuration storage is unavailable" });
        }
    }

    [HttpDelete("{provider}")]
    public IActionResult Remove(string provider)
    {
        try
        {
            var response = _ratings.RemoveCredential(provider);
            _collections.ClearProviderCache(provider);
            return new JsonResult(response, CompanionJson.CamelCase);
        }
        catch (ArgumentException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (InvalidOperationException exception)
        {
            _logger.LogWarning(exception, StorageUnavailableMessage, provider);
            return StatusCode(
                StatusCodes.Status503ServiceUnavailable,
                new { error = "secure plugin configuration storage is unavailable" });
        }
    }
}
