using System.Collections.Concurrent;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class ServiceHealthStore
{
    private static readonly TimeSpan HealthyFor = TimeSpan.FromMinutes(30);
    private readonly ConcurrentDictionary<string, HealthRecord> _records =
        new(StringComparer.OrdinalIgnoreCase);

    public void Success(string service)
        => _records[service] = new HealthRecord(DateTimeOffset.UtcNow, null);

    public void Failure(string service, string message)
    {
        _records.AddOrUpdate(
            service,
            _ => new HealthRecord(null, message),
            (_, previous) => previous with { Error = message });
    }

    public bool IsHealthy(string service)
        => _records.TryGetValue(service, out var record)
            && record.LastSuccess is { } success
            && DateTimeOffset.UtcNow - success <= HealthyFor;

    public HealthRecord Get(string service)
        => _records.GetValueOrDefault(service) ?? new HealthRecord(null, null);

    public sealed record HealthRecord(DateTimeOffset? LastSuccess, string? Error);
}
