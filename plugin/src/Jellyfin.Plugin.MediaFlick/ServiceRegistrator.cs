using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Controller;
using MediaBrowser.Controller.Configuration;
using MediaBrowser.Controller.Plugins;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick;

public sealed class ServiceRegistrator : IPluginServiceRegistrator
{
    public void RegisterServices(
        IServiceCollection serviceCollection,
        IServerApplicationHost applicationHost)
    {
        serviceCollection.AddHttpClient(CompanionHttpClient.ClientName);
        serviceCollection.AddSingleton<ServiceHealthStore>();
        serviceCollection.AddSingleton<CompanionHttpClient>();
        serviceCollection.AddSingleton<CalendarCache>();
        serviceCollection.AddSingleton<CalendarService>();
        serviceCollection.AddSingleton<SeerrGateway>();
        serviceCollection.AddSingleton<RatingsCacheStore>(serviceProvider =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            return new RatingsCacheStore(
                Path.Combine(dataPath, "ratings-v1-cache.json"),
                serviceProvider.GetRequiredService<ILogger<RatingsCacheStore>>());
        });
        serviceCollection.AddSingleton<IRatingSecretStore>(serviceProvider =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            var keyRingPath = Path.Combine(dataPath, "data-protection-keys");
            Directory.CreateDirectory(keyRingPath);
            var protection = DataProtectionProvider.Create(
                new DirectoryInfo(keyRingPath),
                builder => builder.SetApplicationName("Jellyfin.MediaFlick.Companion"));
            return new DataProtectedRatingSecretStore(
                protection,
                keyRingPath,
                serviceProvider.GetRequiredService<ILogger<DataProtectedRatingSecretStore>>());
        });
        serviceCollection.AddSingleton<IMdbListTransport, MdbListHttpTransport>();
        serviceCollection.AddSingleton<ITmdbTransport, TmdbHttpTransport>();
        serviceCollection.AddSingleton(serviceProvider => new CollectionProviderService(
            serviceProvider.GetRequiredService<ITmdbTransport>(),
            serviceProvider.GetRequiredService<IMdbListTransport>(),
            serviceProvider.GetRequiredService<IRatingSecretStore>(),
            serviceProvider.GetRequiredService<RatingsCacheStore>(),
            serviceProvider.GetRequiredService<ILogger<CollectionProviderService>>(),
            serviceProvider.GetRequiredService<IServerConfigurationManager>()
                .Configuration.PreferredMetadataLanguage,
            serviceProvider.GetRequiredService<IServerConfigurationManager>()
                .Configuration.MetadataCountryCode));
        serviceCollection.AddSingleton(serviceProvider => new RatingsService(
            serviceProvider.GetRequiredService<RatingsCacheStore>(),
            serviceProvider.GetRequiredService<IRatingSecretStore>(),
            serviceProvider.GetRequiredService<IMdbListTransport>(),
            serviceProvider.GetRequiredService<ILogger<RatingsService>>(),
            tmdbTransport: serviceProvider.GetRequiredService<ITmdbTransport>()));
    }
}
