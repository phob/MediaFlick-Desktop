using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Controller;
using MediaBrowser.Controller.Plugins;
using Microsoft.Extensions.DependencyInjection;

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
    }
}
