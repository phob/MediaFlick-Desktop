using System.Text;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Api;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Mvc.Abstractions;
using Microsoft.AspNetCore.Routing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class AdminControllerTests
{
    [Fact]
    public async Task GetConfigurationReturnsTypedDtoSerializedAsCamelCaseUnderPascalCaseHost()
    {
        var configuration = new PluginConfiguration
        {
            Sonarr = new ServiceConfiguration
            {
                Enabled = true,
                BaseUrl = "http://sonarr:8989",
                ApiKey = "sonarr-secret-key"
            },
            Radarr = new ServiceConfiguration
            {
                Enabled = false,
                BaseUrl = string.Empty,
                ApiKey = "   "
            },
            Seerr = new ServiceConfiguration
            {
                Enabled = true,
                BaseUrl = "https://seerr.example.com",
                ApiKey = string.Empty
            },
            AutoImportSeerrUsers = true,
            ProtectedMdbListApiKey = "protected-mdblist-secret",
            ProtectedTmdbApiKey = "protected-tmdb-secret"
        };

        var controller = CreateController(() => configuration);
        var actionResult = controller.GetConfiguration();

        var jsonResult = Assert.IsType<JsonResult>(actionResult.Result);
        Assert.Same(CompanionJson.CamelCase, jsonResult.SerializerSettings);
        var dto = Assert.IsType<CompanionConfigurationResponse>(jsonResult.Value);
        Assert.Equal(new ServiceConfigurationResponse(true, "http://sonarr:8989", true), dto.Sonarr);
        Assert.Equal(new ServiceConfigurationResponse(false, string.Empty, false), dto.Radarr);
        Assert.Equal(new ServiceConfigurationResponse(true, "https://seerr.example.com", false), dto.Seerr);
        Assert.True(dto.AutoImportSeerrUsers);

        var (statusCode, contentType, body) = await ExecuteUnderPascalCaseHostAsync(jsonResult);
        Assert.Equal(StatusCodes.Status200OK, statusCode);
        Assert.NotNull(contentType);
        Assert.StartsWith("application/json", contentType, StringComparison.OrdinalIgnoreCase);

        Assert.DoesNotContain("sonarr-secret-key", body, StringComparison.Ordinal);
        Assert.DoesNotContain("protected-mdblist-secret", body, StringComparison.Ordinal);
        Assert.DoesNotContain("protected-tmdb-secret", body, StringComparison.Ordinal);
        Assert.DoesNotContain("\"apiKey\"", body, StringComparison.OrdinalIgnoreCase);

        var root = Assert.IsType<JsonObject>(JsonNode.Parse(body));
        Assert.Equal(
            ["sonarr", "radarr", "seerr", "autoImportSeerrUsers"],
            root.Select(static pair => pair.Key));
        Assert.True(root["autoImportSeerrUsers"]!.GetValue<bool>());

        AssertService(root["sonarr"], true, "http://sonarr:8989", true);
        AssertService(root["radarr"], false, string.Empty, false);
        AssertService(root["seerr"], true, "https://seerr.example.com", false);
    }

    [Fact]
    public async Task GetConfigurationDefaultsToUnconfiguredServicesWhenPluginInstanceIsAbsent()
    {
        var controller = new AdminController(CreateHttpClient());
        var actionResult = controller.GetConfiguration();

        var jsonResult = Assert.IsType<JsonResult>(actionResult.Result);
        Assert.Same(CompanionJson.CamelCase, jsonResult.SerializerSettings);
        var dto = Assert.IsType<CompanionConfigurationResponse>(jsonResult.Value);
        Assert.Equal(new ServiceConfigurationResponse(false, string.Empty, false), dto.Sonarr);
        Assert.Equal(new ServiceConfigurationResponse(false, string.Empty, false), dto.Radarr);
        Assert.Equal(new ServiceConfigurationResponse(false, string.Empty, false), dto.Seerr);
        Assert.False(dto.AutoImportSeerrUsers);

        var (statusCode, _, body) = await ExecuteUnderPascalCaseHostAsync(jsonResult);
        Assert.Equal(StatusCodes.Status200OK, statusCode);

        var root = Assert.IsType<JsonObject>(JsonNode.Parse(body));
        Assert.Equal(
            ["sonarr", "radarr", "seerr", "autoImportSeerrUsers"],
            root.Select(static pair => pair.Key));
        Assert.False(root["autoImportSeerrUsers"]!.GetValue<bool>());
        AssertService(root["sonarr"], false, string.Empty, false);
        AssertService(root["radarr"], false, string.Empty, false);
        AssertService(root["seerr"], false, string.Empty, false);
    }

    [Theory]
    [InlineData("", false)]
    [InlineData("   ", false)]
    [InlineData("\t\r\n", false)]
    [InlineData("secret-token-123", true)]
    [InlineData(" padded-secret ", true)]
    public async Task GetConfigurationReportsHasApiKeyWithoutExposingKeyMaterial(
        string apiKey,
        bool expectedHasApiKey)
    {
        var configuration = new PluginConfiguration
        {
            Sonarr = new ServiceConfiguration
            {
                Enabled = true,
                BaseUrl = "http://sonarr:8989",
                ApiKey = apiKey
            }
        };

        var controller = CreateController(() => configuration);
        var jsonResult = Assert.IsType<JsonResult>(controller.GetConfiguration().Result);
        var (_, _, body) = await ExecuteUnderPascalCaseHostAsync(jsonResult);

        if (!string.IsNullOrWhiteSpace(apiKey))
        {
            Assert.DoesNotContain(apiKey.Trim(), body, StringComparison.Ordinal);
        }

        var root = Assert.IsType<JsonObject>(JsonNode.Parse(body));
        AssertService(root["sonarr"], true, "http://sonarr:8989", expectedHasApiKey);
    }

    private static void AssertService(
        JsonNode? node,
        bool expectedEnabled,
        string expectedBaseUrl,
        bool expectedHasApiKey)
    {
        var service = Assert.IsType<JsonObject>(node);
        Assert.Equal(
            ["enabled", "baseUrl", "hasApiKey"],
            service.Select(static pair => pair.Key));
        Assert.Equal(expectedEnabled, service["enabled"]!.GetValue<bool>());
        Assert.Equal(expectedBaseUrl, service["baseUrl"]!.GetValue<string>());
        Assert.Equal(expectedHasApiKey, service["hasApiKey"]!.GetValue<bool>());
    }

    private static AdminController CreateController(Func<PluginConfiguration> getConfiguration)
        => new(CreateHttpClient(), getConfiguration);

    private static CompanionHttpClient CreateHttpClient()
        => new(
            new StubFactory(new StubHandler()),
            new ServiceHealthStore(),
            NullLogger<CompanionHttpClient>.Instance);

    private static async Task<(int StatusCode, string? ContentType, string Body)>
        ExecuteUnderPascalCaseHostAsync(IActionResult result)
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services
            .AddControllers()
            .AddJsonOptions(static options =>
                options.JsonSerializerOptions.PropertyNamingPolicy = null);

        await using var provider = services.BuildServiceProvider();
        using var responseStream = new MemoryStream();
        var httpContext = new DefaultHttpContext
        {
            RequestServices = provider
        };
        httpContext.Response.Body = responseStream;

        var actionContext = new ActionContext(
            httpContext,
            new RouteData(),
            new ActionDescriptor());
        await result.ExecuteResultAsync(actionContext);

        responseStream.Position = 0;
        using var reader = new StreamReader(responseStream, Encoding.UTF8);
        var body = await reader.ReadToEndAsync();
        return (httpContext.Response.StatusCode, httpContext.Response.ContentType, body);
    }
}
