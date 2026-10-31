using System.Text.Json;
using System.Text.Json.Nodes;
using RioSettings.Toml;
using Settings.Core;

namespace RioSettings;

/// <summary>Rio's config.toml: span-splicing edits over Tomlyn's lossless syntax tree.</summary>
internal sealed class RioTomlFormat : IConfigFormat
{
    public ParsedConfig Parse(string text, SettingCatalog settings)
    {
        try
        {
            var doc = ConfigDocument.Parse(text);
            var raw = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var s in settings.All.Where(s => s.Type == SettingType.Raw))
                raw[s.Key] = doc.GetTableArray(s.Path);
            return new ParsedConfig(doc.Values(), raw);
        }
        catch (TomlEditException ex)
        {
            throw new ConfigFormatException(ex.Message);
        }
    }

    public string Apply(string text, IReadOnlyList<SettingChange> changes, string scope)
    {
        ConfigDocument doc;
        try { doc = ConfigDocument.Parse(text); }
        catch (TomlEditException ex) { throw new ConfigFormatException(ex.Message); }

        var errors = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (setting, value) in changes)
        {
            string[] path = scope.Length > 0 ? ["platform", scope, .. setting.Path] : setting.Path;
            try
            {
                if (setting.Type == SettingType.Raw)
                    doc.SetTableArray(path, value?.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : "");
                else if (value is null)
                    doc.Remove(path);
                else
                    doc.Set(path, SettingValues.ToToml(setting, value));
            }
            catch (SettingValueException ex) { errors[setting.Key] = ex.Message; }
            catch (TomlEditException ex) { errors[setting.Key] = ex.Message; }
        }
        if (errors.Count > 0) throw new ChangeErrors(errors);
        return doc.Text;
    }
}
