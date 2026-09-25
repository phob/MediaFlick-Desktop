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
        serviceCollection.AddSingleton<ISeerrTransport>(serviceProvider =>
            new CompanionSeerrTransport(serviceProvider.GetRequiredService<CompanionHttpClient>()));
        serviceCollection.AddSingleton(serviceProvider => new ArrFactsLookup(
            new CompanionArrTransport(serviceProvider.GetRequiredService<CompanionHttpClient>()),
            serviceProvider.GetRequiredService<ILogger<ArrFactsLookup>>()));
        serviceCollection.AddSingleton(serviceProvider => new SeerrGateway(
            serviceProvider.GetRequiredService<ISeerrTransport>(),
            serviceProvider.GetRequiredService<ILogger<SeerrGateway>>(),
            arr: serviceProvider.GetRequiredService<ArrFactsLookup>()));
        serviceCollection.AddSingleton<ProviderCacheStore>(serviceProvider =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            return new ProviderCacheStore(
                Path.Combine(dataPath, "ratings-v1-cache.json"),
                serviceProvider.GetRequiredService<ILogger<ProviderCacheStore>>());
        });
        serviceCollection.AddSingleton<IProviderSecretStore>(serviceProvider =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            var keyRingPath = Path.Combine(dataPath, "data-protection-keys");
            Directory.CreateDirectory(keyRingPath);
            var protection = DataProtectionProvider.Create(
                new DirectoryInfo(keyRingPath),
                builder => builder.SetApplicationName("Jellyfin.MediaFlick.Companion"));
            return new DataProtectedProviderSecretStore(
                protection,
                keyRingPath,
                serviceProvider.GetRequiredService<ILogger<DataProtectedProviderSecretStore>>());
        });
        serviceCollection.AddSingleton<IMdbListTransport, MdbListHttpTransport>();
        serviceCollection.AddSingleton<ITmdbTransport, TmdbHttpTransport>();
        serviceCollection.AddSingleton(serviceProvider => new CollectionProviderService(
            serviceProvider.GetRequiredService<ITmdbTransport>(),
            serviceProvider.GetRequiredService<IMdbListTransport>(),
            serviceProvider.GetRequiredService<IProviderSecretStore>(),
            serviceProvider.GetRequiredService<ProviderCacheStore>(),
            serviceProvider.GetRequiredService<ILogger<CollectionProviderService>>(),
            serviceProvider.GetRequiredService<IServerConfigurationManager>()
                .Configuration.PreferredMetadataLanguage,
            serviceProvider.GetRequiredService<IServerConfigurationManager>()
                .Configuration.MetadataCountryCode));
        serviceCollection.AddSingleton(serviceProvider => new RatingsService(
            serviceProvider.GetRequiredService<ProviderCacheStore>(),
            serviceProvider.GetRequiredService<IProviderSecretStore>(),
            serviceProvider.GetRequiredService<IMdbListTransport>(),
            serviceProvider.GetRequiredService<ILogger<RatingsService>>(),
            tmdbTransport: serviceProvider.GetRequiredService<ITmdbTransport>()));
    }
}
