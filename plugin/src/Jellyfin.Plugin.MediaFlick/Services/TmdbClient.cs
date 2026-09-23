using System.Globalization;
using System.IO.Compression;
using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed record TmdbResponse(HttpStatusCode StatusCode, JsonNode? Body, long? RetryAt);

public sealed record ArtworkResponse(HttpStatusCode StatusCode, byte[] Body, string ContentType);

internal interface ITmdbTransport
{
    Task<TmdbResponse> GetAsync(
        string credential,
        string path,
        IReadOnlyDictionary<string, string> query,
        CancellationToken cancellationToken);

    Task<ArtworkResponse> GetArtworkAsync(
        string size,
        string path,
        CancellationToken cancellationToken);
}

/// <summary>
/// Fixed-origin TMDB transport. Credentials are added after the relative path
/// is fixed and no request URI is ever relayed through an exception or log.
/// Diagnostics log only status codes and error categories, never the request
/// URI, the exception text, or the response body.
/// </summary>
internal sealed class TmdbHttpTransport : ITmdbTransport, IDisposable
{
    private const int MaxResponseBytes = 8 * 1024 * 1024;
    private const string ApiSubject = "TMDB";
    private const string ImageSubject = "TMDB images";
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(20);
    private readonly HttpClient _client;
    private readonly HttpClient _imageClient;
    private readonly ILogger<TmdbHttpTransport> _logger;
    private readonly FailureLogGate _failures = new();
    private readonly TimeProvider _time;

    public TmdbHttpTransport(
        ILogger<TmdbHttpTransport> logger,
        TimeProvider? timeProvider = null)
    {
        _logger = logger;
        _time = timeProvider ?? TimeProvider.System;
        var handler = new SocketsHttpHandler
        {
            AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.All,
            ConnectTimeout = TimeSpan.FromSeconds(8),
            PooledConnectionLifetime = TimeSpan.FromMinutes(10)
        };
        _client = new HttpClient(handler, true)
        {
            BaseAddress = new Uri("https://api.themoviedb.org/", UriKind.Absolute),
            Timeout = Timeout.InfiniteTimeSpan
        };
        _client.DefaultRequestHeaders.Accept.ParseAdd("application/json");
        _client.DefaultRequestHeaders.UserAgent.ParseAdd("MediaFlick-Companion/0.2");
        _imageClient = new HttpClient(new SocketsHttpHandler
        {
            AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.All,
            ConnectTimeout = TimeSpan.FromSeconds(8),
            PooledConnectionLifetime = TimeSpan.FromMinutes(10)
        }, true)
        {
            BaseAddress = new Uri("https://image.tmdb.org/t/p/", UriKind.Absolute),
            Timeout = Timeout.InfiniteTimeSpan
        };
        _imageClient.DefaultRequestHeaders.UserAgent.ParseAdd("MediaFlick-Companion/0.2");
    }

    public void Dispose()
    {
        _client.Dispose();
        _imageClient.Dispose();
    }

    public async Task<ArtworkResponse> GetArtworkAsync(
        string size,
        string path,
        CancellationToken cancellationToken)
    {
        if (!SafeArtwork(size, path))
        {
            return new ArtworkResponse(HttpStatusCode.BadRequest, [], "application/octet-stream");
        }
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(RequestTimeout);
        try
        {
            using var response = await _imageClient.GetAsync(
                size + "/" + path.TrimStart('/'),
                HttpCompletionOption.ResponseHeadersRead,
                timeout.Token).ConfigureAwait(false);
            if (response.Content.Headers.ContentLength is > MaxResponseBytes)
            {
                ReportOversized(ImageSubject);
                return new ArtworkResponse(HttpStatusCode.BadGateway, [], "application/octet-stream");
            }
            await response.Content.LoadIntoBufferAsync(MaxResponseBytes, timeout.Token)
                .ConfigureAwait(false);
            var bytes = await response.Content.ReadAsByteArrayAsync(timeout.Token)
                .ConfigureAwait(false);
            ReportStatus(ImageSubject, response.StatusCode, false);
            return new ArtworkResponse(
                response.StatusCode,
                bytes,
                response.Content.Headers.ContentType?.MediaType ?? "application/octet-stream");
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            ReportTimeout(ImageSubject);
            return new ArtworkResponse(HttpStatusCode.GatewayTimeout, [], "application/octet-stream");
        }
        catch (HttpRequestException exception)
        {
            ReportUnreachable(ImageSubject, exception);
            return new ArtworkResponse(HttpStatusCode.BadGateway, [], "application/octet-stream");
        }
    }

    public async Task<TmdbResponse> GetAsync(
        string credential,
        string path,
        IReadOnlyDictionary<string, string> query,
        CancellationToken cancellationToken)
    {
        if (!SafePath(path))
        {
            return new TmdbResponse(HttpStatusCode.BadRequest, null, null);
        }
        var pairs = query
            .Where(entry => !string.IsNullOrWhiteSpace(entry.Value))
            .Select(entry => Uri.EscapeDataString(entry.Key) + "=" + Uri.EscapeDataString(entry.Value))
            .ToList();
        var bearer = credential.Count(character => character == '.') == 2;
        if (!bearer)
        {
            pairs.Add("api_key=" + Uri.EscapeDataString(credential));
        }
        var relative = path.TrimStart('/') + (pairs.Count > 0 ? "?" + string.Join('&', pairs) : string.Empty);
        using var request = new HttpRequestMessage(HttpMethod.Get, relative);
        if (bearer)
        {
            request.Headers.Authorization = new("Bearer", credential);
        }
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(RequestTimeout);
        try
        {
            using var response = await _client.SendAsync(
                request,
                HttpCompletionOption.ResponseHeadersRead,
                timeout.Token).ConfigureAwait(false);
            var retryAt = RetryAt(response);
            if (response.Content.Headers.ContentLength is > MaxResponseBytes)
            {
                ReportOversized(ApiSubject);
                return new TmdbResponse(HttpStatusCode.BadGateway, null, retryAt);
            }
            await response.Content.LoadIntoBufferAsync(MaxResponseBytes, timeout.Token)
                .ConfigureAwait(false);
            var bytes = await response.Content.ReadAsByteArrayAsync(timeout.Token)
                .ConfigureAwait(false);
            JsonNode? body = null;
            if (bytes.Length > 0)
            {
                try
                {
                    body = JsonNode.Parse(bytes);
                }
                catch (JsonException)
                {
                    _logger.Log(
                        _failures.Failure(ApiSubject, "invalid_response"),
                        "TMDB returned an invalid JSON response (HTTP {StatusCode})",
                        (int)response.StatusCode);
                    return new TmdbResponse(HttpStatusCode.BadGateway, null, retryAt);
                }
            }
            ReportStatus(ApiSubject, response.StatusCode, true);
            return new TmdbResponse(response.StatusCode, body, retryAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            ReportTimeout(ApiSubject);
            return new TmdbResponse(HttpStatusCode.GatewayTimeout, null, null);
        }
        catch (HttpRequestException exception)
        {
            ReportUnreachable(ApiSubject, exception);
            return new TmdbResponse(HttpStatusCode.BadGateway, null, null);
        }
    }

    private void ReportStatus(string subject, HttpStatusCode status, bool authenticated)
    {
        var code = (int)status;
        if (status.IsSuccess())
        {
            if (_failures.Recovered(subject))
            {
                _logger.LogInformation("{Service} is answering again", subject);
            }
        }
        else if (authenticated && status is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden)
        {
            _logger.Log(
                _failures.Failure(subject, "rejected"),
                "{Service} rejected the API credential (HTTP {StatusCode})",
                subject,
                code);
        }
        else if (code == StatusCodes.Status429TooManyRequests)
        {
            _logger.Log(
                _failures.Failure(subject, "rate_limited"),
                "{Service} rate limit reached (HTTP {StatusCode})",
                subject,
                code);
        }
        else if (code >= 500)
        {
            _logger.Log(
                _failures.Failure(subject, "unavailable"),
                "{Service} returned HTTP {StatusCode}",
                subject,
                code);
        }
        else
        {
            // Per-request outcomes such as a title or image that does not exist.
            _logger.LogDebug("{Service} returned HTTP {StatusCode}", subject, code);
        }
    }

    private void ReportOversized(string subject)
        => _logger.Log(
            _failures.Failure(subject, "invalid_response"),
            "{Service} returned a response larger than {MaxBytes} bytes",
            subject,
            MaxResponseBytes);

    private void ReportTimeout(string subject)
        => _logger.Log(
            _failures.Failure(subject, "timeout"),
            "{Service} did not answer within {TimeoutSeconds} seconds",
            subject,
            RequestTimeout.TotalSeconds);

    // Never log the exception itself: its message can include the request URI,
    // which carries a v3 API key. Its error category is safe.
    private void ReportUnreachable(string subject, HttpRequestException exception)
        => _logger.Log(
            _failures.Failure(subject, "unreachable"),
            "{Service} could not be reached ({HttpRequestError})",
            subject,
            exception.HttpRequestError);

    private static bool SafePath(string path)
        => path.StartsWith("3/", StringComparison.Ordinal)
            && path.All(character => char.IsAsciiLetterOrDigit(character) || character is '/' or '-' or '_');

    internal static bool SafeArtwork(string size, string path)
        => size is "w92" or "w154" or "w185" or "w300" or "w342" or "w500"
            or "w780" or "w1280" or "original"
            && path.Length is > 4 and <= 200
            && path[0] == '/'
            && !path.Contains("..", StringComparison.Ordinal)
            && path.All(character => char.IsAsciiLetterOrDigit(character)
                || character is '/' or '-' or '_' or '.');

    private long? RetryAt(HttpResponseMessage response)
    {
        if (response.Headers.RetryAfter?.Delta is { } delta)
        {
            return _time.GetUtcNow().Add(delta).ToUnixTimeSeconds();
        }
        return response.Headers.RetryAfter?.Date?.ToUnixTimeSeconds();
    }

}
