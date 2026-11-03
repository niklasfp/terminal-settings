//! Merges ghostty-source.json (generated from `ghostty +show-config --default --docs` and
//! Config.zig) with ghostty-overlay.json (groups, widget types, ranges). Labels and docs come from
//! Ghostty itself.

use serde_json::Value;
use settings_core::merge::{Obj, boolean, error, humanize, number, obj, objects, string, strings, warning};
use settings_core::{Field, Group, Schema, SchemaDiagnostic, Setting, SettingType};
use std::collections::HashSet;

/// Repeatable keys with long built-in defaults show "N built-in entries" instead of listing them.
const MAX_LISTED_DEFAULTS: usize = 12;

/// Name of the app suggestion list for keybind actions (see `AppDefinition::lists`).
pub const ACTIONS_LIST: &str = "actions";

/// Merges the two layers. `Err` only when either file isn't a JSON object.
pub fn merge_schema(
    source_json: &str,
    overlay_json: &str,
) -> Result<(Schema, Vec<SchemaDiagnostic>), String> {
    let source: Obj =
        serde_json::from_str(source_json).map_err(|e| format!("ghostty-source.json: {e}"))?;
    let overlay: Obj =
        serde_json::from_str(overlay_json).map_err(|e| format!("ghostty-overlay.json: {e}"))?;
    let empty = Obj::new();
    let mut diags = Vec::new();

    let app = obj(&overlay, "app").unwrap_or(&empty);
    let terminal = string(app, "terminal").unwrap_or_else(|| "Terminal".into());
    let mut schema = Schema {
        revision: string(&source, "ghosttyVersion").unwrap_or_else(|| "unknown".into()),
        title: string(app, "title").unwrap_or_else(|| format!("{terminal} settings")),
        terminal,
        brand: string(app, "brand").unwrap_or_default(),
        scopes: strings(app, "scopes").unwrap_or_default(),
        theme_locks_colors: boolean(app, "themeLocksColors"),
        groups: Vec::new(),
        settings: Vec::new(),
    };

    for g in objects(&overlay, "groups") {
        schema.groups.push(Group {
            id: string(g, "id").unwrap_or_default(),
            title: string(g, "title").unwrap_or_default(),
            icon: string(g, "icon").unwrap_or_default(),
            doc: string(g, "doc"),
        });
    }
    let group_ids: HashSet<&str> = schema.groups.iter().map(|g| g.id.as_str()).collect();

    let mut rules: Vec<(String, String)> = Vec::new();
    for r in objects(&overlay, "rules") {
        let group = string(r, "group").unwrap_or_default();
        for prefix in strings(r, "prefix").unwrap_or_default() {
            rules.push((group.clone(), prefix));
        }
    }
    for (group, _) in &rules {
        if !group_ids.contains(group.as_str()) {
            diags.push(error(
                "GHO007",
                format!("Rule uses unknown group '{group}'."),
            ));
        }
    }

    let platform_prefixes = obj(&overlay, "platformPrefixes").unwrap_or(&empty);
    let type_map = obj(&overlay, "typeMap").unwrap_or(&empty);
    let color_keywords = obj(&overlay, "colorKeywords").unwrap_or(&empty);
    let compact_types = strings(&overlay, "compactTypes").unwrap_or_default();
    let overrides = obj(&overlay, "settings").unwrap_or(&empty);
    let mut seen: HashSet<String> = HashSet::new();

    for opt in objects(&source, "options") {
        let key = string(opt, "key").unwrap_or_default();
        seen.insert(key.clone());
        let ov = obj(overrides, &key).unwrap_or(&empty);
        let kind = string(opt, "kind").unwrap_or_else(|| "other".into());
        let zig_type = string(opt, "zigType").unwrap_or_default();
        let repeatable = boolean(opt, "repeatable");
        let optional = boolean(opt, "optional");
        let defaults = strings(opt, "defaults").unwrap_or_default();

        let type_name = string(ov, "type")
            .unwrap_or_else(|| map_kind(&key, &kind, &zig_type, repeatable, type_map, &mut diags));
        let mut ty = SettingType::parse(&type_name).unwrap_or_else(|| {
            diags.push(error(
                "GHO006",
                format!("'{key}' has unknown widget type '{type_name}'; it's shown as text."),
            ));
            SettingType::String
        });
        let (summary, more) = split_doc(string(opt, "doc").as_deref());

        let mut setting = Setting::new(&key, group_for(&key, &rules), ty);
        setting.label = humanize(&key);
        setting.doc = summary;
        setting.doc_more = more;
        setting.min = number(ov, "min");
        setting.max = number(ov, "max");
        setting.step = number(ov, "step");
        setting.unit = string(ov, "unit");
        // Durations are free text in Ghostty's own syntax; say what it looks like.
        setting.placeholder = string(ov, "placeholder")
            .or_else(|| (kind == "duration").then(|| "e.g. 750ms or 5s".to_string()));
        setting.pick = string(ov, "pick");
        setting.compact = kind == "duration" || is_compact_type(&zig_type, &compact_types);
        setting.repeatable = repeatable;
        setting.platforms =
            strings(opt, "platforms").or_else(|| platforms_by_prefix(&key, platform_prefixes));

        if let Some(since) = string(opt, "since") {
            setting.tags.push(format!("Since {since}"));
        }
        match string(opt, "reload").as_deref() {
            Some("restart") => setting.tags.push("Restart needed".into()),
            Some("new-terminals") => setting.tags.push("New windows only".into()),
            _ => {}
        }

        setting.values = strings(ov, "values")
            .or_else(|| strings(opt, "values"))
            .or_else(|| {
                opt.get("flags").and_then(Value::as_array).map(|flags| {
                    flags
                        .iter()
                        .filter_map(Value::as_object)
                        .map(|f| string(f, "name").unwrap_or_default())
                        .collect()
                })
            });
        if ty == SettingType::Enum && setting.values.as_ref().is_none_or(Vec::is_empty) {
            diags.push(warning(
                "GHO004",
                format!("Enum '{key}' has no known values; it's shown as text."),
            ));
            ty = SettingType::String;
            setting.ty = ty;
        }

        // Colors that also accept keywords (e.g. cell-foreground) keep them as allowed extra values.
        if ty == SettingType::Color
            && let Some(keywords) = strings(color_keywords, &zig_type)
        {
            setting.values = Some(keywords);
        }

        if ty == SettingType::Records {
            setting.fields = Some(record_fields(&kind));
        }

        setting.default = default_for(ty, &defaults, opt);
        setting.nullable = match ov.get("nullable") {
            Some(v) => v.as_bool().unwrap_or(false),
            None => optional || setting.default.is_null(),
        };
        if repeatable && defaults.len() > MAX_LISTED_DEFAULTS {
            setting.built_ins_note = Some(format!("{} built-in entries", defaults.len()));
        }

        schema.settings.push(setting);
    }

    for key in overrides.keys() {
        if !seen.contains(key) {
            diags.push(warning(
                "GHO003",
                format!("Overlay setting '{key}' is not in this Ghostty version's options."),
            ));
        }
    }

    Ok((schema, diags))
}

fn map_kind(
    key: &str,
    kind: &str,
    zig_type: &str,
    repeatable: bool,
    type_map: &Obj,
    diags: &mut Vec<SchemaDiagnostic>,
) -> String {
    let fixed = match kind {
        "bool" => Some("bool"),
        "int" => Some("int"),
        "float" => Some("float"),
        "string" | "duration" => Some("string"),
        "enum" => Some("enum"),
        "flags" => Some("flags"),
        "color" => Some("color"),
        "repeatable-string" => Some("strings"),
        "palette" | "keybind" => Some("records"),
        _ => None,
    };
    if let Some(t) = fixed {
        return t.to_string();
    }
    // "other": look the Zig type up in the overlay (exact, then by its generic name, e.g. Limit(...)).
    if let Some(mapped) = string(type_map, zig_type) {
        return mapped;
    }
    if let Some(paren) = zig_type.find('(').filter(|&p| p > 0)
        && let Some(generic) = string(type_map, &zig_type[..paren])
    {
        return generic;
    }
    diags.push(warning(
        "GHO001",
        format!("'{key}' has Zig type {zig_type} with no widget mapping; it's shown as text."),
    ));
    if repeatable { "strings" } else { "string" }.to_string()
}

/// Zig types whose values are short (the overlay's "compactTypes", with or without generics).
fn is_compact_type(zig_type: &str, compact_types: &[String]) -> bool {
    let bare = match zig_type.find('(') {
        Some(p) if p > 0 => &zig_type[..p],
        _ => zig_type,
    };
    compact_types.iter().any(|t| t == zig_type || t == bare)
}

fn record_fields(kind: &str) -> Vec<Field> {
    let field = |key: &str, label: &str, ty, placeholder: &str, list: Option<&str>| Field {
        key: key.into(),
        label: label.into(),
        ty,
        placeholder: Some(placeholder.into()),
        list: list.map(str::to_string),
    };
    if kind == "palette" {
        vec![
            field("index", "Index", SettingType::String, "0–255", None),
            field("color", "Color", SettingType::Color, "#rrggbb", None),
        ]
    } else {
        vec![
            field(
                "trigger",
                "Trigger",
                SettingType::String,
                "super+shift+t",
                None,
            ),
            field(
                "action",
                "Action",
                SettingType::String,
                "new_tab",
                Some(ACTIONS_LIST),
            ),
        ]
    }
}

fn default_for(ty: SettingType, defaults: &[String], opt: &Obj) -> Value {
    let first = defaults.first().map(String::as_str);
    let text = || match first {
        Some(s) if !s.is_empty() => Value::String(s.to_string()),
        _ => Value::Null,
    };
    match ty {
        SettingType::Bool => match first {
            Some("true") => Value::Bool(true),
            Some("false") => Value::Bool(false),
            _ => Value::Null,
        },
        SettingType::Int => first
            .and_then(|s| s.trim().parse::<i64>().ok())
            .map_or(Value::Null, Value::from),
        SettingType::Float => first
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|d| d.is_finite())
            .map_or(Value::Null, Value::from),
        SettingType::Flags => Value::Array(
            objects(opt, "flags")
                .filter(|f| boolean(f, "default"))
                .map(|f| string(f, "name").map_or(Value::Null, Value::String))
                .collect(),
        ),
        SettingType::Strings | SettingType::Fonts => {
            if defaults.len() > MAX_LISTED_DEFAULTS {
                Value::Array(Vec::new())
            } else {
                Value::Array(
                    defaults
                        .iter()
                        .filter(|v| !v.is_empty())
                        .map(|v| Value::String(v.clone()))
                        .collect(),
                )
            }
        }
        SettingType::Records => Value::Array(Vec::new()),
        SettingType::FontStyle if first == Some("false") => Value::Bool(false),
        _ => text(),
    }
}

fn group_for(key: &str, rules: &[(String, String)]) -> String {
    // Longest matching prefix wins, so "background-image" beats "background"; ties go to the first rule.
    let mut best: Option<&(String, String)> = None;
    for rule in rules.iter().filter(|r| key.starts_with(r.1.as_str())) {
        if best.is_none_or(|b| rule.1.len() > b.1.len()) {
            best = Some(rule);
        }
    }
    best.map_or_else(|| "general".to_string(), |r| r.0.clone())
}

fn platforms_by_prefix(key: &str, prefixes: &Obj) -> Option<Vec<String>> {
    for (prefix, platforms) in prefixes {
        if key.starts_with(prefix.as_str()) {
            return platforms.as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });
        }
    }
    None
}

/// First paragraph as the summary, the rest (markdown) behind "More".
fn split_doc(doc: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(doc) = doc.filter(|d| !d.trim().is_empty()) else {
        return (None, None);
    };
    let normalized = doc.replace("\r\n", "\n");
    let text = normalized.trim_matches('\n');
    let (summary, more) = match text.find("\n\n") {
        Some(cut) => (&text[..cut], Some(text[cut + 2..].trim_matches('\n'))),
        None => (text, None),
    };
    let summary = summary.replace('\n', " ").trim().to_string();
    let more = more.filter(|m| !m.trim().is_empty()).map(str::to_string);
    (Some(summary), more)
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE: &str = r##"{
      "ghosttyVersion": "9.9.9",
      "options": [
        { "key": "font-size", "zigType": "f32", "kind": "float", "optional": false, "repeatable": false,
          "defaults": ["13"], "doc": "Font size in points.\nSecond line.\n\nMore text\n\n    code", "platforms": null,
          "since": null, "reload": "runtime", "values": null, "flags": null },
        { "key": "cursor-style", "zigType": "CursorStyle", "kind": "enum", "optional": false, "repeatable": false,
          "defaults": ["block"], "doc": "", "platforms": null, "since": "1.1.0", "reload": "restart",
          "values": ["block", "bar"], "flags": null },
        { "key": "window-theme", "zigType": "WindowTheme", "kind": "enum", "optional": false, "repeatable": false,
          "defaults": ["auto"], "doc": null, "platforms": null, "since": null, "reload": "new-terminals",
          "values": null, "flags": null },
        { "key": "shell-integration-features", "zigType": "ShellIntegrationFeatures", "kind": "flags",
          "optional": false, "repeatable": false, "defaults": ["cursor,no-sudo"], "doc": null, "platforms": null,
          "values": null, "flags": [ { "name": "cursor", "default": true }, { "name": "sudo", "default": false } ] },
        { "key": "background", "zigType": "Color", "kind": "color", "optional": false, "repeatable": false,
          "defaults": ["#282c34"], "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "cursor-color", "zigType": "?TerminalColor", "kind": "other", "optional": true, "repeatable": false,
          "defaults": [], "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "keybind", "zigType": "Keybinds", "kind": "keybind", "optional": false, "repeatable": true,
          "defaults": ["a=b","c=d","e=f","g=h","i=j","k=l","m=n","o=p","q=r","s=t","u=v","w=x","y=z"],
          "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "macos-titlebar-style", "zigType": "MacTitlebarStyle", "kind": "other", "optional": false,
          "repeatable": false, "defaults": ["transparent"], "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "language", "zigType": "?[:0]const u8", "kind": "string", "optional": true, "repeatable": false,
          "defaults": [], "doc": null, "platforms": ["linux"], "values": null, "flags": null },
        { "key": "image-storage-limit", "zigType": "Limit(u32)", "kind": "other", "optional": false,
          "repeatable": false, "defaults": ["320000000"], "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "quit-after-last-window-closed-delay", "zigType": "?Duration", "kind": "duration", "optional": true,
          "repeatable": false, "defaults": [], "doc": null, "platforms": null, "values": null, "flags": null },
        { "key": "font-style", "zigType": "FontStyle", "kind": "other", "optional": false, "repeatable": false,
          "defaults": ["false"], "doc": null, "platforms": null, "values": null, "flags": null }
      ]
    }"##;

    const OVERLAY: &str = r#"{
      "app": { "terminal": "Ghostty", "brand": "ghostty" },
      "groups": [ { "id": "general", "title": "General" }, { "id": "fonts", "title": "Fonts" },
                  { "id": "colors", "title": "Colors" }, { "id": "cursor", "title": "Cursor" },
                  { "id": "macos", "title": "macOS" }, { "id": "advanced", "title": "Advanced" } ],
      "rules": [ { "group": "fonts", "prefix": ["font-"] }, { "group": "colors", "prefix": ["background"] },
                 { "group": "cursor", "prefix": ["cursor-"] }, { "group": "macos", "prefix": ["macos-"] },
                 { "group": "advanced", "prefix": ["image-storage-limit"] }, { "group": "general", "prefix": [""] } ],
      "platformPrefixes": { "macos-": ["macos"] },
      "typeMap": { "?TerminalColor": "color", "Limit": "string", "FontStyle": "fontStyle" },
      "compactTypes": ["Limit"],
      "colorKeywords": { "?TerminalColor": ["cell-foreground", "cell-background"] },
      "settings": {
        "font-size": { "min": 4, "max": 72, "step": 0.5, "unit": "pt" },
        "cursor-style": { "nullable": true },
        "gone-option": { "min": 1 }
      }
    }"#;

    fn merge(source: &str, overlay: &str) -> (Schema, Vec<SchemaDiagnostic>) {
        merge_schema(source, overlay).expect("test JSON parses")
    }

    fn get<'a>(s: &'a Schema, key: &str) -> &'a Setting {
        s.find(key).unwrap_or_else(|| panic!("{key} merged"))
    }

    #[test]
    fn app_block_and_revision() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(schema.revision, "9.9.9");
        assert_eq!(schema.title, "Ghostty settings");
        assert_eq!(schema.brand, "ghostty");
        assert!(schema.scopes.is_empty());
        assert!(!schema.theme_locks_colors);
    }

    #[test]
    fn source_order_types_groups_and_labels() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(schema.settings[0].key, "font-size");
        let size = get(&schema, "font-size");
        assert_eq!(size.ty, SettingType::Float);
        assert_eq!(size.group, "fonts");
        assert_eq!(size.label, "Font size");
        assert_eq!(size.default, json!(13.0));
        assert_eq!(
            (size.min, size.max, size.step),
            (Some(4.0), Some(72.0), Some(0.5))
        );
        assert_eq!(size.unit.as_deref(), Some("pt"));
        assert!(!size.nullable);
        assert_eq!(get(&schema, "language").group, "general");
        assert_eq!(get(&schema, "image-storage-limit").group, "advanced");
    }

    #[test]
    fn doc_splits_into_summary_and_more() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let size = get(&schema, "font-size");
        assert_eq!(
            size.doc.as_deref(),
            Some("Font size in points. Second line.")
        );
        assert_eq!(size.doc_more.as_deref(), Some("More text\n\n    code"));
        assert_eq!(get(&schema, "cursor-style").doc, None);
    }

    #[test]
    fn tags_from_since_and_reload() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(
            get(&schema, "cursor-style").tags,
            ["Since 1.1.0", "Restart needed"]
        );
        assert_eq!(get(&schema, "window-theme").tags, ["New windows only"]);
        assert!(get(&schema, "font-size").tags.is_empty());
    }

    #[test]
    fn enum_without_values_becomes_text_with_a_warning() {
        let (schema, diags) = merge(SOURCE, OVERLAY);
        assert_eq!(get(&schema, "window-theme").ty, SettingType::String);
        assert!(diags.iter().any(|d| d.id == "GHO004" && !d.is_error));
        let style = get(&schema, "cursor-style");
        assert_eq!(style.ty, SettingType::Enum);
        assert!(style.nullable, "overlay nullable wins");
        assert_eq!(style.default, json!("block"));
    }

    #[test]
    fn flags_take_names_and_default_flags() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let f = get(&schema, "shell-integration-features");
        assert_eq!(f.ty, SettingType::Flags);
        assert_eq!(
            f.values.as_deref(),
            Some(&["cursor".to_string(), "sudo".to_string()][..])
        );
        assert_eq!(f.default, json!(["cursor"]));
    }

    #[test]
    fn colors_get_keywords_and_null_defaults_are_nullable() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let cursor = get(&schema, "cursor-color");
        assert_eq!(cursor.ty, SettingType::Color);
        assert_eq!(
            cursor.values.as_deref(),
            Some(&["cell-foreground".to_string(), "cell-background".to_string()][..])
        );
        assert!(cursor.nullable);
        assert_eq!(cursor.default, Value::Null);
        assert_eq!(get(&schema, "background").default, json!("#282c34"));
    }

    #[test]
    fn keybinds_are_records_with_a_built_ins_note() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let kb = get(&schema, "keybind");
        assert_eq!(kb.ty, SettingType::Records);
        assert!(kb.repeatable);
        assert_eq!(kb.default, json!([]));
        assert_eq!(kb.built_ins_note.as_deref(), Some("13 built-in entries"));
        let fields = kb.fields.as_ref().unwrap();
        assert_eq!(fields[0].key, "trigger");
        assert_eq!(fields[1].list.as_deref(), Some(ACTIONS_LIST));
    }

    #[test]
    fn platforms_from_source_or_prefix() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(
            get(&schema, "language").platforms.as_deref(),
            Some(&["linux".to_string()][..])
        );
        assert_eq!(
            get(&schema, "macos-titlebar-style").platforms.as_deref(),
            Some(&["macos".to_string()][..])
        );
        assert_eq!(get(&schema, "font-size").platforms, None);
    }

    #[test]
    fn unmapped_zig_types_warn_and_generic_types_map_by_name() {
        let (schema, diags) = merge(SOURCE, OVERLAY);
        assert_eq!(get(&schema, "macos-titlebar-style").ty, SettingType::String);
        assert!(
            diags
                .iter()
                .any(|d| d.id == "GHO001" && d.message.contains("macos-titlebar-style"))
        );
        let limit = get(&schema, "image-storage-limit");
        assert_eq!(limit.ty, SettingType::String);
        assert!(limit.compact);
        assert_eq!(limit.default, json!("320000000"));
    }

    #[test]
    fn durations_are_compact_with_a_placeholder() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let d = get(&schema, "quit-after-last-window-closed-delay");
        assert_eq!(d.ty, SettingType::String);
        assert!(d.compact);
        assert_eq!(d.placeholder.as_deref(), Some("e.g. 750ms or 5s"));
        assert!(d.nullable);
    }

    #[test]
    fn font_style_false_default_is_a_bool() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let s = get(&schema, "font-style");
        assert_eq!(s.ty, SettingType::FontStyle);
        assert_eq!(s.default, json!(false));
    }

    #[test]
    fn overlay_keys_missing_from_source_warn() {
        let (_, diags) = merge(SOURCE, OVERLAY);
        assert!(
            diags
                .iter()
                .any(|d| d.id == "GHO003" && !d.is_error && d.message.contains("gone-option"))
        );
    }

    #[test]
    fn unknown_rule_group_and_unknown_type_are_errors() {
        let overlay = OVERLAY
            .replace(
                r#""group": "macos", "prefix""#,
                r#""group": "nope", "prefix""#,
            )
            .replace(r#""unit": "pt""#, r#""unit": "pt", "type": "slider""#);
        let (schema, diags) = merge(SOURCE, &overlay);
        for id in ["GHO006", "GHO007"] {
            assert!(
                diags.iter().any(|d| d.id == id && d.is_error),
                "{id} in {diags:?}"
            );
        }
        assert_eq!(get(&schema, "font-size").ty, SettingType::String);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(merge_schema("[]", OVERLAY).is_err());
        assert!(merge_schema(SOURCE, "{").is_err());
    }

    #[test]
    fn real_schema_files_merge_without_errors() {
        let errors: Vec<String> = crate::schema_diagnostics()
            .iter()
            .filter(|d| d.is_error)
            .map(ToString::to_string)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let schema = crate::schema();
        assert!(schema.settings.len() > 200);
        let groups: HashSet<&str> = schema.groups.iter().map(|g| g.id.as_str()).collect();
        for s in &schema.settings {
            assert!(
                groups.contains(s.group.as_str()),
                "{} in {}",
                s.key,
                s.group
            );
        }
    }
}
