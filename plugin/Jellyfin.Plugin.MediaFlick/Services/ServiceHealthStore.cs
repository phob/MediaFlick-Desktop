using System.Collections.Concurrent;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class ServiceHealthStore
{
    private readonly ConcurrentDictionary<string, HealthRecord> _records =
        new(StringComparer.OrdinalIgnoreCase);

    public void Success(string service)
    {
        var observedAt = DateTimeOffset.UtcNow;
        _records.AddOrUpdate(
            service,
            _ => new HealthRecord(
                ServiceHealthState.Healthy,
                observedAt,
                null,
                null),
            (_, previous) => previous with
            {
                State = ServiceHealthState.Healthy,
                LastSuccess = observedAt,
                Error = null
            });
    }

    public void Failure(string service, string message)
    {
        var observedAt = DateTimeOffset.UtcNow;
        _records.AddOrUpdate(
            service,
            _ => new HealthRecord(
                ServiceHealthState.Unhealthy,
                null,
                observedAt,
                message),
            (_, previous) => previous with
            {
                State = ServiceHealthState.Unhealthy,
                LastFailure = observedAt,
                Error = message
            });
    }

    public bool IsHealthy(string service)
        => _records.TryGetValue(service, out var record)
            && record.State == ServiceHealthState.Healthy;

    public HealthRecord Get(string service)
        => _records.GetValueOrDefault(service)
            ?? new HealthRecord(ServiceHealthState.Unknown, null, null, null);

    public sealed record HealthRecord(
        ServiceHealthState State,
        DateTimeOffset? LastSuccess,
        DateTimeOffset? LastFailure,
        string? Error);

    public enum ServiceHealthState
    {
        Unknown,
        Healthy,
        Unhealthy
    }
}
