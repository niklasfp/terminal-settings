//! Merges the generated layer (rio-source.json: Rio's defaults dump, enums, color defaults)
//! with the hand-written layer (rio-overlay.json: labels, docs, ranges, widgets).

use serde_json::Value;
use settings_core::merge::{Obj, boolean, error, humanize, number, obj, objects, string, strings, warning};
use settings_core::{Field, Group, Schema, SchemaDiagnostic, Setting, SettingType};
use std::collections::HashSet;

// Keys in the dump that are containers handled elsewhere, not settings.
const IGNORED_DUMP_KEYS: [&str; 1] = ["platform"];

/// Merges the two layers. `Err` only when either file isn't a JSON object.
pub fn merge_schema(
    source_json: &str,
    overlay_json: &str,
) -> Result<(Schema, Vec<SchemaDiagnostic>), String> {
    let source: Obj =
        serde_json::from_str(source_json).map_err(|e| format!("rio-source.json: {e}"))?;
    let overlay: Obj =
        serde_json::from_str(overlay_json).map_err(|e| format!("rio-overlay.json: {e}"))?;
    let empty = Obj::new();
    let defaults = obj(&source, "defaults").unwrap_or(&empty);
    let enums = obj(&source, "enums").unwrap_or(&empty);
    let color_defaults = obj(&source, "colorDefaults").unwrap_or(&empty);
    let mut diags = Vec::new();

    let app = obj(&overlay, "app").unwrap_or(&empty);
    let terminal = string(app, "terminal").unwrap_or_else(|| "Terminal".into());
    let mut schema = Schema {
        revision: string(&source, "rioVersion")
            .or_else(|| string(&source, "rioRevision"))
            .unwrap_or_else(|| "unknown".into()),
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
    let group_ids: HashSet<String> = schema.groups.iter().map(|g| g.id.clone()).collect();

    let override_prefixes = strings(&overlay, "platformOverridable").unwrap_or_default();
    let override_exclude: HashSet<String> = strings(&overlay, "platformOverrideExclude")
        .unwrap_or_default()
        .into_iter()
        .collect();
    let is_overridable = |key: &str| {
        !override_exclude.contains(key)
            && override_prefixes.iter().any(|p| {
                if p.ends_with('.') {
                    key.starts_with(p.as_str())
                } else {
                    key == p
                }
            })
    };

    let mut seen: HashSet<String> = HashSet::new();

    for so in objects(&overlay, "settings") {
        let key = string(so, "key").unwrap_or_default();
        if !seen.insert(key.clone()) {
            diags.push(error(
                "RIO005",
                format!("Setting '{key}' is listed twice in rio-overlay.json."),
            ));
            continue;
        }
        let type_name = string(so, "type").unwrap_or_else(|| "string".into());
        let ty = SettingType::parse(&type_name).unwrap_or_else(|| {
            diags.push(error(
                "RIO006",
                format!("Setting '{key}' has unknown type '{type_name}'."),
            ));
            SettingType::String
        });
        let group = string(so, "group").unwrap_or_else(|| "general".into());
        if !group_ids.contains(&group) {
            diags.push(error(
                "RIO007",
                format!("Setting '{key}' uses unknown group '{group}'."),
            ));
        }

        let mut setting = new_setting(&key, group, ty);
        setting.subgroup = string(so, "subgroup");
        setting.label = string(so, "label").unwrap_or_default();
        setting.doc = string(so, "doc");
        setting.min = number(so, "min");
        setting.max = number(so, "max");
        setting.step = number(so, "step");
        setting.unit = string(so, "unit");
        setting.placeholder = string(so, "placeholder");
        setting.nullable = boolean(so, "nullable");
        setting.platforms = strings(so, "platforms");
        setting.pick = string(so, "pick");
        setting.values = resolve_values(&key, so, enums, &mut diags);
        setting.overridable = is_overridable(&key);
        if so.get("fields").is_some_and(Value::is_array) {
            setting.fields = Some(
                objects(so, "fields")
                    .map(|f| {
                        let fkey = string(f, "key").unwrap_or_default();
                        Field {
                            label: string(f, "label").unwrap_or_else(|| fkey.clone()),
                            ty: string(f, "type")
                                .and_then(|t| SettingType::parse(&t))
                                .unwrap_or(SettingType::String),
                            placeholder: string(f, "placeholder"),
                            list: string(f, "list"),
                            key: fkey,
                        }
                    })
                    .collect(),
            );
        }

        let dumped = lookup(defaults, &key);
        let manual = boolean(so, "manual");
        let default = match (so.get("default"), dumped) {
            (Some(explicit), _) => explicit.clone(),
            (None, Some(d)) => d.clone(),
            (None, None) => Value::Null,
        };
        if dumped.is_none() && !manual {
            diags.push(warning(
                "RIO003",
                format!(
                    "Setting '{key}' is not in Rio's defaults dump. It may have been removed from Rio; mark it \"manual\": true if it is skipped on purpose."
                ),
            ));
        }
        if dumped.is_some() && manual {
            diags.push(warning("RIO008", format!("Setting '{key}' is marked manual but Rio's defaults dump now has it; drop \"manual\".")));
        }
        setting.default = normalize_default(&setting, default, enums);
        schema.settings.push(setting);
    }

    // Colors come entirely from the source layer.
    add_colors(&mut schema, &overlay, color_defaults, &mut seen);

    // Anything Rio has that the overlay doesn't know: warn, but still render it.
    let mut leaves = Vec::new();
    collect_leaves(defaults, "", &seen, &mut leaves);
    for (key, value) in leaves {
        diags.push(warning("RIO001", format!("Rio setting '{key}' has no entry in rio-overlay.json; it is shown with a basic control.")));
        let first = key.split('.').next().unwrap_or_default();
        let group = if group_ids.contains(first) {
            first.to_string()
        } else {
            "general".into()
        };
        let mut setting = new_setting(&key, group, infer_type(value));
        setting.subgroup = Some("Other".into());
        setting.label = humanize(key.rsplit('.').next().unwrap_or(&key));
        setting.inferred = true;
        setting.nullable = value.is_null();
        setting.overridable = is_overridable(&key);
        setting.default = normalize_default(&setting, value.clone(), enums);
        schema.settings.push(setting);
    }

    Ok((schema, diags))
}

fn new_setting(key: &str, group: String, ty: SettingType) -> Setting {
    Setting::new(key, group, ty)
}


fn resolve_values(
    key: &str,
    so: &Obj,
    enums: &Obj,
    diags: &mut Vec<SchemaDiagnostic>,
) -> Option<Vec<String>> {
    let explicit = strings(so, "values");
    let Some(enum_name) = string(so, "enum") else {
        return explicit;
    };
    let Some(e) = obj(enums, &enum_name) else {
        diags.push(error("RIO002", format!("Setting '{key}' references enum '{enum_name}', which was not found in Rio's source.")));
        return explicit;
    };
    let from_source = enum_values(e);
    let Some(explicit) = explicit else {
        return Some(from_source);
    };

    let contains = |list: &[String], v: &str| list.iter().any(|x| x.eq_ignore_ascii_case(v));
    for v in &explicit {
        if !contains(&from_source, v) {
            diags.push(warning(
                "RIO004",
                format!("Setting '{key}' lists value '{v}', which Rio's enum {enum_name} no longer has (has: {}).", from_source.join(", ")),
            ));
        }
    }
    for v in &from_source {
        if !contains(&explicit, v) {
            diags.push(warning(
                "RIO004",
                format!(
                    "Rio's enum {enum_name} has value '{v}' that setting '{key}' does not list."
                ),
            ));
        }
    }
    Some(explicit)
}

/// The spelling users write in TOML: the first serde alias, else the (renamed) variant name.
fn enum_values(e: &Obj) -> Vec<String> {
    let rename_all = string(e, "renameAll");
    objects(e, "variants")
        .map(|v| {
            let name = string(v, "name").unwrap_or_default();
            match strings(v, "aliases").and_then(|a| a.into_iter().next()) {
                Some(alias) => alias,
                None => apply_rename_all(&name, rename_all.as_deref()),
            }
        })
        .collect()
}

fn apply_rename_all(name: &str, rename_all: Option<&str>) -> String {
    match rename_all {
        Some("lowercase") => name.to_lowercase(),
        Some("kebab-case") => {
            let mut out = String::new();
            for (i, c) in name.chars().enumerate() {
                if c.is_uppercase() && i > 0 {
                    out.push('-');
                }
                out.extend(c.to_lowercase());
            }
            out
        }
        _ => name.to_string(),
    }
}

fn normalize_default(setting: &Setting, value: Value, enums: &Obj) -> Value {
    match (setting.ty, &value) {
        (SettingType::Enum, Value::String(s)) if setting.values.is_some() => {
            let values = setting.values.as_deref().unwrap_or_default();
            // The dump writes variant names (e.g. "Block"); map to the TOML spelling ("block").
            if let Some(v) = values.iter().find(|v| v.eq_ignore_ascii_case(s)) {
                return Value::String(v.clone());
            }
            for e in enums.values().filter_map(Value::as_object) {
                let hit = objects(e, "variants")
                    .position(|x| string(x, "name").as_deref() == Some(s.as_str()));
                if let Some(spelled) = hit.and_then(|i| enum_values(e).into_iter().nth(i))
                    && values.contains(&spelled)
                {
                    return Value::String(spelled);
                }
            }
            value
        }
        (SettingType::Float, Value::Number(n)) if !n.is_f64() => {
            n.as_f64().map_or(value.clone(), Value::from)
        }
        (SettingType::Margin, Value::Object(m)) => {
            let side = |k: &str| number(m, k).unwrap_or(0.0);
            let (t, r, b, l) = (side("top"), side("right"), side("bottom"), side("left"));
            let nums: Vec<f64> = if t == r && r == b && b == l {
                vec![t]
            } else if t == b && r == l {
                vec![t, r]
            } else {
                vec![t, r, b, l]
            };
            Value::Array(nums.into_iter().map(Value::from).collect())
        }
        (SettingType::String, Value::Null) if !setting.nullable => Value::String(String::new()),
        // Raw blocks are edited as TOML text; empty means "Rio's built-in value".
        (SettingType::Raw, _) => Value::Null,
        _ => value,
    }
}

fn add_colors(
    schema: &mut Schema,
    overlay: &Obj,
    color_defaults: &Obj,
    seen: &mut HashSet<String>,
) {
    let subgroups: Vec<&Obj> = objects(overlay, "colorSubgroups").collect();
    let mut placed: Vec<(usize, usize, usize, Setting)> = Vec::new();
    for (idx, (name, value)) in color_defaults.iter().enumerate() {
        let key = format!("colors.{name}");
        if !seen.insert(key.clone()) {
            continue;
        }
        let mut order = subgroups.len();
        let mut title = Some("Other".to_string());
        let mut label = name.as_str();
        for (i, sg) in subgroups.iter().enumerate() {
            if strings(sg, "match").is_some_and(|m| m.iter().any(|x| x == name)) {
                order = i;
                title = string(sg, "title");
                break;
            }
            if let Some(prefix) = string(sg, "prefix")
                && let Some(rest) = name.strip_prefix(prefix.as_str())
            {
                order = i;
                title = string(sg, "title");
                label = rest;
                break;
            }
        }
        let mut setting = new_setting(&key, "colors".into(), SettingType::Color);
        setting.subgroup = title;
        setting.label = humanize(label);
        setting.default = value.clone();
        // Dim/light-foreground etc. have no fixed default: Rio derives them.
        setting.nullable = value.is_null();
        let match_index = subgroups.get(order).map_or(0, |sg| {
            strings(sg, "match")
                .and_then(|m| m.iter().position(|x| x == name))
                .unwrap_or(usize::MAX)
        });
        placed.push((order, match_index, idx, setting));
    }
    // Keep "Normal" colors in ANSI order: subgroup order, then the order listed in `match`.
    placed.sort_by_key(|p| (p.0, p.1, p.2));
    schema.settings.extend(placed.into_iter().map(|p| p.3));
}

fn lookup<'a>(root: &'a Obj, dotted: &str) -> Option<&'a Value> {
    let mut parts = dotted.split('.');
    let mut cur = root.get(parts.next()?)?;
    for part in parts {
        cur = cur.as_object()?.get(part)?;
    }
    Some(cur)
}

fn collect_leaves<'a>(
    o: &'a Obj,
    prefix: &str,
    known: &HashSet<String>,
    out: &mut Vec<(String, &'a Value)>,
) {
    for (k, v) in o {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        if IGNORED_DUMP_KEYS.contains(&key.as_str()) || known.contains(&key) {
            continue;
        }
        if let Value::Object(child) = v {
            // A known setting further down means this is just a table.
            let table_prefix = format!("{key}.");
            let has_known_child = known.iter().any(|x| x.starts_with(&table_prefix));
            if has_known_child || (!child.is_empty() && !looks_like_value(child)) {
                collect_leaves(child, &key, known, out);
                continue;
            }
        }
        out.push((key, v));
    }
}

fn looks_like_value(o: &Obj) -> bool {
    o.contains_key("top") && o.contains_key("left")
}

fn infer_type(v: &Value) -> SettingType {
    match v {
        Value::Bool(_) => SettingType::Bool,
        Value::Number(n) if n.is_f64() => SettingType::Float,
        Value::Number(_) => SettingType::Int,
        Value::Array(_) => SettingType::Strings,
        Value::Object(o) if looks_like_value(o) => SettingType::Margin,
        _ => SettingType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE: &str = r##"{
      "rioRevision": "abc1234def",
      "defaults": {
        "cursor": { "shape": "Block", "blinking": false },
        "margin": { "top": 2.0, "right": 2.0, "bottom": 2.0, "left": 2.0 },
        "brand-new-option": 3,
        "platform": { "macos": null }
      },
      "enums": {
        "CursorShape": { "renameAll": null, "variants": [
          { "name": "Block", "aliases": ["block"], "isDefault": true },
          { "name": "Beam", "aliases": ["beam"], "isDefault": false } ] }
      },
      "colorDefaults": { "background": "#0f0d0e", "dim-red": null, "light-red": "#c55555" }
    }"##;

    const OVERLAY: &str = r#"{
      "groups": [ { "id": "general", "title": "General" }, { "id": "cursor", "title": "Cursor" }, { "id": "colors", "title": "Colors" } ],
      "colorSubgroups": [ { "title": "Base", "match": ["background"] }, { "title": "Bright", "prefix": "light-" } ],
      "settings": [
        { "key": "cursor.shape", "group": "cursor", "label": "Shape", "type": "enum", "enum": "CursorShape" },
        { "key": "cursor.blinking", "group": "cursor", "label": "Blinking", "type": "bool" },
        { "key": "margin", "group": "general", "label": "Margin", "type": "margin" },
        { "key": "fonts.size", "group": "general", "label": "Size", "type": "float", "manual": true, "default": 14 }
      ]
    }"#;

    fn merge(source: &str, overlay: &str) -> (Schema, Vec<SchemaDiagnostic>) {
        merge_schema(source, overlay).expect("test JSON parses")
    }

    fn get<'a>(s: &'a Schema, key: &str) -> &'a Setting {
        s.find(key).unwrap_or_else(|| panic!("{key} merged"))
    }

    #[test]
    fn revision_prefers_rio_version_over_rio_revision() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(schema.revision, "abc1234def");
        let with_version = SOURCE.replace(
            "\"rioRevision\": \"abc1234def\"",
            "\"rioVersion\": \"v0.5.29\", \"rioRevision\": \"abc1234def\"",
        );
        let (schema, _) = merge(&with_version, OVERLAY);
        assert_eq!(schema.revision, "v0.5.29");
    }

    #[test]
    fn enum_values_and_default_use_the_toml_spelling() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let shape = get(&schema, "cursor.shape");
        assert_eq!(
            shape.values.as_deref(),
            Some(&["block".to_string(), "beam".to_string()][..])
        );
        assert_eq!(shape.default, json!("block"));
    }

    #[test]
    fn margin_struct_default_collapses_to_css_shorthand() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        assert_eq!(get(&schema, "margin").default, json!([2.0]));
    }

    #[test]
    fn float_default_from_an_integer_becomes_a_float() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let size = &get(&schema, "fonts.size").default;
        assert!(size.is_f64());
        assert_eq!(size.as_f64(), Some(14.0));
    }

    #[test]
    fn unknown_dump_keys_warn_and_still_render() {
        let (schema, diags) = merge(SOURCE, OVERLAY);
        assert!(
            diags
                .iter()
                .any(|d| d.id == "RIO001" && d.message.contains("brand-new-option"))
        );
        let inferred = get(&schema, "brand-new-option");
        assert!(inferred.inferred);
        assert_eq!(inferred.ty, SettingType::Int);
        assert_eq!(inferred.label, "Brand new option");
        assert!(
            !schema
                .settings
                .iter()
                .any(|s| s.key.starts_with("platform"))
        );
    }

    #[test]
    fn colors_come_from_source_with_subgroups_and_nullable_when_derived() {
        let (schema, _) = merge(SOURCE, OVERLAY);
        let bg = get(&schema, "colors.background");
        assert_eq!(bg.subgroup.as_deref(), Some("Base"));
        assert_eq!(bg.default, json!("#0f0d0e"));
        let light_red = get(&schema, "colors.light-red");
        assert_eq!(light_red.subgroup.as_deref(), Some("Bright"));
        assert_eq!(light_red.label, "Red");
        assert!(get(&schema, "colors.dim-red").nullable);
    }

    #[test]
    fn missing_enum_is_an_error_and_removed_setting_is_a_warning() {
        let overlay = OVERLAY
            .replace("\"enum\": \"CursorShape\"", "\"enum\": \"Nope\"")
            .replace("\"key\": \"cursor.blinking\"", "\"key\": \"cursor.gone\"");
        let (_, diags) = merge(SOURCE, &overlay);
        assert!(diags.iter().any(|d| d.id == "RIO002" && d.is_error));
        assert!(
            diags
                .iter()
                .any(|d| d.id == "RIO003" && !d.is_error && d.message.contains("cursor.gone"))
        );
    }

    #[test]
    fn duplicate_keys_unknown_types_and_groups_are_errors() {
        let overlay = OVERLAY
            .replace("\"key\": \"margin\"", "\"key\": \"cursor.blinking\"")
            .replace("\"type\": \"float\"", "\"type\": \"slider\"")
            .replace(
                "\"group\": \"cursor\", \"label\": \"Shape\"",
                "\"group\": \"nope\", \"label\": \"Shape\"",
            );
        let (_, diags) = merge(SOURCE, &overlay);
        for id in ["RIO005", "RIO006", "RIO007"] {
            assert!(
                diags.iter().any(|d| d.id == id && d.is_error),
                "{id} in {diags:?}"
            );
        }
    }

    #[test]
    fn real_schema_files_merge_without_errors() {
        let diags = crate::schema_diagnostics();
        let errors: Vec<String> = diags
            .iter()
            .filter(|d| d.is_error)
            .map(ToString::to_string)
            .collect();
        assert!(errors.is_empty(), "schema errors:\n{}", errors.join("\n"));
        assert!(crate::schema().settings.len() > 100);
    }

    #[test]
    fn real_schema_files_have_no_drift() {
        let diags: Vec<String> = crate::schema_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(diags.is_empty(), "schema diagnostics:\n{}", diags.join("\n"));
    }
}
