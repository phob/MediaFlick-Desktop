using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Type-tolerant reads of upstream JSON. A field with an unexpected type is
/// reported as absent instead of throwing, so one malformed provider value
/// cannot fail a whole request.
/// </summary>
internal static class JsonRead
{
    /// <summary>
    /// A boolean that providers variously encode as a JSON boolean, a 0/1
    /// number, or a "true"/"false"/"0"/"1" string. Anything else is null.
    /// </summary>
    public static bool? Flag(JsonNode? node)
    {
        if (node is not JsonValue value)
        {
            return null;
        }

        switch (value.GetValueKind())
        {
            case JsonValueKind.True:
                return true;
            case JsonValueKind.False:
                return false;
            case JsonValueKind.Number:
                // The JSON text works for both parsed and constructed values;
                // TryGetValue only converts between identical CLR types.
                return double.TryParse(
                    value.ToJsonString(),
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var number) && double.IsFinite(number)
                    ? number != 0
                    : null;
            case JsonValueKind.String:
                var text = value.GetValue<string>().Trim();
                if (bool.TryParse(text, out var parsed))
                {
                    return parsed;
                }

                return long.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var flag)
                    ? flag != 0
                    : null;
            default:
                return null;
        }
    }
}
