namespace RioSettings.Tests;

public class RioThemesTests
{
    static string TempConfig()
    {
        var dir = Path.Combine(Path.GetTempPath(), "rio-settings-tests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(dir);
        return Path.Combine(dir, "config.toml");
    }

    [Test]
    public async Task Catalogue_is_embedded_with_preview_colors()
    {
        var themes = RioThemes.List(TempConfig());
        await Assert.That(themes.Count).IsGreaterThan(500);
        var dracula = themes.Single(t => t.Name == "Dracula");
        await Assert.That(dracula.Installed).IsFalse();
        await Assert.That(dracula.Background).IsEqualTo("#282a36");
        await Assert.That(dracula.Ansi.Count).IsEqualTo(8);
    }

    [Test]
    public async Task Installed_themes_come_first_and_win_over_the_catalogue()
    {
        var config = TempConfig();
        var dir = Path.Combine(Path.GetDirectoryName(config)!, "themes");
        Directory.CreateDirectory(dir);
        File.WriteAllText(Path.Combine(dir, "Dracula.toml"), "[colors]\nbackground = '#000000'\nforeground = '#ffffff'\nred = '#ff0000'\n");
        var themes = RioThemes.List(config);
        await Assert.That(themes[0].Name).IsEqualTo("Dracula");
        await Assert.That(themes[0].Installed).IsTrue();
        await Assert.That(themes[0].Background).IsEqualTo("#000000");
        await Assert.That(themes.Count(t => t.Name == "Dracula")).IsEqualTo(1);
    }

    [Test]
    [Arguments("../../evil")]
    [Arguments("Not A Real Theme")]
    public async Task Install_only_accepts_catalogue_names(string name)
    {
        var config = TempConfig();
        var error = await RioThemes.InstallAsync(config, name);
        await Assert.That(error).IsNotNull();
        await Assert.That(Directory.Exists(Path.Combine(Path.GetDirectoryName(config)!, "themes"))).IsFalse();
    }
}
