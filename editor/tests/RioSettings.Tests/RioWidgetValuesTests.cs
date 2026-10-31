using System.Diagnostics;
using System.Text.Json.Nodes;
using Settings.Core;
using Settings.Generated;

namespace RioSettings.Tests;

public class RioWidgetValuesTests
{
    // Built by `cargo build --release` in tools/rio/schema-dump; it parses a file with Rio's own Config.
    static readonly string Validator = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory,
        "../../../../../tools/rio/schema-dump/target/release/schema-dump"));

    static async Task<string?> RioRejects(string toml)
    {
        var file = Path.Combine(Path.GetTempPath(), $"rio-widgets-{Guid.NewGuid():n}.toml");
        await File.WriteAllTextAsync(file, toml);
        try
        {
            var psi = new ProcessStartInfo(Validator, ["validate", file]) { RedirectStandardError = true, RedirectStandardOutput = true };
            using var p = Process.Start(psi)!;
            var err = await p.StandardError.ReadToEndAsync();
            await p.WaitForExitAsync();
            return p.ExitCode == 0 ? null : err.Trim();
        }
        finally
        {
            File.Delete(file);
        }
    }

    static string Write(params (Setting setting, JsonNode? value)[] changes) =>
        new RioTomlFormat().Apply("", [.. changes.Select(c => new SettingChange(c.setting, c.value))], "");

    /// <summary>Every value a widget can produce must be something Rio itself accepts.</summary>
    [Test]
    public async Task Every_widget_value_is_accepted_by_rio()
    {
        if (!File.Exists(Validator)) return;
        var settings = AppSchema.Settings.Where(s => s.Type != SettingType.Raw).ToList();

        // All defaults together.
        var defaults = Write([.. settings.Where(s => s.Default is not null).Select(s => (s, s.Default))]);
        await Assert.That(await RioRejects(defaults)).IsNull().Because("defaults:\n" + defaults);

        // Each choice / flag / on-off value on its own (TOML keys can't repeat).
        foreach (var s in settings)
        {
            IEnumerable<JsonNode> values = s.Type switch
            {
                SettingType.Enum => (s.Values ?? []).Select(v => (JsonNode)JsonValue.Create(v)!),
                SettingType.Flags => (s.Values ?? []).Select(v => (JsonNode)new JsonArray(v)),
                SettingType.Bool => [JsonValue.Create(true)!, JsonValue.Create(false)!],
                SettingType.Blur => [JsonValue.Create(false)!, JsonValue.Create(true)!, JsonValue.Create("macos-glass-regular")!, JsonValue.Create("macos-glass-clear")!],
                SettingType.FontStyle => [JsonValue.Create(false)!, JsonValue.Create("Bold")!],
                SettingType.Margin => [new JsonArray(4), new JsonArray(4, 8), new JsonArray(1, 2, 3, 4)],
                _ => [],
            };
            foreach (var v in values)
            {
                var toml = Write((s, v));
                await Assert.That(await RioRejects(toml)).IsNull().Because($"{s.Key} = {v.ToJsonString()}:\n{toml}");
            }
        }
    }
}
