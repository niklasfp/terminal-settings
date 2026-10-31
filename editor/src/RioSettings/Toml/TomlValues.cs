using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using Tomlyn.Syntax;

namespace RioSettings.Toml;

/// <summary>Converts between Tomlyn syntax values, JSON nodes and TOML literals.</summary>
internal static class TomlValues
{
    public static JsonNode? ToJson(ValueSyntax value) => value switch
    {
        StringValueSyntax s => JsonValue.Create(s.Value),
        IntegerValueSyntax i => JsonValue.Create(i.Value),
        FloatValueSyntax f => JsonValue.Create(f.Value),
        BooleanValueSyntax b => JsonValue.Create(b.Value),
        DateTimeValueSyntax d => JsonValue.Create(d.ToString().Trim()),
        ArraySyntax a => new JsonArray([.. a.Items.Select(item => ToJson(item.Value!))]),
        InlineTableSyntax t => InlineTable(t),
        _ => JsonValue.Create(value.ToString().Trim()),
    };

    static JsonObject InlineTable(InlineTableSyntax table)
    {
        var obj = new JsonObject();
        foreach (var item in table.Items)
        {
            var kv = item.KeyValue!;
            var parts = new List<string> { Part(kv.Key!.Key!) };
            foreach (var dot in kv.Key.DotKeys) parts.Add(Part(dot.Key!));
            var target = obj;
            for (int i = 0; i < parts.Count - 1; i++)
            {
                if (target[parts[i]] is not JsonObject next)
                {
                    next = new JsonObject();
                    target[parts[i]] = next;
                }
                target = next;
            }
            target[parts[^1]] = ToJson(kv.Value!);
        }
        return obj;
    }

    static string Part(SyntaxNode node) => node switch
    {
        BareKeySyntax bare => bare.Key!.Text!,
        StringValueSyntax str => str.Value!,
        _ => node.ToString().Trim(),
    };

    public static string Quote(string s)
    {
        var sb = new StringBuilder("\"");
        foreach (char c in s)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                case '\b': sb.Append("\\b"); break;
                case '\f': sb.Append("\\f"); break;
                default:
                    // \e is TOML 1.1 only; Rio's parser is TOML 1.0, so always use \u.
                    if (c < 0x20 || c == 0x7f) sb.Append("\\u").Append(((int)c).ToString("X4", CultureInfo.InvariantCulture));
                    else sb.Append(c);
                    break;
            }
        }
        return sb.Append('"').ToString();
    }

    public static string Float(double d)
    {
        if (double.IsNaN(d)) return "nan";
        if (double.IsPositiveInfinity(d)) return "inf";
        if (double.IsNegativeInfinity(d)) return "-inf";
        var r = d.ToString("R", CultureInfo.InvariantCulture);
        if (r.Contains('E')) r = d.ToString("0.0###############", CultureInfo.InvariantCulture);
        return r.Contains('.') ? r : r + ".0";
    }

    public static string Int(long l) => l.ToString(CultureInfo.InvariantCulture);
}
