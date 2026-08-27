using System.Globalization;
using System.IO.Compression;
using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;

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
/// </summary>
internal sealed class TmdbHttpTransport : ITmdbTransport, IDisposable
{
    private const int MaxResponseBytes = 8 * 1024 * 1024;
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(20);
    private readonly HttpClient _client;
    private readonly HttpClient _imageClient;

    public TmdbHttpTransport()
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
                return new ArtworkResponse(HttpStatusCode.BadGateway, [], "application/octet-stream");
            }
            await using var stream = await response.Content.ReadAsStreamAsync(timeout.Token)
                .ConfigureAwait(false);
            var bytes = await ReadBoundedAsync(stream, timeout.Token).ConfigureAwait(false);
            return bytes is null
                ? new ArtworkResponse(HttpStatusCode.BadGateway, [], "application/octet-stream")
                : new ArtworkResponse(
                    response.StatusCode,
                    bytes,
                    response.Content.Headers.ContentType?.MediaType ?? "application/octet-stream");
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return new ArtworkResponse(HttpStatusCode.GatewayTimeout, [], "application/octet-stream");
        }
        catch (HttpRequestException)
        {
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
                return new TmdbResponse(HttpStatusCode.BadGateway, null, retryAt);
            }
            await using var stream = await response.Content.ReadAsStreamAsync(timeout.Token)
                .ConfigureAwait(false);
            var bytes = await ReadBoundedAsync(stream, timeout.Token).ConfigureAwait(false);
            if (bytes is null)
            {
                return new TmdbResponse(HttpStatusCode.BadGateway, null, retryAt);
            }
            JsonNode? body = null;
            if (bytes.Length > 0)
            {
                try
                {
                    body = JsonNode.Parse(bytes);
                }
                catch (JsonException)
                {
                    return new TmdbResponse(HttpStatusCode.BadGateway, null, retryAt);
                }
            }
            return new TmdbResponse(response.StatusCode, body, retryAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return new TmdbResponse(HttpStatusCode.GatewayTimeout, null, null);
        }
        catch (HttpRequestException)
        {
            return new TmdbResponse(HttpStatusCode.BadGateway, null, null);
        }
    }

    private static bool SafePath(string path)
        => path.StartsWith("3/", StringComparison.Ordinal)
            && path.All(character => char.IsAsciiLetterOrDigit(character) || character is '/' or '-' or '_');

    private static bool SafeArtwork(string size, string path)
        => size is "w342" or "w500" or "w780" or "w1280" or "original"
            && path.Length is > 4 and <= 200
            && path[0] == '/'
            && !path.Contains("..", StringComparison.Ordinal)
            && path.All(character => char.IsAsciiLetterOrDigit(character)
                || character is '/' or '-' or '_' or '.');

    private static long? RetryAt(HttpResponseMessage response)
    {
        if (response.Headers.RetryAfter?.Delta is { } delta)
        {
            return DateTimeOffset.UtcNow.Add(delta).ToUnixTimeSeconds();
        }
        return response.Headers.RetryAfter?.Date?.ToUnixTimeSeconds();
    }

    private static async Task<byte[]?> ReadBoundedAsync(Stream stream, CancellationToken cancellationToken)
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
