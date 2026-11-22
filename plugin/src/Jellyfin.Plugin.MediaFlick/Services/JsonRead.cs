using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal static class JsonRead
{
    private const int MaxNumericTextLength = 32;

    public static string? String(JsonNode? node)
        => node is JsonValue value
            && value.GetValueKind() == JsonValueKind.String
            && value.TryGetValue<string>(out var text)
                ? text
                : null;

    public static string? String(JsonObject? value, string name) => String(value?[name]);

    public static bool? Bool(JsonNode? node)
        => node is JsonValue value
            ? value.GetValueKind() switch
            {
                JsonValueKind.True => true,
                JsonValueKind.False => false,
                _ => null
            }
            : null;

    public static bool? Bool(JsonObject? value, string name) => Bool(value?[name]);

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
                return Number(node) is { } number ? number != 0 : null;
            case JsonValueKind.String:
                var text = String(node)?.Trim();
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

    public static int? Int32(JsonNode? node)
        => NumberText(node) is { } text
            && int.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;

    public static int? Int32(JsonObject? value, string name) => Int32(value?[name]);

    public static ulong? UInt64(JsonNode? node)
        => NumberText(node) is { } text
            && ulong.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;

    public static ulong? UInt64(JsonObject? value, string name) => UInt64(value?[name]);

    public static long? Integer(JsonNode? node)
    {
        var text = NumberText(node) ?? String(node)?.Trim();
        return text is { Length: > 0 and <= MaxNumericTextLength }
            && long.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;
    }

    public static long? Positive(JsonNode? node) => Integer(node) is > 0 and var id ? id : null;

    public static double? Number(JsonNode? node)
    {
        var text = NumberText(node) ?? String(node)?.Trim();
        return text is { Length: > 0 and <= MaxNumericTextLength }
            && double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number)
            && double.IsFinite(number)
                ? number
                : null;
    }

    public static int? Year(string? date)
        => date is { Length: >= 4 }
            && int.TryParse(date.AsSpan(0, 4), NumberStyles.None, CultureInfo.InvariantCulture, out var year)
                ? year
                : null;

    private static string? NumberText(JsonNode? node)
        => node is JsonValue value && value.GetValueKind() == JsonValueKind.Number
            ? value.ToJsonString()
            : null;
}
