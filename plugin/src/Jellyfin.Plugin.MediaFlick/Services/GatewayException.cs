namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// A failure answered to the client with <see cref="StatusCode"/> and the
/// exception message. Messages are fixed plugin wording, never upstream
/// response text or transport exception text.
/// </summary>
public sealed class GatewayException : Exception
{
    public GatewayException(int statusCode, string message, ServiceFailure? failure = null)
        : base(message)
    {
        StatusCode = statusCode;
        Failure = failure;
    }

    public int StatusCode { get; }

    /// <summary>Why an upstream service call failed, when one was attempted.</summary>
    public ServiceFailure? Failure { get; }
}
