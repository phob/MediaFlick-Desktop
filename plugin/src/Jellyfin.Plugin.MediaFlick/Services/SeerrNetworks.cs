using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The TV networks Desktop offers as a Discover filter. Neither Seerr nor
/// TMDB can search networks, so the Companion ships a curated list and
/// updates it with the plugin. Every id was checked against
/// <c>themoviedb.org/network/{id}</c>; names follow TMDB.
/// </summary>
internal static class SeerrNetworks
{
    public static IReadOnlyList<SeerrCompanyResponse> All { get; } =
    [
        new(213, "Netflix"),
        new(1024, "Prime Video"),
        new(2739, "Disney+"),
        new(2552, "Apple TV"),
        new(49, "HBO"),
        new(3186, "HBO Max"),
        new(453, "Hulu"),
        new(4330, "Paramount+"),
        new(3353, "Peacock"),
        new(1112, "Crunchyroll"),
        new(4353, "discovery+"),
        new(174, "AMC"),
        new(88, "FX"),
        new(67, "Showtime"),
        new(318, "Starz"),
        new(359, "Cinemax"),
        new(2, "ABC"),
        new(6, "NBC"),
        new(16, "CBS"),
        new(19, "FOX"),
        new(71, "The CW"),
        new(80, "Adult Swim"),
        new(56, "Cartoon Network"),
        new(13, "Nickelodeon"),
        new(4, "BBC One"),
        new(332, "BBC Two"),
        new(9, "ITV1"),
        new(26, "Channel 4"),
        new(1063, "Sky Atlantic"),
        new(31, "ZDF"),
        new(308, "Das Erste"),
        new(1628, "ARTE"),
    ];
}
