using System.Text.RegularExpressions;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The one IMDb title-id rule used by ratings, identity mapping, and Seerr
/// detail: "tt" followed by 5 to 12 digits, compared in lower case.
/// </summary>
internal static partial class ImdbIds
{
    /// <summary>The canonical lower-case id, or null when the value is not one.</summary>
    public static string? Normalize(string? value)
    {
        var candidate = value?.Trim().ToLowerInvariant();
        return candidate is not null && TitleId().IsMatch(candidate) ? candidate : null;
    }

    [GeneratedRegex("^tt[0-9]{5,12}$", RegexOptions.CultureInvariant)]
    private static partial Regex TitleId();
}
