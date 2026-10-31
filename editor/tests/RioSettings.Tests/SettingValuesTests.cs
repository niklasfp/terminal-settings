using System.Text.Json.Nodes;
using RioSettings;
using Settings.Core;
using Settings.Generated;

namespace RioSettings.Tests;

public class SettingValuesTests
{
    static readonly SettingCatalog Catalog = new(AppSchema.Settings);

    static string Toml(string key, string json) => SettingValues.ToToml(Catalog.Find(key)!, JsonNode.Parse(json));

    [Test]
    [Arguments("window.opacity", "0.5", "0.5")]
    [Arguments("window.opacity", "1", "1.0")]
    [Arguments("fonts.size", "14", "14.0")]
    [Arguments("window.width", "1200", "1200")]
    [Arguments("cursor.blinking", "true", "true")]
    [Arguments("cursor.shape", "\"beam\"", "\"beam\"")]
    [Arguments("colors.background", "\"1E1E2E\"", "\"#1e1e2e\"")]
    [Arguments("colors.background", "\"#1e1e2ecc\"", "\"#1e1e2ecc\"")]
    [Arguments("env-vars", "[\"A=1\",\"B=\\\"x\\\"\"]", "[\"A=1\", \"B=\\\"x\\\"\"]")]
    [Arguments("margin", "[10, 20]", "[10, 20]")]
    [Arguments("effects.trail-cursor-decay", "[100, 400]", "[100, 400]")]
    [Arguments("window.blur", "\"macos-glass-clear\"", "\"macos-glass-clear\"")]
    [Arguments("window.blur", "false", "false")]
    [Arguments("fonts.regular.style", "false", "false")]
    [Arguments("fonts.regular.style", "\"\"", "\"default\"")]
    [Arguments("keyboard.forward-to-ime-modifier-mask", "[\"shift\",\"alt\"]", "[\"shift\", \"alt\"]")]
    public async Task Converts_to_toml(string key, string json, string expected)
    {
        await Assert.That(Toml(key, json)).IsEqualTo(expected);
    }

    [Test]
    [Arguments("window.opacity", "1.5")]
    [Arguments("cursor.blinking-interval", "100")]
    [Arguments("window.width", "100.5")]
    [Arguments("cursor.shape", "\"triangle\"")]
    [Arguments("colors.background", "\"red\"")]
    [Arguments("margin", "[1, 2, 3]")]
    [Arguments("window.blur", "\"frosted\"")]
    [Arguments("keyboard.forward-to-ime-modifier-mask", "[\"hyper\"]")]
    [Arguments("cursor.blinking", "\"yes\"")]
    public async Task Rejects_invalid_values(string key, string json)
    {
        await Assert.That(() => Toml(key, json)).Throws<SettingValueException>();
    }

    [Test]
    public async Task Records_become_one_inline_table_per_line_and_skip_empty_fields()
    {
        var toml = Toml("bindings.keys", """[{"key":"t","with":"super","action":"CreateTab","esc":""},{"key":"w","with":"super","action":"CloseTab"}]""");
        await Assert.That(toml).IsEqualTo("""
            [
              { key = "t", with = "super", action = "CreateTab" },
              { key = "w", with = "super", action = "CloseTab" },
            ]
            """);
    }

    [Test]
    public async Task Control_characters_use_unicode_escapes_not_toml_1_1_escapes()
    {
        var toml = Toml("title.content", "\"a\\u001bb\"");
        await Assert.That(toml).IsEqualTo("\"a\\u001Bb\"");
    }

    [Test]
    public async Task Every_generated_default_is_accepted_by_its_own_validator()
    {
        foreach (var s in AppSchema.Settings)
        {
            if (s.Type == SettingType.Raw) continue;
            var d = s.Default;
            if (d is null) continue;
            await Assert.That(() => SettingValues.ToToml(s, d)).ThrowsNothing().Because($"default of {s.Key}");
        }
    }
}
