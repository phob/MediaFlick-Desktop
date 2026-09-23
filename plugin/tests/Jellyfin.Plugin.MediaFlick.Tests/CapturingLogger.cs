using System.Collections.Concurrent;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Tests;

internal sealed record CapturedLog(LogLevel Level, string Message, Exception? Exception)
{
    /// <summary>Everything a log sink could render for this entry.</summary>
    public string Rendered => Message + Environment.NewLine + Exception;
}

internal sealed class CapturingLogger<T> : ILogger<T>
{
    private readonly ConcurrentQueue<CapturedLog> _entries = new();

    public IReadOnlyList<CapturedLog> Entries => _entries.ToArray();

    public IDisposable? BeginScope<TState>(TState state)
        where TState : notnull
        => null;

    public bool IsEnabled(LogLevel logLevel) => true;

    public void Log<TState>(
        LogLevel logLevel,
        EventId eventId,
        TState state,
        Exception? exception,
        Func<TState, Exception?, string> formatter)
        => _entries.Enqueue(new CapturedLog(logLevel, formatter(state, exception), exception));
}
