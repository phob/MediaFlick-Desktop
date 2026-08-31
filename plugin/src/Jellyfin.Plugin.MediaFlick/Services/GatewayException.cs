namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class GatewayException : Exception
{
    public GatewayException(int statusCode, string message)
        : base(message)
    {
        StatusCode = statusCode;
    }

    public int StatusCode { get; }
}
