using System.Text.Json.Nodes;
using Settings.Core;

namespace RioSettings.Tests;

public class ConfigStoreTests
{
    static string TempDir()
    {
        var dir = Path.Combine(Path.GetTempPath(), "rio-settings-tests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(dir);
        return dir;
    }

    [Test]
    public async Task Save_writes_through_symlink_and_keeps_the_link()
    {
        var dir = TempDir();
        var target = Path.Combine(dir, "real", "config.toml");
        Directory.CreateDirectory(Path.GetDirectoryName(target)!);
        File.WriteAllText(target, "# mine\n[window]\nopacity = 1.0\n");
        var link = Path.Combine(dir, "config.toml");
        File.CreateSymbolicLink(link, target);

        var store = new ConfigStore(RioApp.Create(), link);
        await store.SaveAsync(new JsonObject { ["changes"] = new JsonObject { ["window.opacity"] = 0.8 } });

        await Assert.That(new FileInfo(link).LinkTarget).IsNotNull();
        await Assert.That(File.ReadAllText(target)).IsEqualTo("# mine\n[window]\nopacity = 0.8\n");
        await Assert.That(File.ReadAllText(target + ".bak")).IsEqualTo("# mine\n[window]\nopacity = 1.0\n");
    }

    [Test]
    public async Task Stale_version_is_a_conflict()
    {
        var dir = TempDir();
        var path = Path.Combine(dir, "config.toml");
        File.WriteAllText(path, "[window]\nopacity = 1.0\n");
        var store = new ConfigStore(RioApp.Create(), path);
        var version = ConfigStore.VersionOf(File.ReadAllText(path));
        File.WriteAllText(path, "[window]\nopacity = 0.5\n");

        await Assert.That(async () => await store.ApplyAsync(new JsonObject
        {
            ["version"] = version,
            ["changes"] = new JsonObject { ["window.opacity"] = 0.8 },
        })).Throws<ConflictException>();
    }

    [Test]
    public async Task Invalid_values_are_reported_per_key_and_nothing_is_written()
    {
        var dir = TempDir();
        var path = Path.Combine(dir, "config.toml");
        File.WriteAllText(path, "");
        var store = new ConfigStore(RioApp.Create(), path);

        var ex = await Assert.ThrowsAsync<ChangeErrors>(async () => await store.SaveAsync(new JsonObject
        {
            ["changes"] = new JsonObject { ["window.opacity"] = 3, ["cursor.shape"] = "beam", ["nope"] = 1 },
        }));

        await Assert.That(ex!.Errors.Keys).Contains("window.opacity");
        await Assert.That(ex.Errors.Keys).Contains("nope");
        await Assert.That(ex.Errors.Keys).DoesNotContain("cursor.shape");
        await Assert.That(File.ReadAllText(path)).IsEqualTo("");
    }

    [Test]
    public async Task Platform_scope_only_accepts_overridable_settings()
    {
        var store = new ConfigStore(RioApp.Create(), Path.Combine(TempDir(), "config.toml"));
        var ok = await store.ApplyAsync(new JsonObject { ["scope"] = "macos", ["changes"] = new JsonObject { ["window.opacity"] = 0.9 } });
        await Assert.That(ok.After).IsEqualTo("[platform.macos.window]\nopacity = 0.9\n");

        await Assert.That(async () => await store.ApplyAsync(new JsonObject { ["scope"] = "macos", ["changes"] = new JsonObject { ["cursor.shape"] = "beam" } }))
            .Throws<ChangeErrors>();
    }

    [Test]
    public async Task Null_removes_the_key()
    {
        var path = Path.Combine(TempDir(), "config.toml");
        File.WriteAllText(path, "[window]\nopacity = 0.9\nmode = \"Maximized\"\n");
        var r = await new ConfigStore(RioApp.Create(), path).ApplyAsync(new JsonObject { ["changes"] = new JsonObject { ["window.opacity"] = null } });
        await Assert.That(r.After).IsEqualTo("[window]\nmode = \"Maximized\"\n");
    }
}

public class LineEndingTests
{
    [Test]
    public async Task Crlf_files_stay_crlf_after_edits()
    {
        var before = "# mine\r\n[window]\r\nopacity = 1.0\r\n";
        var after = "# mine\r\n[window]\r\nopacity = 0.8\r\n\n[cursor]\nshape = \"beam\"\n";
        await Assert.That(Settings.Core.ConfigStore.MatchLineEndings(before, after))
            .IsEqualTo("# mine\r\n[window]\r\nopacity = 0.8\r\n\r\n[cursor]\r\nshape = \"beam\"\r\n");
    }

    [Test]
    public async Task Lf_files_stay_lf()
    {
        await Assert.That(Settings.Core.ConfigStore.MatchLineEndings("a = 1\n", "a = 2\nb = 3\n")).IsEqualTo("a = 2\nb = 3\n");
    }
}
