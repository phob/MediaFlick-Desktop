using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Common.Api;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

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
public sealed class RatingsAdminController : ControllerBase
{
    private readonly RatingsService _ratings;

    public RatingsAdminController(RatingsService ratings)
    {
        _ratings = ratings;
    }

    [HttpGet]
    public ActionResult<RatingAdminStatusResponse> Status()
        => new JsonResult(_ratings.AdminStatus(), CompanionJson.CamelCase);

    [HttpPut("{provider}")]
    public async Task<IActionResult> Save(
        string provider,
        [FromBody] RatingSecretUpdate? update,
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
        catch (InvalidOperationException)
        {
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
        catch (InvalidOperationException)
        {
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
            return new JsonResult(
                _ratings.RemoveCredential(provider),
                CompanionJson.CamelCase);
        }
        catch (ArgumentException exception)
        {
            return BadRequest(new { error = exception.Message });
        }
        catch (InvalidOperationException)
        {
            return StatusCode(
                StatusCodes.Status503ServiceUnavailable,
                new { error = "secure plugin configuration storage is unavailable" });
        }
    }
}
