//! `plugin.toml`: parsing and validation (docs/PLUGINS.md §3).
//!
//! Unknown keys are errors everywhere. A [`Manifest`] only exists in
//! validated form: [`Manifest::parse`] runs serde, then `validate` checks
//! ids, names, patterns and settings. Limits are kept as written and clamped
//! with [`Limits::clamp`] against the user's maxima.

mod activation;
mod ids;
mod limits;
mod validate;

#[cfg(test)]
mod tests;

use serde::Deserialize;

pub use activation::{ActivationEvent, When};
pub use ids::{PluginId, QualifiedId, RESERVED_PREFIX, is_icon_name, is_language_id, is_local_id};
pub use limits::{EffectiveLimits, Limits, UserLimits};

/// API versions this host has bindings for (`api` in the manifest).
pub const SUPPORTED_APIS: &[&str] = &["0.1"];

/// Largest `plugin.toml` the host reads.
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Where a plugin was found. The order is the override order: a dev folder
/// replaces an installed plugin of the same id, which replaces a bundled one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PluginSource {
    /// Shipped with Arcode (`share/arcode/plugins`).
    Bundled,
    /// Installed by the user into the data dir.
    Installed,
    /// "Load Unpacked…" for this session.
    Dev,
}

/// A validated `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[plugin]`
    pub plugin: PluginInfo,
    /// `[capabilities]`; everything defaults to "none".
    #[serde(default)]
    pub capabilities: Capabilities,
    /// `[limits]`, as written (see [`Limits::clamp`]).
    #[serde(default)]
    pub limits: Limits,
    /// `[contributes]`
    #[serde(default)]
    pub contributes: Contributions,
}

/// `[plugin]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PluginInfo {
    /// Validated plugin id.
    pub id: PluginId,
    /// Display name.
    pub name: String,
    /// The plugin's own version (shown, not interpreted).
    pub version: String,
    /// API version, `major.minor`; must be in [`SUPPORTED_APIS`].
    pub api: String,
    /// Component file inside the plugin folder.
    #[serde(default = "default_entry")]
    pub entry: String,
    /// One-line description.
    #[serde(default)]
    pub description: Option<String>,
}

fn default_entry() -> String {
    "plugin.wasm".to_owned()
}

/// `[capabilities]`
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Capabilities {
    /// Project files through the WASI preopen.
    #[serde(default)]
    pub workspace: WorkspaceAccess,
    /// Which documents `editor.*` may read.
    #[serde(default)]
    pub editor: EditorAccess,
    /// Whether `editor.apply-edits` may be granted.
    #[serde(default)]
    pub editor_write: bool,
    /// Hosts the plugin may ask to contact.
    #[serde(default)]
    pub network: Option<NetworkCapability>,
}

/// `workspace = "none" | "read" | "read-write"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceAccess {
    /// No preopen.
    #[default]
    None,
    /// `/project` preopened read-only.
    Read,
    /// `/project` preopened read-write.
    ReadWrite,
}

/// `editor = "none" | "context" | "all-open"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditorAccess {
    /// No document access.
    #[default]
    None,
    /// The document a call is about, or documents of contributed languages.
    Context,
    /// Any open document (high risk, asked at enable).
    AllOpen,
}

/// `network = { hosts = [...], background = false }`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct NetworkCapability {
    /// Host patterns: `*`, `example.com`, `*.example.com`, optionally `:port`.
    pub hosts: Vec<String>,
    /// Requests outside user gestures; never granted by default.
    #[serde(default)]
    pub background: bool,
}

/// `[contributes]`
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Contributions {
    /// `[[contributes.languages]]`
    #[serde(default)]
    pub languages: Vec<LanguageContribution>,
    /// `[[contributes.commands]]`
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    /// `[[contributes.code-lenses]]`
    #[serde(default)]
    pub code_lenses: Vec<CodeLensContribution>,
    /// `[[contributes.panels]]`
    #[serde(default)]
    pub panels: Vec<PanelContribution>,
    /// `[[contributes.openers]]`
    #[serde(default)]
    pub openers: Vec<OpenerContribution>,
    /// `[[contributes.settings]]`
    #[serde(default)]
    pub settings: Vec<SettingContribution>,
}

/// A language known by file extension.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LanguageContribution {
    /// Global language id (not namespaced: editors and `when` clauses use it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Extensions without the dot.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Line comment token.
    #[serde(default)]
    pub line_comment: Option<String>,
}

/// A palette command.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CommandContribution {
    /// Manifest-local id.
    pub id: String,
    /// Palette title.
    pub title: String,
    /// Enablement; 0.1 supports only `language == <id>`.
    #[serde(default)]
    pub when: Option<When>,
    /// Default key binding (host key syntax, checked by the app).
    #[serde(default)]
    pub key: Option<String>,
}

/// Code lenses for documents of these languages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CodeLensContribution {
    /// Language ids.
    pub languages: Vec<String>,
}

/// Where a panel is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelLocation {
    /// Activity-bar entry and sidebar view.
    Sidebar,
    /// Bottom panel tab.
    Bottom,
    /// Editor tab.
    Editor,
    /// Editor tab beside the active one.
    EditorBeside,
}

/// A panel the plugin can open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PanelContribution {
    /// Manifest-local id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Default location.
    pub location: PanelLocation,
    /// Host icon name.
    #[serde(default)]
    pub icon: Option<String>,
}

/// Files the plugin shows instead of the text editor or binary notice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct OpenerContribution {
    /// Manifest-local id.
    pub id: String,
    /// Globs; without `/` they match the file name, otherwise the
    /// project-relative path.
    pub patterns: Vec<String>,
    /// Tab title / description.
    pub title: String,
}

/// Setting value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingType {
    /// `true` / `false`.
    Boolean,
    /// 64-bit integer.
    Integer,
    /// Free text.
    String,
    /// One of `options`.
    Enum,
}

/// A user-visible setting of the plugin.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SettingContribution {
    /// Manifest-local key.
    pub key: String,
    /// Value type.
    #[serde(rename = "type")]
    pub kind: SettingType,
    /// Default; must match `type`.
    pub default: toml::Value,
    /// Help text.
    #[serde(default)]
    pub description: Option<String>,
    /// Allowed values for `type = "enum"` (required there, refused elsewhere).
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

/// Why a manifest was refused. Messages are user-facing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The file could not be read.
    Io(String),
    /// TOML syntax, a missing or unknown key, or a wrongly typed value.
    Syntax(String),
    /// The plugin targets an API this host has no bindings for.
    UnsupportedApi {
        /// What the manifest asked for.
        requested: String,
    },
    /// A non-bundled plugin uses the reserved `arcode.` prefix.
    ReservedId(PluginId),
    /// Any other rule; the text names the offending field.
    Invalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read plugin.toml: {e}"),
            Self::Syntax(e) => write!(f, "invalid plugin.toml: {e}"),
            Self::UnsupportedApi { requested } => write!(
                f,
                "plugin requires plugin API {requested}; this Arcode supports {}",
                SUPPORTED_APIS.join(", ")
            ),
            Self::ReservedId(id) => write!(
                f,
                "plugin id `{id}` uses the reserved `{RESERVED_PREFIX}` prefix"
            ),
            Self::Invalid(e) => write!(f, "invalid plugin.toml: {e}"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// Parses and validates `text` for a plugin found in `source`.
    pub fn parse(text: &str, source: PluginSource) -> Result<Self, ManifestError> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|e| ManifestError::Syntax(e.to_string()))?;
        validate::validate(&manifest, source)?;
        Ok(manifest)
    }

    /// The plugin id.
    pub fn id(&self) -> &PluginId {
        &self.plugin.id
    }

    /// Events that activate the plugin; derived from contributions, there
    /// is no `startup` event (§3).
    pub fn activation_events(&self) -> Vec<ActivationEvent> {
        activation::events(self)
    }

    /// Setting defaults as a JSON object (`host.settings` merges user
    /// values over this).
    pub fn default_settings(&self) -> serde_json::Map<String, serde_json::Value> {
        self.contributes
            .settings
            .iter()
            .map(|s| {
                (
                    s.key.clone(),
                    serde_json::to_value(&s.default).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect()
    }
}
