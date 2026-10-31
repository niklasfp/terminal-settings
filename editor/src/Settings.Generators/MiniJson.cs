using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace Settings.Generators;

/// <summary>Ordered JSON object. Generators can't easily ship System.Text.Json, so this is a tiny reader/writer.</summary>
internal sealed class JObj
{
    public List<KeyValuePair<string, object?>> Members { get; } = new();

    public object? this[string key]
    {
        get
        {
            foreach (var kv in Members)
                if (kv.Key == key) return kv.Value;
            return null;
        }
    }

    public bool Has(string key)
    {
        foreach (var kv in Members)
            if (kv.Key == key) return true;
        return false;
    }

    public string? Str(string key) => this[key] as string;
    public bool Bool(string key) => this[key] is bool b && b;
    public double? Num(string key) => this[key] switch { double d => d, long l => l, _ => null };
    public JObj? Obj(string key) => this[key] as JObj;
    public List<object?>? Arr(string key) => this[key] as List<object?>;
}

internal static class MiniJson
{
    public static object? Parse(string text)
    {
        int i = 0;
        var v = Value(text, ref i);
        Ws(text, ref i);
        if (i != text.Length) throw Error(text, i, "trailing content");
        return v;
    }

    static FormatException Error(string s, int i, string msg)
    {
        int line = 1;
        for (int k = 0; k < i && k < s.Length; k++) if (s[k] == '\n') line++;
        return new FormatException($"JSON error at line {line}: {msg}");
    }

    static void Ws(string s, ref int i)
    {
        while (i < s.Length && char.IsWhiteSpace(s[i])) i++;
    }

    static object? Value(string s, ref int i)
    {
        Ws(s, ref i);
        if (i >= s.Length) throw Error(s, i, "unexpected end");
        char c = s[i];
        switch (c)
        {
            case '{':
            {
                i++;
                var o = new JObj();
                Ws(s, ref i);
                if (s[i] == '}') { i++; return o; }
                while (true)
                {
                    Ws(s, ref i);
                    var key = Str(s, ref i);
                    Ws(s, ref i);
                    if (s[i] != ':') throw Error(s, i, "expected ':'");
                    i++;
                    o.Members.Add(new KeyValuePair<string, object?>(key, Value(s, ref i)));
                    Ws(s, ref i);
                    if (s[i] == ',') { i++; continue; }
                    if (s[i] == '}') { i++; return o; }
                    throw Error(s, i, "expected ',' or '}'");
                }
            }
            case '[':
            {
                i++;
                var a = new List<object?>();
                Ws(s, ref i);
                if (s[i] == ']') { i++; return a; }
                while (true)
                {
                    a.Add(Value(s, ref i));
                    Ws(s, ref i);
                    if (s[i] == ',') { i++; continue; }
                    if (s[i] == ']') { i++; return a; }
                    throw Error(s, i, "expected ',' or ']'");
                }
            }
            case '"':
                return Str(s, ref i);
            case 't' when string.CompareOrdinal(s, i, "true", 0, 4) == 0: i += 4; return true;
            case 'f' when string.CompareOrdinal(s, i, "false", 0, 5) == 0: i += 5; return false;
            case 'n' when string.CompareOrdinal(s, i, "null", 0, 4) == 0: i += 4; return null;
            default:
            {
                int start = i;
                bool isFloat = false;
                while (i < s.Length && "+-0123456789.eE".IndexOf(s[i]) >= 0)
                {
                    if (s[i] == '.' || s[i] == 'e' || s[i] == 'E') isFloat = true;
                    i++;
                }
                if (start == i) throw Error(s, i, $"unexpected '{c}'");
                var num = s.Substring(start, i - start);
                if (!isFloat && long.TryParse(num, NumberStyles.Integer, CultureInfo.InvariantCulture, out var l)) return l;
                return double.Parse(num, NumberStyles.Float, CultureInfo.InvariantCulture);
            }
        }
    }

    static string Str(string s, ref int i)
    {
        if (s[i] != '"') throw Error(s, i, "expected string");
        i++;
        var sb = new StringBuilder();
        while (s[i] != '"')
        {
            char c = s[i++];
            if (c != '\\') { sb.Append(c); continue; }
            char e = s[i++];
            switch (e)
            {
                case 'n': sb.Append('\n'); break;
                case 't': sb.Append('\t'); break;
                case 'r': sb.Append('\r'); break;
                case 'b': sb.Append('\b'); break;
                case 'f': sb.Append('\f'); break;
                case 'u':
                    sb.Append((char)int.Parse(s.Substring(i, 4), NumberStyles.HexNumber, CultureInfo.InvariantCulture));
                    i += 4;
                    break;
                default: sb.Append(e); break;
            }
        }
        i++;
        return sb.ToString();
    }

    /// <summary>Serialises the object model back to compact JSON.</summary>
    public static string Write(object? v)
    {
        var sb = new StringBuilder();
        Write(sb, v);
        return sb.ToString();
    }

    public static void Write(StringBuilder sb, object? v)
    {
        switch (v)
        {
            case null: sb.Append("null"); break;
            case bool b: sb.Append(b ? "true" : "false"); break;
            case long l: sb.Append(l.ToString(CultureInfo.InvariantCulture)); break;
            case int n: sb.Append(n.ToString(CultureInfo.InvariantCulture)); break;
            case double d: sb.Append(FormatDouble(d)); break;
            case string s: WriteString(sb, s); break;
            case JObj o:
                sb.Append('{');
                for (int k = 0; k < o.Members.Count; k++)
                {
                    if (k > 0) sb.Append(',');
                    WriteString(sb, o.Members[k].Key);
                    sb.Append(':');
                    Write(sb, o.Members[k].Value);
                }
                sb.Append('}');
                break;
            case System.Collections.IEnumerable e:
                sb.Append('[');
                bool first = true;
                foreach (var item in e)
                {
                    if (!first) sb.Append(',');
                    first = false;
                    Write(sb, item);
                }
                sb.Append(']');
                break;
            default: throw new InvalidOperationException("Cannot write " + v.GetType());
        }
    }

    public static string FormatDouble(double d)
    {
        var r = d.ToString("R", CultureInfo.InvariantCulture);
        return r.IndexOfAny(new[] { '.', 'E', 'e' }) >= 0 ? r : r + ".0";
    }

    public static void WriteString(StringBuilder sb, string s)
    {
        sb.Append('"');
        foreach (char c in s)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                // Keep the JSON safe to embed inside a <script> element.
                case '<': sb.Append("\\u003c"); break;
                case '>': sb.Append("\\u003e"); break;
                case '&': sb.Append("\\u0026"); break;
                default:
                    if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
    }
}
