using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace Settings.Generators;

internal sealed class FieldDef
{
    public string Key = "";
    public string Label = "";
    public string Type = "string";
    public string? Placeholder;
    /// <summary>Datalist id for suggestions (e.g. "action-list").</summary>
    public string? List;
}

internal sealed class SettingDef
{
    public string Key = "";
    public string Group = "";
    public string? Subgroup;
    public string Label = "";
    public string Type = "string";
    public string? Doc;
    public double? Min, Max, Step;
    public string? Unit;
    public string? Placeholder;
    public List<string>? Values;
    public bool Nullable;
    public List<string>? Platforms;
    public object? Default;
    public bool Overridable;
    public bool Inferred;
    public bool Repeatable;
    /// <summary>Longer markdown docs shown behind "More".</summary>
    public string? DocMore;
    /// <summary>Short badges such as "Restart needed" or "Since 1.3.0".</summary>
    public List<string> Tags = new();
    /// <summary>For repeatable keys whose built-in defaults are too long to list.</summary>
    public string? BuiltInsNote;
    /// <summary>Native dialog for path fields: "image", "audio", "file" or "folder".</summary>
    public string? Pick;
    /// <summary>Short free-text values (durations, "10%", "2,4"): rendered as a compact field.</summary>
    public bool Compact;
    public List<FieldDef>? Fields;
}

internal sealed class GroupDef
{
    public string Id = "";
    public string Title = "";
    public string Icon = "";
    public string? Doc;
}

internal sealed class SchemaDoc
{
    /// <summary>Source revision the schema was generated from (git commit or version).</summary>
    public string Revision = "";
    public string Terminal = "";
    public string Title = "";
    public string Logo = "";
    /// <summary>Platform scopes for per-platform overrides ("Applies to"); empty hides the control.</summary>
    public List<string> Scopes = new();
    public bool ThemeLocksColors;
    /// <summary>Brand id ("rio", "ghostty"): selects the palette and shows the app's own logo (served at /logo).</summary>
    public string Brand = "";
    public List<GroupDef> Groups = new();
    public List<SettingDef> Settings = new();
}

internal readonly struct SchemaDiagnostic
{
    public SchemaDiagnostic(string id, bool isError, string message)
    {
        Id = id;
        IsError = isError;
        Message = message;
    }

    public string Id { get; }
    public bool IsError { get; }
    public string Message { get; }
}

/// <summary>
/// Merges the generated layer (rio-source.json: Rio's defaults dump, enums, color defaults)
/// with the hand-written layer (rio-overlay.json: labels, docs, ranges, widgets).
/// </summary>
internal static class RioSchemaMerger
{
    static readonly HashSet<string> KnownTypes = new(StringComparer.Ordinal)
    {
        "bool", "int", "float", "string", "path", "font", "theme", "enum", "color",
        "strings", "flags", "margin", "pair", "blur", "fontStyle", "records", "raw", "fonts",
    };

    // Keys in the dump that are containers handled elsewhere, not settings.
    static readonly HashSet<string> IgnoredDumpKeys = new(StringComparer.Ordinal) { "platform" };

    public static SchemaDoc Merge(string sourceJson, string overlayJson, List<SchemaDiagnostic> diags)
    {
        var source = (JObj)MiniJson.Parse(sourceJson)!;
        var overlay = (JObj)MiniJson.Parse(overlayJson)!;
        var defaults = source.Obj("defaults") ?? new JObj();
        var enums = source.Obj("enums") ?? new JObj();
        var colorDefaults = source.Obj("colorDefaults") ?? new JObj();

        var schema = new SchemaDoc { Revision = source.Str("rioRevision") ?? "unknown" };
        ReadApp(schema, overlay);

        foreach (var g in overlay.Arr("groups") ?? new List<object?>())
        {
            var go = (JObj)g!;
            schema.Groups.Add(new GroupDef
            {
                Id = go.Str("id")!,
                Title = go.Str("title")!,
                Icon = go.Str("icon") ?? "",
                Doc = go.Str("doc"),
            });
        }
        var groupIds = new HashSet<string>(schema.Groups.Select(g => g.Id));

        var overridePrefixes = (overlay.Arr("platformOverridable") ?? new List<object?>()).OfType<string>().ToList();
        var overrideExclude = new HashSet<string>((overlay.Arr("platformOverrideExclude") ?? new List<object?>()).OfType<string>());
        bool IsOverridable(string key) =>
            !overrideExclude.Contains(key) &&
            overridePrefixes.Any(p => p.EndsWith(".") ? key.StartsWith(p, StringComparison.Ordinal) : key == p);

        var seen = new HashSet<string>(StringComparer.Ordinal);

        foreach (var s in overlay.Arr("settings") ?? new List<object?>())
        {
            var so = (JObj)s!;
            var def = new SettingDef
            {
                Key = so.Str("key")!,
                Group = so.Str("group") ?? "general",
                Subgroup = so.Str("subgroup"),
                Label = so.Str("label") ?? "",
                Type = so.Str("type") ?? "string",
                Doc = so.Str("doc"),
                Min = so.Num("min"),
                Max = so.Num("max"),
                Step = so.Num("step"),
                Unit = so.Str("unit"),
                Placeholder = so.Str("placeholder"),
                Nullable = so.Bool("nullable"),
                Platforms = so.Arr("platforms")?.OfType<string>().ToList(),
            };
            def.Pick = so.Str("pick");
            if (!seen.Add(def.Key))
            {
                diags.Add(new SchemaDiagnostic("RIO005", true, $"Setting '{def.Key}' is listed twice in rio-overlay.json."));
                continue;
            }
            if (!KnownTypes.Contains(def.Type))
                diags.Add(new SchemaDiagnostic("RIO006", true, $"Setting '{def.Key}' has unknown type '{def.Type}'."));
            if (!groupIds.Contains(def.Group))
                diags.Add(new SchemaDiagnostic("RIO007", true, $"Setting '{def.Key}' uses unknown group '{def.Group}'."));

            def.Values = ResolveValues(def.Key, so, enums, diags);
            def.Overridable = IsOverridable(def.Key);

            if (so.Arr("fields") is { } fields)
            {
                def.Fields = fields.OfType<JObj>().Select(f => new FieldDef
                {
                    Key = f.Str("key")!,
                    Label = f.Str("label") ?? f.Str("key")!,
                    Type = f.Str("type") ?? "string",
                    Placeholder = f.Str("placeholder"),
                }).ToList();
            }

            bool inDump = TryGet(defaults, def.Key, out var dumped);
            bool manual = so.Bool("manual");
            if (so.Has("default")) def.Default = so["default"];
            else if (inDump) def.Default = dumped;

            if (!inDump && !manual)
                diags.Add(new SchemaDiagnostic("RIO003", false,
                    $"Setting '{def.Key}' is not in Rio's defaults dump. It may have been removed from Rio; mark it \"manual\": true if it is skipped on purpose."));
            if (inDump && manual)
                diags.Add(new SchemaDiagnostic("RIO008", false,
                    $"Setting '{def.Key}' is marked manual but Rio's defaults dump now has it; drop \"manual\"."));

            def.Default = NormalizeDefault(def, def.Default, enums);
            schema.Settings.Add(def);
        }

        // Colors come entirely from the source layer.
        AddColors(schema, overlay, colorDefaults, seen);

        // Anything Rio has that the overlay doesn't know: warn, but still render it.
        foreach (var (key, value) in Leaves(defaults, "", seen))
        {
            diags.Add(new SchemaDiagnostic("RIO001", false,
                $"Rio setting '{key}' has no entry in rio-overlay.json; it is shown with a basic control."));
            var group = key.Split('.')[0];
            var def = new SettingDef
            {
                Key = key,
                Group = groupIds.Contains(group) ? group : "general",
                Subgroup = "Other",
                Label = Humanize(key.Substring(key.LastIndexOf('.') + 1)),
                Type = InferType(value),
                Inferred = true,
                Nullable = value is null,
                Overridable = IsOverridable(key),
            };
            def.Default = NormalizeDefault(def, value, enums);
            schema.Settings.Add(def);
        }

        return schema;
    }

    static List<string>? ResolveValues(string key, JObj so, JObj enums, List<SchemaDiagnostic> diags)
    {
        var explicitValues = so.Arr("values")?.OfType<string>().ToList();
        var enumName = so.Str("enum");
        if (enumName is null) return explicitValues;

        if (enums.Obj(enumName) is not { } e)
        {
            diags.Add(new SchemaDiagnostic("RIO002", true, $"Setting '{key}' references enum '{enumName}', which was not found in Rio's source."));
            return explicitValues;
        }

        var fromSource = EnumValues(e);
        if (explicitValues is null) return fromSource;

        foreach (var v in explicitValues)
            if (!fromSource.Contains(v, StringComparer.OrdinalIgnoreCase))
                diags.Add(new SchemaDiagnostic("RIO004", false,
                    $"Setting '{key}' lists value '{v}', which Rio's enum {enumName} no longer has (has: {string.Join(", ", fromSource)})."));
        foreach (var v in fromSource)
            if (!explicitValues.Contains(v, StringComparer.OrdinalIgnoreCase))
                diags.Add(new SchemaDiagnostic("RIO004", false,
                    $"Rio's enum {enumName} has value '{v}' that setting '{key}' does not list."));
        return explicitValues;
    }

    /// <summary>The spelling users write in TOML: the first serde alias, else the (renamed) variant name.</summary>
    static List<string> EnumValues(JObj e)
    {
        var renameAll = e.Str("renameAll");
        var list = new List<string>();
        foreach (var v in (e.Arr("variants") ?? new List<object?>()).OfType<JObj>())
        {
            var aliases = v.Arr("aliases")?.OfType<string>().ToList();
            var name = v.Str("name")!;
            list.Add(aliases is { Count: > 0 } ? aliases[0] : ApplyRenameAll(name, renameAll));
        }
        return list;
    }

    static string ApplyRenameAll(string name, string? renameAll) => renameAll switch
    {
        "lowercase" => name.ToLowerInvariant(),
        "kebab-case" => string.Concat(name.Select((c, i) => char.IsUpper(c) && i > 0 ? "-" + char.ToLowerInvariant(c) : char.ToLowerInvariant(c).ToString())),
        _ => name,
    };

    static object? NormalizeDefault(SettingDef def, object? value, JObj enums)
    {
        switch (def.Type)
        {
            case "enum" when value is string s && def.Values is { } values:
                // The dump writes variant names (e.g. "Block"); map to the TOML spelling ("block").
                foreach (var v in values)
                    if (string.Equals(v, s, StringComparison.OrdinalIgnoreCase)) return v;
                foreach (var e in enums.Members.Select(m => m.Value).OfType<JObj>())
                {
                    var variants = (e.Arr("variants") ?? new List<object?>()).OfType<JObj>().ToList();
                    var hit = variants.FindIndex(x => x.Str("name") == s);
                    if (hit >= 0)
                    {
                        var spelled = EnumValues(e)[hit];
                        if (values.Contains(spelled)) return spelled;
                    }
                }
                return s;
            case "float" when value is long l:
                return (double)l;
            case "margin" when value is JObj m:
            {
                double t = m.Num("top") ?? 0, r = m.Num("right") ?? 0, b = m.Num("bottom") ?? 0, lft = m.Num("left") ?? 0;
                if (t == r && r == b && b == lft) return new List<object?> { t };
                if (t == b && r == lft) return new List<object?> { t, r };
                return new List<object?> { t, r, b, lft };
            }
            case "string" when value is null && !def.Nullable:
                return "";
            case "raw":
                // Raw blocks are edited as TOML text; empty means "Rio's built-in value".
                return null;
            default:
                return value;
        }
    }

    static void AddColors(SchemaDoc schema, JObj overlay, JObj colorDefaults, HashSet<string> seen)
    {
        var subgroups = (overlay.Arr("colorSubgroups") ?? new List<object?>()).OfType<JObj>().ToList();
        var placed = new List<(int order, int idx, SettingDef def)>();
        int idx = 0;
        foreach (var kv in colorDefaults.Members)
        {
            var key = "colors." + kv.Key;
            if (!seen.Add(key)) continue;
            int order = subgroups.Count;
            string? title = "Other";
            string label = kv.Key;
            for (int i = 0; i < subgroups.Count; i++)
            {
                var sg = subgroups[i];
                var match = sg.Arr("match")?.OfType<string>().ToList();
                var prefix = sg.Str("prefix");
                if (match?.Contains(kv.Key) == true)
                {
                    order = i; title = sg.Str("title"); break;
                }
                if (prefix is not null && kv.Key.StartsWith(prefix, StringComparison.Ordinal))
                {
                    order = i; title = sg.Str("title"); label = kv.Key.Substring(prefix.Length); break;
                }
            }
            placed.Add((order, idx++, new SettingDef
            {
                Key = key,
                Group = "colors",
                Subgroup = title,
                Label = Humanize(label),
                Type = "color",
                Default = kv.Value,
                // Dim/light-foreground etc. have no fixed default: Rio derives them.
                Nullable = kv.Value is null,
            }));
        }
        // Keep "Normal" colors in ANSI order: subgroup order, then the order listed in `match`.
        foreach (var p in placed.OrderBy(p => p.order).ThenBy(p => MatchIndex(subgroups, p.order, p.def.Key)).ThenBy(p => p.idx))
            schema.Settings.Add(p.def);
    }

    static int MatchIndex(List<JObj> subgroups, int order, string key)
    {
        if (order >= subgroups.Count) return 0;
        var match = subgroups[order].Arr("match")?.OfType<string>().ToList();
        var i = match?.IndexOf(key.Substring("colors.".Length)) ?? -1;
        return i < 0 ? int.MaxValue : i;
    }

    static bool TryGet(JObj root, string dottedKey, out object? value)
    {
        object? cur = root;
        foreach (var part in dottedKey.Split('.'))
        {
            if (cur is not JObj o || !o.Has(part))
            {
                value = null;
                return false;
            }
            cur = o[part];
        }
        value = cur;
        return true;
    }

    static IEnumerable<(string key, object? value)> Leaves(JObj o, string prefix, HashSet<string> known)
    {
        foreach (var kv in o.Members)
        {
            var key = prefix.Length == 0 ? kv.Key : prefix + "." + kv.Key;
            if (IgnoredDumpKeys.Contains(key) || known.Contains(key)) continue;
            // A known setting further down means this is just a table.
            if (kv.Value is JObj child && known.Any(k => k.StartsWith(key + ".", StringComparison.Ordinal)))
            {
                foreach (var leaf in Leaves(child, key, known)) yield return leaf;
                continue;
            }
            if (kv.Value is JObj nested && nested.Members.Count > 0 && !LooksLikeValue(nested))
            {
                foreach (var leaf in Leaves(nested, key, known)) yield return leaf;
                continue;
            }
            yield return (key, kv.Value);
        }
    }

    static bool LooksLikeValue(JObj o) => o.Has("top") && o.Has("left");

    static string InferType(object? v) => v switch
    {
        bool => "bool",
        long => "int",
        double => "float",
        List<object?> => "strings",
        JObj o when LooksLikeValue(o) => "margin",
        _ => "string",
    };

    static void ReadApp(SchemaDoc schema, JObj overlay) => SchemaCommon.ReadApp(schema, overlay);

    internal static string Humanize(string key) => SchemaCommon.Humanize(key);
}

/// <summary>Helpers shared by every terminal's schema merger.</summary>
internal static class SchemaCommon
{
    /// <summary>Reads the overlay's "app" block: name, title, logo and UI capabilities.</summary>
    internal static void ReadApp(SchemaDoc schema, JObj overlay)
    {
        var app = overlay.Obj("app") ?? new JObj();
        schema.Terminal = app.Str("terminal") ?? "Terminal";
        schema.Title = app.Str("title") ?? schema.Terminal + " settings";
        schema.Logo = app.Str("logo") ?? "";
        schema.Scopes = app.Arr("scopes")?.OfType<string>().ToList() ?? new List<string>();
        schema.ThemeLocksColors = app.Bool("themeLocksColors");
        schema.Brand = app.Str("brand") ?? "";
    }

    internal static string Humanize(string key)
    {
        var words = key.Replace('_', '-').Split(new[] { '-' }, StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0) return key;
        words[0] = char.ToUpper(words[0][0], CultureInfo.InvariantCulture) + words[0].Substring(1);
        return string.Join(" ", words);
    }
}
