using System.Globalization;
using System.IO.Compression;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed record MdbListResponse(
    HttpStatusCode StatusCode,
    JsonNode? Body,
    RatingQuotaResponse Quota,
    long? RetryAt,
    bool HasMore = false);

internal interface IMdbListTransport
{
    Task<MdbListResponse> ValidateAsync(string apiKey, CancellationToken cancellationToken);

    Task<MdbListResponse> BatchAsync(
        string apiKey,
        string provider,
        string mediaType,
        IReadOnlyList<string> ids,
        CancellationToken cancellationToken);

    Task<MdbListResponse> ListItemsAsync(
        string apiKey,
        string resource,
        CancellationToken cancellationToken);
}

/// <summary>
/// Fixed-origin MDBList transport. It deliberately does not use
/// IHttpClientFactory: MDBList API-key authentication is a query parameter and
/// the factory's normal request logging could otherwise record the full URI.
/// Diagnostics log only status codes and error categories, never the request
/// URI, the exception text, or the response body.
/// </summary>
internal sealed class MdbListHttpTransport : IMdbListTransport, IDisposable
{
    private const int MaxResponseBytes = 8 * 1024 * 1024;
    private const string Subject = "mdblist";
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(20);
    private readonly HttpClient _client;
    private readonly ILogger<MdbListHttpTransport> _logger;
    private readonly FailureLogGate _failures = new();

    public MdbListHttpTransport(ILogger<MdbListHttpTransport> logger)
    {
        _logger = logger;
        var handler = new SocketsHttpHandler
        {
            AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.All,
            ConnectTimeout = TimeSpan.FromSeconds(8),
            PooledConnectionLifetime = TimeSpan.FromMinutes(10)
        };
        _client = new HttpClient(handler, true)
        {
            BaseAddress = new Uri("https://api.mdblist.com/", UriKind.Absolute),
            Timeout = Timeout.InfiniteTimeSpan
        };
        _client.DefaultRequestHeaders.Accept.ParseAdd("application/json");
        _client.DefaultRequestHeaders.UserAgent.ParseAdd("MediaFlick-Companion/0.2");
    }

    public Task<MdbListResponse> ValidateAsync(
        string apiKey,
        CancellationToken cancellationToken)
        => SendAsync(
            HttpMethod.Get,
            "user?apikey=" + Uri.EscapeDataString(apiKey),
            null,
            true,
            cancellationToken);

    public Task<MdbListResponse> BatchAsync(
        string apiKey,
        string provider,
        string mediaType,
        IReadOnlyList<string> ids,
        CancellationToken cancellationToken)
    {
        // provider and mediaType have already passed strict allowlists. No
        // caller can select a host, path, port, or arbitrary upstream query.
        var path = provider + "/" + mediaType + "/?apikey=" + Uri.EscapeDataString(apiKey);
        var bodyIds = ids.Select(id => provider == "tmdb"
            && long.TryParse(id, NumberStyles.None, CultureInfo.InvariantCulture, out var number)
                ? JsonValue.Create(number)
                : JsonValue.Create(id)).ToArray();
        return SendAsync(
            HttpMethod.Post,
            path,
            new JsonObject { ["ids"] = new JsonArray(bodyIds) },
            true,
            cancellationToken);
    }

    public void Dispose() => _client.Dispose();

    /// <summary>
    /// Fetches one MDBList list's items. `resource` is an already-validated
    /// validated lists-relative path built by the collection provider; no caller
    /// can select a host, path, port, or arbitrary upstream query.
    /// </summary>
    public Task<MdbListResponse> ListItemsAsync(
        string apiKey,
        string resource,
        CancellationToken cancellationToken)
        => SendAsync(
            HttpMethod.Get,
            BuildListItemsPath(apiKey, resource),
            null,
            false,
            cancellationToken);

    internal static string BuildListItemsPath(string apiKey, string resource)
    {
        // The regular payload carries rank and media-specific ids. MDBList's
        // ids_only payload drops rank and moves TMDB to a different field,
        // which cannot preserve the order of a mixed movie and show list.
        var separator = resource.Contains('?', StringComparison.Ordinal) ? "&" : "?limit=500&";
        return resource + separator + "apikey=" + Uri.EscapeDataString(apiKey);
    }

    private async Task<MdbListResponse> SendAsync(
        HttpMethod method,
        string relativePath,
        JsonNode? body,
        bool authenticatesKey,
        CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(method, relativePath);
        if (body is not null)
        {
            request.Content = JsonContent.Create(body, options: CompanionJson.CamelCase);
        }

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(RequestTimeout);
        try
        {
            using var response = await _client.SendAsync(
                request,
                HttpCompletionOption.ResponseHeadersRead,
                timeout.Token).ConfigureAwait(false);
            var quota = ReadQuota(response);
            var retryAt = ReadRetryAt(response, quota.ResetAt);
            JsonNode? parsed = null;
            if (response.Content.Headers.ContentLength is > MaxResponseBytes)
            {
                _logger.Log(
                    _failures.Failure(Subject, "invalid_response"),
                    "MDBList returned a response larger than {MaxBytes} bytes",
                    MaxResponseBytes);
                return new MdbListResponse(
                    HttpStatusCode.BadGateway,
                    null,
                    quota,
                    retryAt);
            }

            if (response.Content.Headers.ContentLength != 0)
            {
                await response.Content.LoadIntoBufferAsync(MaxResponseBytes, timeout.Token)
                    .ConfigureAwait(false);
                var bytes = await response.Content.ReadAsByteArrayAsync(timeout.Token)
                    .ConfigureAwait(false);
                if (bytes.Length > 0)
                {
                    try
                    {
                        parsed = JsonNode.Parse(bytes);
                    }
                    catch (JsonException)
                    {
                        _logger.Log(
                            _failures.Failure(Subject, "invalid_response"),
                            "MDBList returned an invalid JSON response (HTTP {StatusCode})",
                            (int)response.StatusCode);
                        return new MdbListResponse(
                            HttpStatusCode.BadGateway,
                            null,
                            quota,
                            retryAt);
                    }
                }
            }

            var hasMore = response.Headers.TryGetValues("X-Has-More", out var values)
                && values.Any(value => value.Equals("true", StringComparison.OrdinalIgnoreCase));
            if (!hasMore && parsed is JsonObject detail)
            {
                hasMore = BodyHasMore(detail);
            }
            ReportStatus(response.StatusCode, retryAt, authenticatesKey);
            return new MdbListResponse(response.StatusCode, parsed, quota, retryAt, hasMore);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            _logger.Log(
                _failures.Failure(Subject, "timeout"),
                "MDBList did not answer within {TimeoutSeconds} seconds",
                RequestTimeout.TotalSeconds);
            return new MdbListResponse(HttpStatusCode.GatewayTimeout, null, new(null, null, null), null);
        }
        catch (HttpRequestException exception)
        {
            // Never propagate or log HttpRequestException itself: its message
            // can contain the API-key-bearing request URI on some
            // handlers/runtimes. Its error category is safe.
            _logger.Log(
                _failures.Failure(Subject, "unreachable"),
                "MDBList could not be reached ({HttpRequestError})",
                exception.HttpRequestError);
            return new MdbListResponse(HttpStatusCode.BadGateway, null, new(null, null, null), null);
        }
    }

    private void ReportStatus(HttpStatusCode status, long? retryAt, bool authenticatesKey)
    {
        var code = (int)status;
        if (status.IsSuccess())
        {
            if (_failures.Recovered(Subject))
            {
                _logger.LogInformation("MDBList is answering again");
            }
        }
        else if (authenticatesKey && status is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden)
        {
            _logger.Log(
                _failures.Failure(Subject, "rejected"),
                "MDBList rejected the API key (HTTP {StatusCode})",
                code);
        }
        else if (code == StatusCodes.Status429TooManyRequests)
        {
            _logger.Log(
                _failures.Failure(Subject, "rate_limited"),
                "MDBList rate limit reached; requests resume after {RetryAt}",
                retryAt is { } seconds ? DateTimeOffset.FromUnixTimeSeconds(seconds) : (DateTimeOffset?)null);
        }
        else if (code >= 500)
        {
            _logger.Log(
                _failures.Failure(Subject, "unavailable"),
                "MDBList returned HTTP {StatusCode}",
                code);
        }
        else
        {
            // Per-request outcomes such as a private or missing public list.
            _logger.LogDebug("MDBList returned HTTP {StatusCode}", code);
        }
    }

    private static RatingQuotaResponse ReadQuota(HttpResponseMessage response)
        => new(
            HeaderInteger(response, "X-RateLimit-Limit"),
            HeaderInteger(response, "X-RateLimit-Remaining"),
            HeaderInteger(response, "X-RateLimit-Reset"));

    private static long? HeaderInteger(HttpResponseMessage response, string name)
        => response.Headers.TryGetValues(name, out var values)
            && long.TryParse(values.FirstOrDefault(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed)
                ? parsed
                : null;

    /// <summary>
    /// Reads MDBList's body pagination markers. Wrongly typed markers are
    /// treated as absent rather than failing the list request.
    /// </summary>
    internal static bool BodyHasMore(JsonObject detail)
    {
        var pagination = detail["pagination"] as JsonObject;
        if (JsonRead.Flag(detail["has_more"]) == true
            || JsonRead.Flag(pagination?["has_more"]) == true)
        {
            return true;
        }

        return pagination is not null
            && Integer(pagination["total"]) is { } total
            && Integer(pagination["offset"]) is { } offset
            && Integer(pagination["limit"]) is { } limit
            && offset + limit < total;
    }

    private static long? Integer(JsonNode? node)
        => node is JsonValue value && value.TryGetValue<long>(out var number)
            ? number
            : long.TryParse(node?.ToString(), NumberStyles.Integer, CultureInfo.InvariantCulture, out number)
                ? number
                : null;

    private static long? ReadRetryAt(HttpResponseMessage response, long? quotaResetAt)
    {
        var now = DateTimeOffset.UtcNow;
        var retry = response.Headers.RetryAfter;
        if (retry?.Delta is { } delta)
        {
            return now.Add(delta < TimeSpan.FromSeconds(1) ? TimeSpan.FromSeconds(1) : delta)
                .ToUnixTimeSeconds();
        }

        if (retry?.Date is { } date)
        {
            return date.ToUnixTimeSeconds();
        }

        return quotaResetAt is { } reset
            && HeaderInteger(response, "X-RateLimit-Remaining") == 0
                ? reset
                : null;
    }

}
