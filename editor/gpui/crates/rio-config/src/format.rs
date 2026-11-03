use crate::toml_doc::ConfigDocument;
use crate::values;
use serde_json::Value;
use settings_core::{
    Change, ConfigFormat, FieldErrors, FormatError, ParsedConfig, Schema, SettingType,
};
use std::collections::BTreeMap;

/// Rio's config.toml: span-splicing edits over toml_edit's parsed spans.
#[derive(Debug, Default, Clone, Copy)]
pub struct RioTomlFormat;

impl ConfigFormat for RioTomlFormat {
    fn parse(&self, text: &str, schema: &Schema) -> Result<ParsedConfig, String> {
        let doc = ConfigDocument::parse(text).map_err(|e| e.0)?;
        let mut raw = BTreeMap::new();
        for s in schema.settings.iter().filter(|s| s.ty == SettingType::Raw) {
            raw.insert(
                s.key.clone(),
                doc.get_table_array(&s.path).map_err(|e| e.0)?,
            );
        }
        Ok(ParsedConfig {
            values: doc.values(),
            raw,
            notices: Vec::new(),
        })
    }

    fn apply(
        &self,
        text: &str,
        changes: &[Change<'_>],
        scope: &str,
    ) -> Result<String, FormatError> {
        let mut doc = ConfigDocument::parse(text).map_err(|e| FormatError::Parse(e.0))?;
        let mut errors = FieldErrors::new();
        for change in changes {
            let setting = change.setting;
            let path: Vec<String> = if scope.is_empty() {
                setting.path.clone()
            } else {
                ["platform".to_string(), scope.to_string()]
                    .into_iter()
                    .chain(setting.path.iter().cloned())
                    .collect()
            };
            let result = if setting.ty == SettingType::Raw {
                doc.set_table_array(&path, change.value.as_str().unwrap_or_default())
                    .map_err(|e| e.0)
            } else if change.value == Value::Null {
                doc.remove(&path).map(|_| ()).map_err(|e| e.0)
            } else {
                values::to_toml(setting, &change.value)
                    .and_then(|literal| doc.set(&path, &literal).map_err(|e| e.0))
            };
            if let Err(message) = result {
                errors.insert(setting.key.clone(), message);
            }
        }
        if !errors.is_empty() {
            return Err(FormatError::Invalid(errors));
        }
        Ok(doc.into_text())
    }
}
