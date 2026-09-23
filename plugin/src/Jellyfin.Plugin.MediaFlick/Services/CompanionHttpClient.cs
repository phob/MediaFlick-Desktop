using System.Globalization;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CompanionHttpClient
{
    public const string ClientName = "MediaFlick.Companion";
    /// <summary>Seerr answers are single pages of results.</summary>
    internal const int MaxSeerrResponseBytes = 16 * 1024 * 1024;

    /// <summary>
    /// Sonarr's calendar and Radarr's movie list cover a whole library in one
    /// response, so they get a larger, still bounded, allowance.
    /// </summary>
    internal const int MaxLibraryResponseBytes = 64 * 1024 * 1024;

    private const int ReadChunkBytes = 81_920;
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(10);
    private readonly IHttpClientFactory _factory;
    private readonly ServiceHealthStore _health;
    private readonly ILogger<CompanionHttpClient> _logger;
    private readonly FailureLogGate _failures = new();
    private readonly int? _maxResponseBytes;

    public CompanionHttpClient(
        IHttpClientFactory factory,
        ServiceHealthStore health,
        ILogger<CompanionHttpClient> logger)
        : this(factory, health, logger, null)
    {
    }

    internal CompanionHttpClient(
        IHttpClientFactory factory,
        ServiceHealthStore health,
        ILogger<CompanionHttpClient> logger,
        int? maxResponseBytes)
    {
        _factory = factory;
        _health = health;
        _logger = logger;
        _maxResponseBytes = maxResponseBytes;
    }

    public async Task<JsonNode?> SendAsync(
        string serviceName,
        ServiceConfiguration service,
        HttpMethod method,
        string pathAndQuery,
        JsonNode? body,
        int? seerrUserId,
        CancellationToken cancellationToken)
    {
        ValidateConfiguration(serviceName, service);
        var baseUri = new Uri(service.BaseUrl.TrimEnd('/') + "/", UriKind.Absolute);
        var relative = pathAndQuery.TrimStart('/');
        using var request = new HttpRequestMessage(method, new Uri(baseUri, relative));
        request.Headers.Accept.ParseAdd("application/json");
        request.Headers.Add("X-Api-Key", service.ApiKey.Trim());
        if (seerrUserId is not null)
        {
            request.Headers.Add("X-API-User", seerrUserId.Value.ToString(
                CultureInfo.InvariantCulture));
        }

        if (body is not null)
        {
            request.Content = JsonContent.Create(body);
        }

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(RequestTimeout);
        try
        {
            using var response = await _factory.CreateClient(ClientName)
                .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeout.Token)
                .ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                var mappedUser = seerrUserId is not null;
                var failure = FailureKind(response.StatusCode, mappedUser);
                if (failure == ServiceFailure.RequestFailed)
                {
                    // A missing title or a per-user permission answer proves
                    // the service is reachable and accepts the key.
                    _health.Success(serviceName);
                }
                else
                {
                    _health.Failure(serviceName, failure);
                }
                ReportFailureStatus(serviceName, response.StatusCode, mappedUser);
                // The upstream body can echo request data or name internal
                // hosts, so Desktop only ever receives fixed plugin wording.
                throw new GatewayException(
                    (int)response.StatusCode,
                    FailureMessage(serviceName, response.StatusCode, mappedUser),
                    failure);
            }

            var maxBytes = _maxResponseBytes ?? MaxResponseBytes(serviceName);
            var bytes = await ReadBoundedAsync(response.Content, maxBytes, timeout.Token)
                .ConfigureAwait(false);
            if (bytes is null)
            {
                _health.Failure(serviceName, ServiceFailure.InvalidResponse);
                _logger.Log(
                    _failures.Failure(serviceName, "invalid_response"),
                    "{Service} returned a response larger than {MaxBytes} bytes",
                    serviceName,
                    maxBytes);
                throw new GatewayException(
                    StatusCodes.Status502BadGateway,
                    $"{DisplayName(serviceName)} returned a response that is too large",
                    ServiceFailure.InvalidResponse);
            }

            JsonNode? parsed = null;
            var json = WithoutByteOrderMark(bytes);
            if (!IsBlank(json))
            {
                try
                {
                    parsed = JsonNode.Parse(json.Span);
                }
                catch (JsonException)
                {
                    _health.Failure(serviceName, ServiceFailure.InvalidResponse);
                    _logger.Log(
                        _failures.Failure(serviceName, "invalid_response"),
                        "{Service} returned a non-JSON response",
                        serviceName);
                    throw new GatewayException(
                        StatusCodes.Status502BadGateway,
                        $"{DisplayName(serviceName)} returned an unreadable response",
                        ServiceFailure.InvalidResponse);
                }
            }

            _health.Success(serviceName);

            // Only a usable answer ends a failure streak; a service that keeps
            // returning non-JSON must stay on the Debug path after its first Warning.
            if (_failures.Recovered(serviceName))
            {
                _logger.LogInformation("{Service} is answering again", serviceName);
            }

            return parsed;
        }
        catch (GatewayException)
        {
            throw;
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            _health.Failure(serviceName, ServiceFailure.Timeout);
            _logger.Log(
                _failures.Failure(serviceName, "timeout"),
                "{Service} did not answer within {TimeoutSeconds} seconds",
                serviceName,
                RequestTimeout.TotalSeconds);
            throw new GatewayException(
                StatusCodes.Status504GatewayTimeout,
                $"{DisplayName(serviceName)} did not answer in time",
                ServiceFailure.Timeout);
        }
        catch (HttpRequestException exception)
        {
            // The exception text can name the configured service address, so
            // only its error category is recorded and logged.
            _health.Failure(serviceName, ServiceFailure.Unreachable);
            _logger.Log(
                _failures.Failure(serviceName, "unreachable"),
                "{Service} could not be reached ({HttpRequestError})",
                serviceName,
                exception.HttpRequestError);
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                $"could not reach {DisplayName(serviceName)}",
                ServiceFailure.Unreachable);
        }
    }

    public async Task<string?> TestAsync(
        string serviceName,
        ServiceConfiguration service,
        CancellationToken cancellationToken)
    {
        if (serviceName.Equals("seerr", StringComparison.OrdinalIgnoreCase))
        {
            // `/status` is public on Seerr and therefore proves nothing about
            // the supplied key. `/auth/me` must accept the key before the
            // informational version probe is useful.
            await SendAsync(
                serviceName,
                service,
                HttpMethod.Get,
                "api/v1/auth/me",
                null,
                null,
                cancellationToken).ConfigureAwait(false);
        }

        var path = serviceName.Equals("seerr", StringComparison.OrdinalIgnoreCase)
            ? "api/v1/status"
            : "api/v3/system/status";
        var response = await SendAsync(
            serviceName,
            service,
            HttpMethod.Get,
            path,
            null,
            null,
            cancellationToken).ConfigureAwait(false);
        return response is JsonObject status
            && status["version"] is JsonValue version
            && version.TryGetValue<string>(out var text)
                ? text
                : null;
    }

    private static void ValidateConfiguration(string name, ServiceConfiguration service)
    {
        if (!service.Enabled)
        {
            throw new GatewayException(
                StatusCodes.Status503ServiceUnavailable,
                $"{DisplayName(name)} is disabled",
                ServiceFailure.NotConfigured);
        }

        if (!Uri.TryCreate(service.BaseUrl, UriKind.Absolute, out var uri)
            || (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps)
            || string.IsNullOrWhiteSpace(service.ApiKey))
        {
            throw new GatewayException(
                StatusCodes.Status503ServiceUnavailable,
                $"{DisplayName(name)} is not configured",
                ServiceFailure.NotConfigured);
        }
    }

    /// <summary>
    /// Logs only the service name and status. The response body can echo
    /// request data, so it never reaches the log.
    /// </summary>
    private void ReportFailureStatus(string serviceName, HttpStatusCode status, bool mappedUser)
    {
        var code = (int)status;
        if (!mappedUser && status is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden)
        {
            _logger.Log(
                _failures.Failure(serviceName, "rejected"),
                "{Service} rejected the configured API key (HTTP {StatusCode})",
                serviceName,
                code);
        }
        else if (code is StatusCodes.Status408RequestTimeout or StatusCodes.Status429TooManyRequests
            or >= 500)
        {
            _logger.Log(
                _failures.Failure(serviceName, "unavailable"),
                "{Service} returned HTTP {StatusCode}",
                serviceName,
                code);
        }
        else
        {
            // Per-request outcomes such as a missing title or a Seerr user
            // without permission for an action.
            _logger.LogDebug("{Service} returned HTTP {StatusCode}", serviceName, code);
        }
    }

    internal static int MaxResponseBytes(string serviceName)
        => serviceName.Equals("seerr", StringComparison.OrdinalIgnoreCase)
            ? MaxSeerrResponseBytes
            : MaxLibraryResponseBytes;

    /// <summary>
    /// Reads at most <paramref name="maxBytes"/>, or returns null as soon as
    /// the body (declared or actual) is larger, without buffering the rest.
    /// </summary>
    internal static async Task<byte[]?> ReadBoundedAsync(
        HttpContent content,
        int maxBytes,
        CancellationToken cancellationToken)
    {
        if (content.Headers.ContentLength > maxBytes)
        {
            return null;
        }

        var stream = await content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
        await using (stream.ConfigureAwait(false))
        {
            using var buffer = new MemoryStream();
            var chunk = new byte[ReadChunkBytes];
            int read;
            while ((read = await stream.ReadAsync(chunk, cancellationToken).ConfigureAwait(false)) > 0)
            {
                if (buffer.Length + read > maxBytes)
                {
                    return null;
                }

                buffer.Write(chunk, 0, read);
            }

            return buffer.ToArray();
        }
    }

    private static ReadOnlyMemory<byte> WithoutByteOrderMark(byte[] bytes)
        => bytes is [0xEF, 0xBB, 0xBF, ..]
            ? bytes.AsMemory(3)
            : bytes;

    private static bool IsBlank(ReadOnlyMemory<byte> json)
    {
        foreach (var value in json.Span)
        {
            // JSON whitespace: space, tab, carriage return, line feed.
            if (value is not (0x20 or 0x09 or 0x0D or 0x0A))
            {
                return false;
            }
        }

        return true;
    }

    private static ServiceFailure FailureKind(HttpStatusCode status, bool mappedUser)
    {
        var code = (int)status;
        if (!mappedUser && status is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden)
        {
            return ServiceFailure.Rejected;
        }

        return code is StatusCodes.Status408RequestTimeout or StatusCodes.Status429TooManyRequests
            or >= 500
                ? ServiceFailure.Unavailable
                : ServiceFailure.RequestFailed;
    }

    /// <summary>
    /// Fixed Desktop-facing wording for an upstream status. Only the service
    /// name and status code are used; the response body never is.
    /// </summary>
    internal static string FailureMessage(string serviceName, HttpStatusCode status, bool mappedUser)
    {
        var name = DisplayName(serviceName);
        var code = (int)status;
        return status switch
        {
            HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden when mappedUser =>
                $"{name} did not allow this request for your account",
            HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden =>
                $"{name} rejected the configured API key",
            HttpStatusCode.NotFound => $"{name} could not find the requested item",
            HttpStatusCode.Conflict => $"{name} already has a matching request",
            HttpStatusCode.TooManyRequests => $"{name} is limiting requests; try again later",
            HttpStatusCode.BadRequest or HttpStatusCode.UnprocessableEntity =>
                $"{name} rejected the request",
            _ when code is StatusCodes.Status408RequestTimeout or >= 500 =>
                string.Create(CultureInfo.InvariantCulture, $"{name} is unavailable (HTTP {code})"),
            _ => string.Create(CultureInfo.InvariantCulture, $"{name} returned HTTP {code}")
        };
    }

    private static string DisplayName(string serviceName)
        => serviceName.ToLowerInvariant() switch
        {
            "seerr" => "Seerr",
            "sonarr" => "Sonarr",
            "radarr" => "Radarr",
            _ => "The service"
        };
}
