using System.Collections.Concurrent;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Keeps a failing dependency from flooding the Jellyfin log. The first
/// failure of a kind for a subject is a Warning, repeats of that kind are
/// Debug, and <see cref="Recovered"/> reports the first success afterwards.
/// </summary>
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

    /// <summary>Returns true when the subject was previously failing.</summary>
    public bool Recovered(string subject) => _active.TryRemove(subject, out _);
}

internal static class CompanionLogging
{
    /// <summary>
    /// Returns the exception only when its rendered text cannot reveal the
    /// secret. Callers then log the exception type instead of the object.
    /// </summary>
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
