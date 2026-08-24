using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Controller;
using MediaBrowser.Controller.Collections;
using MediaBrowser.Controller.Library;
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
        // Factory registration: the resolver's constructor is internal to keep
        // its internal transport/secret-store dependencies off the public
        // surface, and MS DI's type-based activation only scans public ones.
        serviceCollection.AddSingleton(serviceProvider => new CuratedCollectionResolver(
            serviceProvider.GetRequiredService<IMdbListTransport>(),
            serviceProvider.GetRequiredService<IRatingSecretStore>()));
        serviceCollection.AddSingleton(serviceProvider => new NativeCollectionSync(
            serviceProvider.GetRequiredService<CollectionsService>(),
            serviceProvider.GetRequiredService<ICollectionManager>(),
            serviceProvider.GetRequiredService<IUserManager>(),
            serviceProvider.GetRequiredService<ILibraryManager>(),
            serviceProvider.GetRequiredService<CuratedCollectionResolver>(),
            serviceProvider.GetRequiredService<ILogger<NativeCollectionSync>>()));
        serviceCollection.AddSingleton(serviceProvider =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            return new CollectionsService(
                serviceProvider.GetRequiredService<SeerrGateway>(),
                serviceProvider.GetRequiredService<ILibraryManager>(),
                Path.Combine(dataPath, "collections-v1-cache.json"));
        });
        serviceCollection.AddSingleton<RatingsCacheStore>(_ =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            return new RatingsCacheStore(Path.Combine(dataPath, "ratings-v1-cache.json"));
        });
        serviceCollection.AddSingleton<IRatingSecretStore>(_ =>
        {
            var dataPath = Plugin.Instance?.DataFolderPath
                ?? throw new InvalidOperationException("plugin data path is unavailable");
            var keyRingPath = Path.Combine(dataPath, "data-protection-keys");
            Directory.CreateDirectory(keyRingPath);
            var protection = DataProtectionProvider.Create(
                new DirectoryInfo(keyRingPath),
                builder => builder.SetApplicationName("Jellyfin.MediaFlick.Companion"));
            return new DataProtectedRatingSecretStore(protection, keyRingPath);
        });
        serviceCollection.AddSingleton<IMdbListTransport, MdbListHttpTransport>();
        serviceCollection.AddSingleton(serviceProvider => new RatingsService(
            serviceProvider.GetRequiredService<RatingsCacheStore>(),
            serviceProvider.GetRequiredService<IRatingSecretStore>(),
            serviceProvider.GetRequiredService<IMdbListTransport>()));
    }
}
