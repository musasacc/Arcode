//! What the presence shows, and the `activity` object sent to Discord.
//!
//! Only what [`Shown`] holds can be sent: a file's name (never its path),
//! its language, the project's folder name and when the project was
//! opened. The privacy switches of [`Settings`] decide which of them
//! appear.

use serde_json::{Map, Value, json};

/// The longest text Discord accepts in an activity field, in characters.
pub const MAX_FIELD_CHARS: usize = 128;
/// Discord rejects text fields shorter than this.
pub const MIN_FIELD_CHARS: usize = 2;
/// Asset key and hover text of the large image.
pub const LARGE_IMAGE: (&str, &str) = ("arcode", "Arcode");

/// The presence settings (`[discord]` in `config.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Show the presence at all.
    pub enabled: bool,
    /// "Editing main.rs" instead of "Editing a Rust file".
    pub show_file_name: bool,
    /// "In <project>"; off: no project name at all.
    pub show_project_name: bool,
    /// Elapsed time since the project was opened.
    pub show_elapsed: bool,
    /// Discord application ID; empty: [`crate::DEFAULT_APPLICATION_ID`].
    pub application_id: String,
}

impl Settings {
    /// The application ID to connect with, if there is one.
    pub fn effective_application_id(&self) -> Option<&str> {
        let id = self.application_id.trim();
        let id = if id.is_empty() {
            crate::DEFAULT_APPLICATION_ID
        } else {
            id
        };
        (!id.is_empty()).then_some(id)
    }

    /// The application ID while the presence is on and has one.
    pub fn active_application_id(&self) -> Option<&str> {
        self.enabled
            .then(|| self.effective_application_id())
            .flatten()
    }
}

/// The language of the active file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Language {
    /// Lowercase key (`rust`, `python`, `typescript`), also the asset key
    /// of the small image.
    pub key: String,
    /// Display name (`Rust`).
    pub name: String,
}

/// What the window shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    /// The welcome screen: no project.
    Choosing,
    /// A project without an active file.
    Browsing,
    /// A file in the active editor.
    Editing {
        /// The file's name. Anything up to a path separator is dropped.
        file_name: String,
        /// Its language.
        language: Language,
    },
}

/// What the presence describes: the active window's view and project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The view.
    pub view: View,
    /// The project's folder name. Anything up to a path separator is
    /// dropped.
    pub project: Option<String>,
    /// When the project was opened, in Unix seconds.
    pub started: Option<u64>,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            view: View::Choosing,
            project: None,
            started: None,
        }
    }
}

/// The `activity` object for `shown`. `idle` replaces what is being done
/// with "Idle".
pub fn activity(shown: &Shown, settings: &Settings, idle: bool) -> Value {
    let mut activity = Map::new();
    let details = if idle {
        "Idle".to_owned()
    } else {
        match &shown.view {
            View::Choosing => "Choosing a project".to_owned(),
            View::Browsing => "Browsing files".to_owned(),
            View::Editing {
                file_name,
                language,
            } => match base_name(file_name) {
                Some(name) if settings.show_file_name => format!("Editing {name}"),
                _ => format!("Editing {}", file_kind(language)),
            },
        }
    };
    insert(&mut activity, "details", &details);
    if settings.show_project_name
        && let Some(project) = shown.project.as_deref().and_then(base_name)
    {
        insert(&mut activity, "state", &format!("In {project}"));
    }
    if settings.show_elapsed
        && shown.project.is_some()
        && let Some(start) = shown.started
    {
        activity.insert("timestamps".into(), json!({ "start": start }));
    }
    let mut assets = Map::new();
    insert(&mut assets, "large_image", LARGE_IMAGE.0);
    insert(&mut assets, "large_text", LARGE_IMAGE.1);
    if let (false, View::Editing { language, .. }) = (idle, &shown.view) {
        let key = asset_key(&language.key);
        if !key.is_empty() {
            // Discord wants at least two characters: C's key is `clang`.
            let key = if key.len() < MIN_FIELD_CHARS {
                format!("{key}lang")
            } else {
                key
            };
            insert(&mut assets, "small_image", &key);
            let name = clean(&language.name);
            let name = if name.chars().count() < MIN_FIELD_CHARS {
                format!("{name} language")
            } else {
                name
            };
            insert(&mut assets, "small_text", &name);
        }
    }
    activity.insert("assets".into(), Value::Object(assets));
    Value::Object(activity)
}

/// "a Rust file", "an HTML file", "a text file".
fn file_kind(language: &Language) -> String {
    let name = clean(&language.name);
    if language.key == "plaintext" || name.is_empty() {
        return "a text file".to_owned();
    }
    let first = name.chars().next().unwrap_or('x').to_ascii_uppercase();
    let vowel = matches!(first, 'A' | 'E' | 'I' | 'O') || name.starts_with("HTML");
    format!("{} {name} file", if vowel { "an" } else { "a" })
}

/// The last component of `name` (a path separator never gets through),
/// or `None` when that is empty.
fn base_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let last = clean(last);
    (!last.is_empty()).then_some(last)
}

/// `text` without control characters, trimmed.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// A lowercase asset key: letters, digits, `-` and `_` only.
fn asset_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `text` cut to [`MAX_FIELD_CHARS`] characters (on a char boundary, with
/// an ellipsis), or `None` when it is shorter than [`MIN_FIELD_CHARS`].
pub fn field_text(text: &str) -> Option<String> {
    let text = clean(text);
    let count = text.chars().count();
    if count < MIN_FIELD_CHARS {
        return None;
    }
    if count <= MAX_FIELD_CHARS {
        return Some(text);
    }
    let mut cut: String = text.chars().take(MAX_FIELD_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    Some(cut)
}

fn insert(map: &mut Map<String, Value>, key: &str, text: &str) {
    if let Some(text) = field_text(text) {
        map.insert(key.to_owned(), Value::String(text));
    }
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
