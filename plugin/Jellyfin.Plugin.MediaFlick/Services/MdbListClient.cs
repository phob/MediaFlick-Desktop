using System.Globalization;
using System.IO.Compression;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed record MdbListResponse(
    HttpStatusCode StatusCode,
    JsonNode? Body,
    RatingQuotaResponse Quota,
    long? RetryAt);

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
/// </summary>
internal sealed class MdbListHttpTransport : IMdbListTransport, IDisposable
{
    private const int MaxResponseBytes = 8 * 1024 * 1024;
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(20);
    private readonly HttpClient _client;

    public MdbListHttpTransport()
    {
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
            cancellationToken);
    }

    public void Dispose() => _client.Dispose();

    /// <summary>
    /// Fetches one MDBList list's items. `resource` is an already-validated
    /// lists-relative path built by [`CuratedCollectionResolver`]; no caller
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
            cancellationToken);

    internal static string BuildListItemsPath(string apiKey, string resource)
    {
        // The regular payload carries rank and media-specific ids. MDBList's
        // ids_only payload drops rank and moves TMDB to a different field,
        // which cannot preserve the order of a mixed movie and show list.
        return resource + "?limit=500&apikey=" + Uri.EscapeDataString(apiKey);
    }

    private async Task<MdbListResponse> SendAsync(
        HttpMethod method,
        string relativePath,
        JsonNode? body,
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
                return new MdbListResponse(
                    HttpStatusCode.BadGateway,
                    null,
                    quota,
                    retryAt);
            }

            if (response.Content.Headers.ContentLength != 0)
            {
                await using var stream = await response.Content
                    .ReadAsStreamAsync(timeout.Token).ConfigureAwait(false);
                var bytes = await ReadBoundedAsync(stream, timeout.Token).ConfigureAwait(false);
                if (bytes is null)
                {
                    return new MdbListResponse(
                        HttpStatusCode.BadGateway,
                        null,
                        quota,
                        retryAt);
                }

                if (bytes.Length > 0)
                {
                    try
                    {
                        parsed = JsonNode.Parse(bytes);
                    }
                    catch (JsonException)
                    {
                        return new MdbListResponse(
                            HttpStatusCode.BadGateway,
                            null,
                            quota,
                            retryAt);
                    }
                }
            }

            return new MdbListResponse(response.StatusCode, parsed, quota, retryAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return new MdbListResponse(HttpStatusCode.GatewayTimeout, null, new(null, null, null), null);
        }
        catch (HttpRequestException)
        {
            // Never propagate HttpRequestException: its message can contain the
            // API-key-bearing request URI on some handlers/runtimes.
            return new MdbListResponse(HttpStatusCode.BadGateway, null, new(null, null, null), null);
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

    private static async Task<byte[]?> ReadBoundedAsync(
        Stream stream,
        CancellationToken cancellationToken)
    {
        using var destination = new MemoryStream();
        var buffer = new byte[16 * 1024];
        while (true)
        {
            var count = await stream.ReadAsync(buffer, cancellationToken).ConfigureAwait(false);
            if (count == 0)
            {
                return destination.ToArray();
            }

            if (destination.Length + count > MaxResponseBytes)
            {
                return null;
            }

            destination.Write(buffer, 0, count);
        }
    }
}
