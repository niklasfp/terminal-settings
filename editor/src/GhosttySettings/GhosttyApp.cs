using System.Diagnostics;
using System.Text.RegularExpressions;
using Settings.Core;
using Settings.Generated;

namespace GhosttySettings;

/// <summary>Ghostty's editor: line-based config, validated by Ghostty itself, reloaded via AppleScript.</summary>
internal static partial class GhosttyApp
{
    public static AppDefinition Create()
    {
        var settings = new SettingCatalog(AppSchema.Settings);
        return new AppDefinition
        {
            Name = "ghostty-settings",
            Terminal = "Ghostty",
            Html = AppSchema.Html,
            Settings = settings,
            Format = new GhosttyFormat(),
            DefaultConfigPath = DefaultConfigPath,
            ValidateAsync = text => ValidateAsync(text, settings),
            AfterSaveAsync = ReloadAsync,
            Logo = () => typeof(GhosttyApp).Assembly.GetManifestResourceStream("logo"),
            LogoContentType = "image/png",
            Themes = configPath => GhosttyThemes.List(configPath, Binary()),
            Lists = new Dictionary<string, Func<string[]>>
            {
                ["actions"] = () => Lines("+list-actions"),
                ["x11-colors"] = () => Lines("+list-colors", "--plain"),
            },
        };
    }

    /// <summary>Ghostty reads config.ghostty (1.2.3+) or the older config, from XDG_CONFIG_HOME/ghostty.</summary>
    static string DefaultConfigPath()
    {
        var xdg = Environment.GetEnvironmentVariable("XDG_CONFIG_HOME");
        var dir = Path.Combine(string.IsNullOrEmpty(xdg)
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config")
            : xdg, "ghostty");
        var modern = Path.Combine(dir, "config.ghostty");
        var legacy = Path.Combine(dir, "config");
        return File.Exists(modern) || !File.Exists(legacy) ? modern : legacy;
    }

    static string? Binary()
    {
        var fromEnv = Environment.GetEnvironmentVariable("GHOSTTY_BIN");
        if (!string.IsNullOrEmpty(fromEnv) && File.Exists(fromEnv)) return fromEnv;
        string[] candidates = OperatingSystem.IsMacOS()
            ? ["/Applications/Ghostty.app/Contents/MacOS/ghostty", Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Applications/Ghostty.app/Contents/MacOS/ghostty")]
            : [];
        foreach (var c in candidates) if (File.Exists(c)) return c;
        foreach (var dir in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator))
        {
            var p = Path.Combine(dir, "ghostty");
            if (File.Exists(p)) return p;
        }
        return null;
    }

    static (int exit, string output) Run(string file, params string[] args)
    {
        var psi = new ProcessStartInfo(file, args) { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false };
        using var p = Process.Start(psi) ?? throw new InvalidOperationException($"Could not start {file}.");
        var stdout = p.StandardOutput.ReadToEndAsync();
        var stderr = p.StandardError.ReadToEndAsync();
        if (!p.WaitForExit(15_000)) { p.Kill(); return (-1, "timed out"); }
        return (p.ExitCode, stdout.Result + stderr.Result);
    }

    static string[] Lines(params string[] args)
    {
        if (Binary() is not { } bin) return [];
        try
        {
            var (_, output) = Run(bin, args);
            return output.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        }
        catch (Exception)
        {
            return [];
        }
    }

    [GeneratedRegex(@":(\d+):([a-z0-9-]+): (.+)$")]
    private static partial Regex ValidationLine();

    /// <summary>Runs Ghostty's own parser on the candidate file; maps "file:line:key: message" to settings.</summary>
    static async Task<Dictionary<string, string>?> ValidateAsync(string text, SettingCatalog settings)
    {
        if (Binary() is not { } bin) return null;
        var temp = Path.Combine(Path.GetTempPath(), $"ghostty-settings-{Environment.ProcessId}-{Guid.NewGuid():n}.ghostty");
        try
        {
            await File.WriteAllTextAsync(temp, text);
            var (exit, output) = Run(bin, "+validate-config", "--config-file=" + temp);
            if (exit == 0) return null;
            var errors = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var line in output.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                var m = ValidationLine().Match(line);
                if (!m.Success) continue;
                var key = settings.Find(m.Groups[2].Value) is not null ? m.Groups[2].Value : "";
                errors[key] = "Ghostty: " + m.Groups[3].Value;
            }
            if (errors.Count == 0) errors[""] = "Ghostty rejected the config: " + output.Trim();
            return errors;
        }
        finally
        {
            File.Delete(temp);
        }
    }

    /// <summary>Asks a running Ghostty to reload its config. Never launches Ghostty.</summary>
    static Task<string?> ReloadAsync()
    {
        const string manual = "Saved. Press ⌘⇧, in Ghostty to reload.";
        if (!OperatingSystem.IsMacOS()) return Task.FromResult<string?>("Saved. Press Ctrl+Shift+, in Ghostty to reload.");
        const string script = """
            if application "Ghostty" is running then
              tell application "Ghostty"
                if (count of windows) is 0 then return "no-window"
                perform action "reload_config" on focused terminal of selected tab of front window
              end tell
              return "reloaded"
            end if
            return "not-running"
            """;
        try
        {
            var (exit, output) = Run("/usr/bin/osascript", "-e", script);
            var result = output.Trim();
            return Task.FromResult<string?>(exit == 0 && result == "reloaded" ? "Saved and reloaded in Ghostty."
                : exit == 0 && result == "not-running" ? "Saved. Ghostty will use it next time it starts."
                : manual);
        }
        catch (Exception)
        {
            return Task.FromResult<string?>(manual);
        }
    }
}
