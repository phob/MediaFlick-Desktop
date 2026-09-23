using System.Collections.Concurrent;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Why a configured service last failed. Only these fixed categories are
/// retained: upstream response text and transport exception messages can name
/// the service address or echo request data, so they never enter health state.
/// </summary>
public enum ServiceFailure
{
    NotConfigured,
    Rejected,
    Unavailable,
    Timeout,
    Unreachable,
    InvalidResponse,
    RequestFailed
}

public sealed class ServiceHealthStore
{
    private readonly ConcurrentDictionary<string, HealthRecord> _records =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly TimeProvider _time;

    public ServiceHealthStore(TimeProvider? timeProvider = null)
    {
        _time = timeProvider ?? TimeProvider.System;
    }

    public void Success(string service)
    {
        var observedAt = _time.GetUtcNow();
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
                Failure = null
            });
    }

    public void Failure(string service, ServiceFailure failure)
    {
        var observedAt = _time.GetUtcNow();
        _records.AddOrUpdate(
            service,
            _ => new HealthRecord(
                ServiceHealthState.Unhealthy,
                null,
                observedAt,
                failure),
            (_, previous) => previous with
            {
                State = ServiceHealthState.Unhealthy,
                LastFailure = observedAt,
                Failure = failure
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
        ServiceFailure? Failure);

    public enum ServiceHealthState
    {
        Unknown,
        Healthy,
        Unhealthy
    }
}
