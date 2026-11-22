using System.Collections.Concurrent;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed class FailureLogGate
{
    private readonly ConcurrentDictionary<string, string> _active =
        new(StringComparer.OrdinalIgnoreCase);

    public LogLevel Failure(string subject, string kind)
    {
        while (true)
        {
            if (_active.TryAdd(subject, kind))
            {
                return LogLevel.Warning;
            }

            if (_active.TryGetValue(subject, out var previous))
            {
                if (string.Equals(previous, kind, StringComparison.Ordinal))
                {
                    return LogLevel.Debug;
                }

                if (_active.TryUpdate(subject, kind, previous))
                {
                    return LogLevel.Warning;
                }
            }
        }
    }

    public bool Recovered(string subject) => _active.TryRemove(subject, out _);
}

internal static class CompanionLogging
{
    public static Exception? WithoutSecret(Exception exception, string? secret)
    {
        if (string.IsNullOrEmpty(secret))
        {
            return exception;
        }

        var text = exception.ToString();
        return text.Contains(secret, StringComparison.Ordinal)
            || text.Contains(Uri.EscapeDataString(secret), StringComparison.Ordinal)
                ? null
                : exception;
    }
}
