using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The one set of type-tolerant reads for upstream JSON. A field with an
/// unexpected type is reported as absent instead of throwing, so one malformed
/// provider value cannot fail a whole request. Numbers are read from their
/// JSON text, which works for parsed documents and constructed nodes alike.
/// </summary>
internal static class JsonRead
{
    private const int MaxNumericTextLength = 32;

    /// <summary>A JSON string, or null for any other kind.</summary>
    public static string? String(JsonNode? node)
        => node is JsonValue value
            && value.GetValueKind() == JsonValueKind.String
            && value.TryGetValue<string>(out var text)
                ? text
                : null;

    public static string? String(JsonObject? value, string name) => String(value?[name]);

    /// <summary>A strict JSON boolean; other kinds are null.</summary>
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

    /// <summary>An integral JSON number that fits in an <see cref="int"/>.</summary>
    public static int? Int32(JsonNode? node)
        => NumberText(node) is { } text
            && int.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;

    public static int? Int32(JsonObject? value, string name) => Int32(value?[name]);

    /// <summary>An integral JSON number that fits in an unsigned 64-bit mask.</summary>
    public static ulong? UInt64(JsonNode? node)
        => NumberText(node) is { } text
            && ulong.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;

    public static ulong? UInt64(JsonObject? value, string name) => UInt64(value?[name]);

    /// <summary>
    /// A whole number sent either as an integral JSON number or as a short
    /// numeric string, which several providers use for ids and counters.
    /// </summary>
    public static long? Integer(JsonNode? node)
    {
        var text = NumberText(node) ?? String(node)?.Trim();
        return text is { Length: > 0 and <= MaxNumericTextLength }
            && long.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var number)
                ? number
                : null;
    }

    /// <summary>A strictly positive <see cref="Integer"/>, as used for provider ids.</summary>
    public static long? Positive(JsonNode? node) => Integer(node) is > 0 and var id ? id : null;

    /// <summary>A finite number sent as a JSON number or a short numeric string.</summary>
    public static double? Number(JsonNode? node)
    {
        var text = NumberText(node) ?? String(node)?.Trim();
        return text is { Length: > 0 and <= MaxNumericTextLength }
            && double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number)
            && double.IsFinite(number)
                ? number
                : null;
    }

    /// <summary>The four-digit year that starts an ISO-style date string.</summary>
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
