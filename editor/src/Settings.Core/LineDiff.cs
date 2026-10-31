using System.Text.Json.Nodes;

namespace Settings.Core;

/// <summary>Line diff (LCS) rendered as hunks with a little context. Config files are small.</summary>
internal static class LineDiff
{
    public static JsonArray Compute(string before, string after, int context = 2)
    {
        var a = Split(before);
        var b = Split(after);
        int n = a.Length, m = b.Length;
        var lcs = new int[n + 1, m + 1];
        for (int i = n - 1; i >= 0; i--)
            for (int j = m - 1; j >= 0; j--)
                lcs[i, j] = a[i] == b[j] ? lcs[i + 1, j + 1] + 1 : Math.Max(lcs[i + 1, j], lcs[i, j + 1]);

        var ops = new List<(char op, string text, int line)>();
        int x = 0, y = 0;
        while (x < n || y < m)
        {
            if (x < n && y < m && a[x] == b[y]) { ops.Add((' ', a[x], y + 1)); x++; y++; }
            else if (y < m && (x == n || lcs[x, y + 1] >= lcs[x + 1, y])) { ops.Add(('+', b[y], y + 1)); y++; }
            else { ops.Add(('-', a[x], 0)); x++; }
        }

        var keep = new bool[ops.Count];
        for (int i = 0; i < ops.Count; i++)
        {
            if (ops[i].op == ' ') continue;
            for (int k = Math.Max(0, i - context); k <= Math.Min(ops.Count - 1, i + context); k++) keep[k] = true;
        }

        var result = new JsonArray();
        bool gap = false;
        for (int i = 0; i < ops.Count; i++)
        {
            if (!keep[i]) { gap = true; continue; }
            if (gap && result.Count > 0) result.Add((JsonNode)new JsonObject { ["o"] = "…", ["t"] = "" });
            gap = false;
            result.Add((JsonNode)new JsonObject { ["o"] = ops[i].op.ToString(), ["t"] = ops[i].text, ["n"] = ops[i].line });
        }
        return result;
    }

    static string[] Split(string text)
    {
        if (text.Length == 0) return [];
        var lines = text.Replace("\r\n", "\n").Split('\n');
        return text.EndsWith('\n') ? lines[..^1] : lines;
    }
}
