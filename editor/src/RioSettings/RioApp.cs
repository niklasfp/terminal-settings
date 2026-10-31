using Settings.Core;
using Settings.Generated;

namespace RioSettings;

/// <summary>Rio's editor: TOML config, per-platform overrides, and Rio's own hot reload.</summary>
internal static class RioApp
{
    public static AppDefinition Create() => new()
    {
        Name = "rio-settings",
        Terminal = "Rio",
        Html = AppSchema.Html,
        Settings = new SettingCatalog(AppSchema.Settings),
        Format = new RioTomlFormat(),
        Scopes = ["macos", "linux", "windows"],
        DefaultConfigPath = DefaultConfigPath,
        Themes = RioThemes.List,
        InstallThemeAsync = RioThemes.InstallAsync,
        Logo = () => typeof(RioApp).Assembly.GetManifestResourceStream("logo"),
        LogoContentType = "image/svg+xml",
        // Rio watches its config folder and reloads by itself.
        AfterSaveAsync = () => Task.FromResult<string?>("Saved. Rio picks up the change right away."),
    };

    static string DefaultConfigPath()
    {
        var home = Environment.GetEnvironmentVariable("RIO_CONFIG_HOME");
        if (!string.IsNullOrEmpty(home)) return Path.Combine(home, "config.toml");
        // Same lookup as rio-backend's config_dir_path(): Windows uses %USERPROFILE%\AppData\Local\rio,
        // macOS always ~/.config/rio, other Unix honours XDG_CONFIG_HOME.
        var userHome = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (OperatingSystem.IsWindows())
            return Path.Combine(userHome, "AppData", "Local", "rio", "config.toml");
        var xdg = Environment.GetEnvironmentVariable("XDG_CONFIG_HOME");
        if (!OperatingSystem.IsMacOS() && !string.IsNullOrEmpty(xdg))
            return Path.Combine(xdg, "rio", "config.toml");
        return Path.Combine(userHome, ".config", "rio", "config.toml");
    }

}
