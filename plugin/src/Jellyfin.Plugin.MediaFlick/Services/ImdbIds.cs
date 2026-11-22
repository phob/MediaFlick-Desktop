using System.Text.RegularExpressions;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal static partial class ImdbIds
{
    public static string? Normalize(string? value)
    {
        var candidate = value?.Trim().ToLowerInvariant();
        return candidate is not null && TitleId().IsMatch(candidate) ? candidate : null;
    }

    [GeneratedRegex("^tt[0-9]{5,12}$", RegexOptions.CultureInvariant)]
    private static partial Regex TitleId();
}
