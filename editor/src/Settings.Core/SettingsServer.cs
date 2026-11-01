using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace Settings.Core;

/// <summary>The loopback web server every settings editor runs: page, UI runtime and the JSON API.</summary>
public static class SettingsServer
{
    public static async Task<int> RunAsync(AppDefinition def, string[] args)
    {
        // null = nothing to serve: --help / --version (exit 0) or a bad option (exit 2).
        var options = CliOptions.Parse(def, args, out var exitCode);
        if (options is null) return exitCode;

        var store = new ConfigStore(def, options.ConfigPath);
        var token = Convert.ToHexStringLower(RandomNumberGenerator.GetBytes(16));
        long lastSeen = Environment.TickCount64;

        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.SetMinimumLevel(LogLevel.Warning);
        builder.WebHost.UseUrls($"http://127.0.0.1:{options.Port}");
        var app = builder.Build();

        // Only loopback Host headers (blocks DNS rebinding); API calls also need the per-run token.
        app.Use(async (ctx, next) =>
        {
            var host = ctx.Request.Host.Host;
            if (host is not ("127.0.0.1" or "localhost"))
            {
                ctx.Response.StatusCode = StatusCodes.Status421MisdirectedRequest;
                return;
            }
            if (ctx.Request.Path.StartsWithSegments("/api"))
            {
                if (ctx.Request.Headers["X-Settings-Token"] != token)
                {
                    ctx.Response.StatusCode = StatusCodes.Status403Forbidden;
                    return;
                }
                Interlocked.Exchange(ref lastSeen, Environment.TickCount64);
            }
            ctx.Response.Headers["X-Content-Type-Options"] = "nosniff";
            ctx.Response.Headers["Cache-Control"] = "no-store";
            await next();
        });

        var html = def.Html.Replace("<!--app-version-->", VersionHtml(def.Name));
        app.MapGet("/", () => Results.Content(html, "text/html; charset=utf-8"));
        app.MapGet("/app.css", () => Asset("app.css", "text/css; charset=utf-8"));
        app.MapGet("/app.js", () => Asset("app.js", "text/javascript; charset=utf-8"));
        app.MapGet("/logo", () => def.Logo?.Invoke() is { } logo ? Results.Stream(logo, def.LogoContentType) : Results.NotFound());

        app.MapGet("/api/state", () => Json(store.State()));
        app.MapGet("/api/fonts", () => Json(Strings(def.Fonts())));
        app.MapGet("/api/themes", () =>
        {
            var themes = new JsonArray();
            foreach (var t in def.Themes?.Invoke(store.Path) ?? [])
            {
                themes.Add((JsonNode)new JsonObject
                {
                    ["name"] = t.Name,
                    ["installed"] = t.Installed,
                    ["bg"] = t.Background,
                    ["fg"] = t.Foreground,
                    ["ansi"] = new JsonArray([.. t.Ansi.Select(c => (JsonNode?)c)]),
                });
            }
            return Json(new JsonObject { ["canInstall"] = def.InstallThemeAsync is not null, ["themes"] = themes });
        });

        app.MapPost("/api/themes/install", async (HttpRequest req) =>
        {
            if (def.InstallThemeAsync is not { } install) return BadRequest("This editor can't install themes.");
            if (await ReadObject(req) is not { } body || body["name"]?.GetValue<string>() is not { Length: > 0 } name)
                return BadRequest("Expected a theme name.");
            var error = await install(store.Path, name);
            return error is null ? Json(new JsonObject { ["installed"] = name }) : BadRequest(error);
        });
        app.MapGet("/api/list/{name}", (string name) =>
            def.Lists.TryGetValue(name, out var list) ? Json(Strings(list())) : Results.NotFound());

        app.MapPost("/api/preview", async (HttpRequest req) =>
        {
            if (await ReadObject(req) is not { } body) return BadRequest("Expected a JSON object.");
            return await Handle(async () =>
            {
                var r = await store.ApplyAsync(body);
                return new JsonObject { ["diff"] = LineDiff.Compute(r.Before, r.After), ["changed"] = r.Before != r.After };
            });
        });

        app.MapPost("/api/save", async (HttpRequest req) =>
        {
            if (await ReadObject(req) is not { } body) return BadRequest("Expected a JSON object.");
            return await Handle(async () =>
            {
                var (_, note) = await store.SaveAsync(body);
                var state = store.State();
                if (note is not null) state["note"] = note;
                return state;
            });
        });

        // The page's own file picker lists folders through this; browsers never reveal real paths.
        app.MapPost("/api/files", async (HttpRequest req) =>
        {
            if (await ReadObject(req) is not { } body) return BadRequest("Expected a JSON object.");
            string? kind = "file", path = null;
            bool hidden = false;
            if (body["kind"] is { } k && !(k is JsonValue kv && kv.TryGetValue(out kind))) return BadRequest("Expected kind to be a string.");
            if (body["path"] is { } p && !(p is JsonValue pv && pv.TryGetValue(out path))) return BadRequest("Expected path to be a string.");
            if (body["hidden"] is { } h && !(h is JsonValue hv && hv.TryGetValue(out hidden))) return BadRequest("Expected hidden to be true or false.");
            if (kind is not ("image" or "audio" or "file" or "folder")) return BadRequest("Unknown kind.");
            return Json(FileBrowser.List(path, kind, hidden, options.ConfigPath));
        });

        app.MapPost("/api/bye",() => { Interlocked.Exchange(ref lastSeen, 0); return Results.NoContent(); });

        await app.StartAsync();
        var url = app.Urls.First() + "/?t=" + token;

        Console.WriteLine($"{def.Terminal} settings for {store.Path}");
        if (store.TargetPath != store.Path) Console.WriteLine($"  (symlink to {store.TargetPath})");
        Console.WriteLine($"Open {url}");
        Console.WriteLine("Press Ctrl+C to stop.");

        if (options.Open)
        {
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
            catch (Exception ex) { Console.WriteLine($"Could not open a browser ({ex.Message}). Open the address above."); }
        }

        if (options.ExitWhenIdle)
        {
            _ = Task.Run(async () =>
            {
                // Background tabs poll at most about once a minute, so allow a generous gap.
                var idle = TimeSpan.FromMinutes(3).TotalMilliseconds;
                var startedAt = Environment.TickCount64;
                while (true)
                {
                    await Task.Delay(TimeSpan.FromSeconds(2));
                    var seen = Interlocked.Read(ref lastSeen);
                    bool goodbye = seen == 0;
                    bool quiet = Environment.TickCount64 - Math.Max(seen, startedAt) > idle;
                    if (!goodbye && !quiet) continue;
                    if (goodbye)
                    {
                        // A reload sends bye and then reconnects; give it a moment.
                        await Task.Delay(TimeSpan.FromSeconds(5));
                        if (Interlocked.Read(ref lastSeen) != 0) continue;
                    }
                    Console.WriteLine("Browser closed; stopping.");
                    await app.StopAsync();
                    return;
                }
            });
        }

        await app.WaitForShutdownAsync();
        return 0;
    }

    const string RepoUrl = "https://github.com/niklasfp/terminal-settings";

    /// <summary>
    /// The version (set from the release tag at publish time) and the git commit it was built from.
    /// The SDK appends the commit to the informational version as "+sha".
    /// </summary>
    static (string Version, string? Commit) BuildInfo()
    {
        var info = System.Reflection.Assembly.GetEntryAssembly()?
            .GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false)
            .OfType<System.Reflection.AssemblyInformationalVersionAttribute>().FirstOrDefault()?.InformationalVersion ?? "dev";
        var plus = info.IndexOf('+');
        if (plus < 0) return (info, null);
        var commit = info[(plus + 1)..];
        var isSha = commit.Length >= 7 && commit.All(Uri.IsHexDigit);
        return (info[..plus], isSha ? commit : null);
    }

    static string Version()
    {
        var (version, commit) = BuildInfo();
        return commit is null ? version : $"{version} ({commit[..7]})";
    }

    static string VersionHtml(string name)
    {
        var (version, commit) = BuildInfo();
        static string e(string s) => System.Net.WebUtility.HtmlEncode(s);
        var text = $"{e(name)} {e(version)}";
        var html = $"<p class=\"rev\" title=\"{text}\">{text}</p>";
        return commit is null
            ? html
            : $"{html}\n  <p class=\"rev\">Commit <a href=\"{RepoUrl}/commit/{commit}\" target=\"_blank\" rel=\"noopener\" title=\"{commit}\"><code>{commit[..7]}</code></a></p>";
    }

    static JsonArray Strings(IEnumerable<string> items) => new([.. items.Select(s => (JsonNode?)s)]);

    static IResult Asset(string name, string contentType)
    {
        var stream = typeof(SettingsServer).Assembly.GetManifestResourceStream("wwwroot/" + name);
        return stream is null ? Results.NotFound() : Results.Stream(stream, contentType);
    }

    static IResult Json(JsonNode node) => Results.Text(node.ToJsonString(), "application/json; charset=utf-8");

    static IResult Error(JsonObject body, int status) =>
        Results.Text(body.ToJsonString(), "application/json; charset=utf-8", statusCode: status);

    static IResult BadRequest(string message) => Error(new JsonObject { ["error"] = message }, 400);

    static async Task<IResult> Handle(Func<Task<JsonObject>> action)
    {
        try
        {
            return Json(await action());
        }
        catch (ConflictException ex)
        {
            return Error(new JsonObject { ["error"] = ex.Message, ["conflict"] = true }, 409);
        }
        catch (ChangeErrors ex)
        {
            var errors = new JsonObject();
            foreach (var (k, v) in ex.Errors) errors[k] = v;
            return Error(new JsonObject { ["error"] = ex.Message, ["errors"] = errors }, 400);
        }
        catch (ConfigFormatException ex)
        {
            return BadRequest(ex.Message);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return Error(new JsonObject { ["error"] = "Could not write the file: " + ex.Message }, 500);
        }
    }

    static async Task<JsonObject?> ReadObject(HttpRequest req)
    {
        try
        {
            return await JsonNode.ParseAsync(req.Body) as JsonObject;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    sealed record CliOptions(string ConfigPath, int Port, bool Open, bool ExitWhenIdle)
    {
        public static CliOptions? Parse(AppDefinition def, string[] args, out int exitCode)
        {
            exitCode = 0;
            string? config = null;
            int port = 0;
            bool open = true, stay = false;
            for (int i = 0; i < args.Length; i++)
            {
                switch (args[i])
                {
                    case "--config" or "-c" when i + 1 < args.Length: config = args[++i]; break;
                    case "--port" or "-p" when i + 1 < args.Length && int.TryParse(args[i + 1], out var p): port = p; i++; break;
                    case "--no-open": open = false; break;
                    case "--stay": stay = true; break;
                    case "--version" or "-v":
                        Console.WriteLine($"{def.Name} {Version()}");
                        return null;
                    case "--help" or "-h":
                        Console.WriteLine($"""
                            {def.Name}: edit {def.Terminal}'s config in your browser.

                            Usage: {def.Name} [options]
                              -c, --config <path>  Config file (default: {def.DefaultConfigPath()})
                              -p, --port <n>       Port on 127.0.0.1 (default: a free port)
                                  --no-open        Don't open a browser
                                  --stay           Keep running after the browser tab closes
                              -v, --version        Print the version
                            """);
                        return null;
                    default:
                        Console.Error.WriteLine($"Unknown option '{args[i]}'. Try --help.");
                        exitCode = 2;
                        return null;
                }
            }
            config ??= def.DefaultConfigPath();
            return new CliOptions(Path.GetFullPath(config), port, open, open && !stay);
        }
    }
}
