namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class GatewayException : Exception
{
    public GatewayException(int statusCode, string message, ServiceFailure? failure = null)
        : base(message)
    {
        StatusCode = statusCode;
        Failure = failure;
    }

    public int StatusCode { get; }

    public ServiceFailure? Failure { get; }
}
