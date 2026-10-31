using System.Text;
using System.Text.Json.Nodes;
using Tomlyn.Parsing;
using Tomlyn.Syntax;

namespace RioSettings.Toml;

internal sealed class TomlEditException(string message) : Exception(message);

/// <summary>
/// A config.toml held as text. Edits splice new text into the exact span of the value
/// they change, so everything else in the file (comments, ordering, spacing) stays
/// byte-identical. Every edit is re-parsed and rejected if the result is invalid TOML.
/// </summary>
internal sealed class ConfigDocument
{
    sealed record Entry(string[] Path, KeyValueSyntax KeyValue, int TableIndex);

    sealed record Table(string[] Path, TableSyntaxBase Syntax, bool IsArray, int Index);

    DocumentSyntax _doc;
    List<Entry> _entries = [];
    List<Table> _tables = [];

    public string Text { get; private set; }

    ConfigDocument(string text, DocumentSyntax doc)
    {
        Text = text;
        _doc = doc;
        Index();
    }

    public static ConfigDocument Parse(string text)
    {
        var doc = SyntaxParser.Parse(text, "config.toml", validate: true);
        if (doc.HasErrors) throw new TomlEditException("config.toml is not valid TOML: " + FirstError(doc));
        return new ConfigDocument(text, doc);
    }

    static string FirstError(DocumentSyntax doc)
    {
        foreach (var d in doc.Diagnostics) return d.ToString();
        return "unknown error";
    }

    void Index()
    {
        _entries = [];
        _tables = [];
        foreach (var kv in _doc.KeyValues) AddEntries([], kv, -1);

        var arrayCounts = new Dictionary<string, int>(StringComparer.Ordinal);
        for (int t = 0; t < _doc.Tables.ChildrenCount; t++)
        {
            var table = _doc.Tables.GetChild(t)!;
            var name = KeyPath(table.Name!);
            bool isArray = table is TableArraySyntax;
            string[] prefix = name;
            if (isArray)
            {
                var joined = string.Join('\u0001', name);
                var i = arrayCounts.GetValueOrDefault(joined);
                arrayCounts[joined] = i + 1;
                prefix = [.. name[..^1], $"{name[^1]}[{i}]"];
            }
            _tables.Add(new Table(name, table, isArray, t));
            foreach (var item in table.Items)
                if (item is KeyValueSyntax kv) AddEntries(prefix, kv, t);
        }
    }

    void AddEntries(string[] prefix, KeyValueSyntax kv, int tableIndex)
    {
        _entries.Add(new Entry([.. prefix, .. KeyPath(kv.Key!)], kv, tableIndex));
    }

    static string[] KeyPath(KeySyntax key)
    {
        var parts = new List<string> { KeyPart(key.Key!) };
        foreach (var dot in key.DotKeys) parts.Add(KeyPart(dot.Key!));
        return [.. parts];
    }

    static string KeyPart(SyntaxNode node) => node switch
    {
        BareKeySyntax bare => bare.Key!.Text!,
        StringValueSyntax str => str.Value!,
        _ => node.ToString().Trim(),
    };

    /// <summary>All values in the file, flattened to dotted keys. Inline tables are flattened too.</summary>
    public Dictionary<string, JsonNode?> Values()
    {
        var result = new Dictionary<string, JsonNode?>(StringComparer.Ordinal);
        foreach (var e in _entries)
        {
            var key = string.Join('.', e.Path);
            var value = TomlValues.ToJson(e.KeyValue.Value!);
            result[key] = value;
            if (value is JsonObject obj) Flatten(result, key, obj);
        }
        return result;
    }

    static void Flatten(Dictionary<string, JsonNode?> into, string prefix, JsonObject obj)
    {
        foreach (var (k, v) in obj)
        {
            var key = prefix + "." + k;
            into[key] = v?.DeepClone();
            if (v is JsonObject child) Flatten(into, key, child);
        }
    }

    public bool Has(string[] path) => _entries.Any(e => e.Path.AsSpan().SequenceEqual(path));

    /// <summary>Sets <paramref name="path"/> to the TOML literal <paramref name="tomlLiteral"/>.</summary>
    public void Set(string[] path, string tomlLiteral)
    {
        var existing = _entries.FirstOrDefault(e => e.Path.AsSpan().SequenceEqual(path));
        if (existing is not null)
        {
            var span = existing.KeyValue.Value!.Span;
            Apply(Splice(Text, span.Start.Offset, span.End.Offset + 1, tomlLiteral));
            return;
        }

        foreach (var e in _entries)
        {
            if (e.Path.Length < path.Length && path.AsSpan(0, e.Path.Length).SequenceEqual(e.Path))
                throw new TomlEditException(
                    $"'{string.Join('.', e.Path)}' is written as an inline table in config.toml. Edit '{string.Join('.', path)}' in the file directly.");
        }

        Apply(Insert(path, tomlLiteral));
    }

    /// <summary>Removes the line that sets <paramref name="path"/>. Returns false when it isn't set.</summary>
    public bool Remove(string[] path)
    {
        var existing = _entries.FirstOrDefault(e => e.Path.AsSpan().SequenceEqual(path));
        if (existing is null) return false;
        var span = existing.KeyValue.Span;
        int start = span.Start.Offset, end = span.End.Offset + 1;
        int lineStart = LineStart(start);
        if (Text[lineStart..start].Trim().Length == 0) start = lineStart;
        // Swallow the rest of the line (trailing comment) and its newline.
        int nl = Text.IndexOf('\n', Math.Max(start, end - 1));
        end = nl < 0 ? Text.Length : nl + 1;
        Apply(Splice(Text, start, end, ""));
        return true;
    }

    /// <summary>The raw TOML of every [[name]] block (with their sub-tables), or "" when there are none.</summary>
    public string GetTableArray(string[] name)
    {
        var (start, end) = TableArrayRange(name);
        return start < 0 ? "" : Text[start..end].Trim('\n') + "\n";
    }

    /// <summary>Replaces every [[name]] block with <paramref name="raw"/>; empty removes them.</summary>
    public void SetTableArray(string[] name, string raw)
    {
        raw = raw.Replace("\r\n", "\n").Trim('\n');
        var check = SyntaxParser.Parse(raw, "input", validate: true);
        if (check.HasErrors) throw new TomlEditException("Invalid TOML: " + FirstError(check));
        foreach (var t in check.Tables)
        {
            var path = KeyPath(t.Name!);
            if (!path.AsSpan().StartsWith(name))
                throw new TomlEditException($"Only [[{string.Join('.', name)}]] blocks and their sub-tables belong here.");
        }
        if (check.KeyValues.ChildrenCount > 0)
            throw new TomlEditException($"Start with a [[{string.Join('.', name)}]] header.");

        var (start, end) = TableArrayRange(name);
        var block = raw.Length == 0 ? "" : raw + "\n";
        if (start < 0)
        {
            if (block.Length == 0) return;
            var text = Text.Length == 0 || Text.EndsWith('\n') ? Text : Text + "\n";
            Apply(text + (text.Length > 0 ? "\n" : "") + block);
            return;
        }
        if (block.Length > 0)
        {
            // Keep the blank lines that separated the old blocks from what follows.
            var old = Text[start..end];
            block = raw + old[old.TrimEnd('\n').Length..];
            if (!block.EndsWith('\n')) block += "\n";
        }
        Apply(Splice(Text, start, end, block));
    }

    (int start, int end) TableArrayRange(string[] name)
    {
        // The contiguous run of tables from the first [[name]] whose header starts with name.
        int first = _tables.FindIndex(t => t.IsArray && t.Path.AsSpan().SequenceEqual(name));
        if (first < 0) return (-1, -1);
        int last = first;
        for (int i = first + 1; i < _tables.Count && _tables[i].Path.AsSpan().StartsWith(name); i++) last = i;
        for (int i = last + 1; i < _tables.Count; i++)
            if (_tables[i].IsArray && _tables[i].Path.AsSpan().SequenceEqual(name))
                throw new TomlEditException($"[[{string.Join('.', name)}]] blocks are split up in the file. Edit them in the file directly.");

        int start = LineStart(TableStart(_tables[first].Syntax));
        int end = last + 1 < _tables.Count ? LineStart(TableStart(_tables[last + 1].Syntax)) : Text.Length;
        return (start, end);
    }

    static int TableStart(TableSyntaxBase table) => table.OpenBracket!.Span.Start.Offset;

    int LineStart(int offset) => offset == 0 ? 0 : Text.LastIndexOf('\n', offset - 1) + 1;

    string Insert(string[] path, string literal)
    {
        // Prefer the deepest existing [table] whose name is a prefix of the key.
        Table? best = null;
        foreach (var t in _tables)
        {
            if (t.IsArray || t.Path.Length >= path.Length) continue;
            if (!path.AsSpan(0, t.Path.Length).SequenceEqual(t.Path)) continue;
            if (best is null || t.Path.Length > best.Path.Length) best = t;
        }

        if (best is not null)
        {
            var rel = FormatKey(path[best.Path.Length..]);
            return InsertIntoTable(best, $"{rel} = {literal}");
        }

        string[] parent = path[..^1];
        bool rootDotted = parent.Length > 0 && _entries.Any(e => e.TableIndex < 0 && e.Path.Length > 1 && e.Path[0] == path[0]);
        if (parent.Length == 0 || rootDotted)
            return InsertAtRoot($"{FormatKey(path)} = {literal}");

        var text = Text.Length == 0 || Text.EndsWith('\n') ? Text : Text + "\n";
        var sep = text.Length == 0 || text.EndsWith("\n\n") ? "" : "\n";
        return $"{text}{sep}[{FormatKey(parent)}]\n{FormatKey(path[^1..])} = {literal}\n";
    }

    string InsertIntoTable(Table table, string line)
    {
        int offset;
        var lastKv = table.Syntax.Items.OfType<KeyValueSyntax>().LastOrDefault();
        if (lastKv is not null) offset = EndOfLine(lastKv.Span.End.Offset);
        else offset = EndOfLine(table.Syntax.CloseBracket!.Span.End.Offset);
        return InsertLine(offset, line);
    }

    string InsertAtRoot(string line)
    {
        var lastRoot = _entries.LastOrDefault(e => e.TableIndex < 0);
        if (lastRoot is not null) return InsertLine(EndOfLine(lastRoot.KeyValue.Span.End.Offset), line);
        if (_tables.Count == 0)
        {
            var text = Text.Length == 0 || Text.EndsWith('\n') ? Text : Text + "\n";
            return text + line + "\n";
        }
        // Root keys must come before the first table: put them after the file's leading comment block.
        int pos = 0;
        while (pos < Text.Length)
        {
            int nl = Text.IndexOf('\n', pos);
            var lineText = (nl < 0 ? Text[pos..] : Text[pos..nl]).Trim();
            if (lineText.Length > 0 && !lineText.StartsWith('#')) break;
            if (nl < 0) { pos = Text.Length; break; }
            pos = nl + 1;
        }
        var insert = line + "\n";
        bool blankAfter = pos < Text.Length && Text[pos] != '\n';
        return Splice(Text, pos, pos, insert + (blankAfter ? "\n" : ""));
    }

    string InsertLine(int offset, string line)
    {
        // offset is just after a newline (or end of text without one).
        if (offset == Text.Length && Text.Length > 0 && !Text.EndsWith('\n'))
            return Text + "\n" + line + "\n";
        return Splice(Text, offset, offset, line + "\n");
    }

    int EndOfLine(int offset)
    {
        int nl = Text.IndexOf('\n', offset);
        return nl < 0 ? Text.Length : nl + 1;
    }

    static string Splice(string text, int start, int end, string replacement) =>
        string.Concat(text.AsSpan(0, start), replacement, text.AsSpan(end));

    void Apply(string newText)
    {
        var doc = SyntaxParser.Parse(newText, "config.toml", validate: true);
        if (doc.HasErrors) throw new TomlEditException("Edit would make config.toml invalid: " + FirstError(doc));
        Text = newText;
        _doc = doc;
        Index();
    }

    internal static string FormatKey(IEnumerable<string> parts) =>
        string.Join('.', parts.Select(p => IsBare(p) ? p : TomlValues.Quote(p)));

    static bool IsBare(string s) =>
        s.Length > 0 && s.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
}
