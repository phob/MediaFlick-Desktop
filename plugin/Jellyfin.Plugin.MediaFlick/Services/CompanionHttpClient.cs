using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CompanionHttpClient
{
    public const string ClientName = "MediaFlick.Companion";
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(10);
    private readonly IHttpClientFactory _factory;
    private readonly ServiceHealthStore _health;

    public CompanionHttpClient(IHttpClientFactory factory, ServiceHealthStore health)
    {
        _factory = factory;
        _health = health;
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
                throw new GatewayException(
                    (int)response.StatusCode,
                    message);
            }

            _health.Success(serviceName);
            if (string.IsNullOrWhiteSpace(text))
            {
                return null;
            }

            try
            {
                return JsonNode.Parse(text);
            }
            catch (JsonException)
            {
                throw new GatewayException(
                    StatusCodes.Status502BadGateway,
                    $"{serviceName} returned a non-JSON response");
            }
        }
        catch (GatewayException)
        {
            throw;
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            _health.Failure(serviceName, "request timed out");
            throw new GatewayException(
                StatusCodes.Status504GatewayTimeout,
                $"{serviceName} did not answer in time");
        }
        catch (HttpRequestException exception)
        {
            _health.Failure(serviceName, exception.Message);
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
