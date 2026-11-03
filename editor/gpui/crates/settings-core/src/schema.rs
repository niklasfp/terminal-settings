use serde_json::Value;

/// Widget/value kind of a setting. Mirrors the overlay's `type` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingType {
    Bool,
    Int,
    Float,
    String,
    Path,
    Font,
    Theme,
    Enum,
    Color,
    Strings,
    Flags,
    Margin,
    Pair,
    Blur,
    FontStyle,
    Records,
    Raw,
    Fonts,
}

impl SettingType {
    /// Parses an overlay `type` string ("bool", "fontStyle", ...).
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// Every type, in declaration order.
    pub const ALL: [SettingType; 18] = [
        Self::Bool,
        Self::Int,
        Self::Float,
        Self::String,
        Self::Path,
        Self::Font,
        Self::Theme,
        Self::Enum,
        Self::Color,
        Self::Strings,
        Self::Flags,
        Self::Margin,
        Self::Pair,
        Self::Blur,
        Self::FontStyle,
        Self::Records,
        Self::Raw,
        Self::Fonts,
    ];

    /// The overlay `type` string for this type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "string",
            Self::Path => "path",
            Self::Font => "font",
            Self::Theme => "theme",
            Self::Enum => "enum",
            Self::Color => "color",
            Self::Strings => "strings",
            Self::Flags => "flags",
            Self::Margin => "margin",
            Self::Pair => "pair",
            Self::Blur => "blur",
            Self::FontStyle => "fontStyle",
            Self::Records => "records",
            Self::Raw => "raw",
            Self::Fonts => "fonts",
        }
    }
}

/// One column of a `Records` setting.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub ty: SettingType,
    pub placeholder: Option<String>,
    /// Name of an app suggestion list for this column (see `AppDefinition::lists`), e.g. "actions".
    pub list: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: String,
    pub title: String,
    /// Icon id from the overlay ("sliders", "window", ...).
    pub icon: String,
    pub doc: Option<String>,
}

/// One setting, merged from the generated source layer and the hand-written overlay.
#[derive(Debug, Clone)]
pub struct Setting {
    pub key: String,
    /// `key` split on '.'.
    pub path: Vec<String>,
    pub group: String,
    pub subgroup: Option<String>,
    pub label: String,
    pub ty: SettingType,
    pub doc: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
    pub placeholder: Option<String>,
    /// Allowed values for enum/flags (and keyword suggestions for colors).
    pub values: Option<Vec<String>>,
    pub nullable: bool,
    /// Platforms the setting applies to ("macos", "linux", "windows"); None = all.
    pub platforms: Option<Vec<String>>,
    /// The terminal's default, already normalised (enum spelling, margin as 1/2/4 numbers). Null = the terminal decides.
    pub default: Value,
    /// Can be overridden per platform scope.
    pub overridable: bool,
    /// In the terminal's source but not described in the overlay.
    pub inferred: bool,
    /// File picker for path-like fields: "image", "audio", "file" or "folder".
    pub pick: Option<String>,
    pub fields: Option<Vec<Field>>,
    /// The key may appear several times in the file; the value is the list of all of them.
    pub repeatable: bool,
    /// Longer docs (markdown) shown behind "More".
    pub doc_more: Option<String>,
    /// Short badges such as "Restart needed" or "Since 1.3.0".
    pub tags: Vec<String>,
    /// For repeatable keys whose built-in defaults are too long to list, e.g. "120 built-in entries".
    pub built_ins_note: Option<String>,
    /// Short free-text values (durations, "10%"): rendered as a compact field.
    pub compact: bool,
}

/// A terminal's settings schema.
#[derive(Debug, Clone)]
pub struct Schema {
    /// Source revision the schema was generated from.
    pub revision: String,
    pub terminal: String,
    pub title: String,
    /// Brand id ("rio", "ghostty"): selects the palette.
    pub brand: String,
    /// Platform scopes for "Applies to"; empty hides the control.
    pub scopes: Vec<String>,
    /// The terminal ignores [colors] while a theme is set.
    pub theme_locks_colors: bool,
    pub groups: Vec<Group>,
    /// In display order.
    pub settings: Vec<Setting>,
}

impl Setting {
    /// A setting with every optional part empty; mergers fill in what the schema says.
    pub fn new(key: &str, group: impl Into<String>, ty: SettingType) -> Self {
        Self {
            key: key.to_string(),
            path: key.split('.').map(str::to_string).collect(),
            group: group.into(),
            subgroup: None,
            label: String::new(),
            ty,
            doc: None,
            min: None,
            max: None,
            step: None,
            unit: None,
            placeholder: None,
            values: None,
            nullable: false,
            platforms: None,
            default: Value::Null,
            overridable: false,
            inferred: false,
            pick: None,
            fields: None,
            repeatable: false,
            doc_more: None,
            tags: Vec::new(),
            built_ins_note: None,
            compact: false,
        }
    }
}

impl Schema {
    pub fn find(&self, key: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.key == key)
    }
}
