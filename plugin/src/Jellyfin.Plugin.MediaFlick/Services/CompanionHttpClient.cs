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
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(10);
    private readonly IHttpClientFactory _factory;
    private readonly ServiceHealthStore _health;
    private readonly ILogger<CompanionHttpClient> _logger;
    private readonly FailureLogGate _failures = new();

    public CompanionHttpClient(
        IHttpClientFactory factory,
        ServiceHealthStore health,
        ILogger<CompanionHttpClient> logger)
    {
        _factory = factory;
        _health = health;
        _logger = logger;
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
                System.Globalization.CultureInfo.InvariantCulture));
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
            var text = await response.Content.ReadAsStringAsync(timeout.Token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                var message = UpstreamMessage(serviceName, response.StatusCode, text);
                _health.Failure(serviceName, message);
                ReportFailureStatus(serviceName, response.StatusCode, seerrUserId is not null);
                throw new GatewayException(
                    (int)response.StatusCode,
                    message);
            }

            _health.Success(serviceName);
            JsonNode? parsed = null;
            if (!string.IsNullOrWhiteSpace(text))
            {
                try
                {
                    parsed = JsonNode.Parse(text);
                }
                catch (JsonException)
                {
                    _logger.Log(
                        _failures.Failure(serviceName, "invalid_response"),
                        "{Service} returned a non-JSON response",
                        serviceName);
                    throw new GatewayException(
                        StatusCodes.Status502BadGateway,
                        $"{serviceName} returned a non-JSON response");
                }
            }

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
            _health.Failure(serviceName, "request timed out");
            _logger.Log(
                _failures.Failure(serviceName, "timeout"),
                "{Service} did not answer within {TimeoutSeconds} seconds",
                serviceName,
                RequestTimeout.TotalSeconds);
            throw new GatewayException(
                StatusCodes.Status504GatewayTimeout,
                $"{serviceName} did not answer in time");
        }
        catch (HttpRequestException exception)
        {
            _health.Failure(serviceName, exception.Message);
            // The exception text can name the configured service address, so
            // only its error category is logged.
            _logger.Log(
                _failures.Failure(serviceName, "unreachable"),
                "{Service} could not be reached ({HttpRequestError})",
                serviceName,
                exception.HttpRequestError);
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                $"could not reach {serviceName}");
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
        return response?["version"]?.GetValue<string>();
    }

    private static void ValidateConfiguration(string name, ServiceConfiguration service)
    {
        if (!service.Enabled)
        {
            throw new GatewayException(StatusCodes.Status503ServiceUnavailable, $"{name} is disabled");
        }

        if (!Uri.TryCreate(service.BaseUrl, UriKind.Absolute, out var uri)
            || (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps)
            || string.IsNullOrWhiteSpace(service.ApiKey))
        {
            throw new GatewayException(
                StatusCodes.Status503ServiceUnavailable,
                $"{name} is not configured");
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

    private static string UpstreamMessage(string service, HttpStatusCode status, string body)
    {
        try
        {
            var parsed = JsonNode.Parse(body);
            var message = parsed?["message"]?.GetValue<string>()
                ?? parsed?["error"]?.GetValue<string>();
            if (!string.IsNullOrWhiteSpace(message))
            {
                return $"{service}: {message}";
            }
        }
        catch (JsonException)
        {
        }

        return $"{service} returned HTTP {(int)status}";
    }
}
