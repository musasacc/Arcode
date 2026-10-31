//! `${...}` variables of launch configurations.

use std::path::{Path, PathBuf};

/// Values for `${...}` variables.
#[derive(Debug, Clone, Default)]
pub struct LaunchVars {
    /// The project folder.
    pub workspace_folder: PathBuf,
    /// The active file, if any.
    pub file: Option<PathBuf>,
}

impl LaunchVars {
    /// Replaces the variables in `text`. Unknown variables stay as they
    /// are; `${file...}` without an active file becomes empty.
    pub fn substitute(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = &after[..end];
            match self.value(name) {
                Some(value) => out.push_str(&value),
                None => out.push_str(&rest[start..start + 3 + end]),
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn value(&self, name: &str) -> Option<String> {
        let lossy = |p: &Path| p.to_string_lossy().into_owned();
        let file = self.file.as_deref();
        Some(match name {
            "workspaceFolder" => lossy(&self.workspace_folder),
            "workspaceFolderBasename" => self
                .workspace_folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "file" => file.map(lossy).unwrap_or_default(),
            "fileDirname" => file.and_then(Path::parent).map(lossy).unwrap_or_default(),
            "fileBasename" => file
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "fileBasenameNoExtension" => file
                .and_then(Path::file_stem)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => std::env::var(name.strip_prefix("env:")?).unwrap_or_default(),
        })
    }

    /// Substitutes and makes a path absolute (relative to the project).
    pub fn path(&self, text: &str) -> PathBuf {
        let path = PathBuf::from(self.substitute(text));
        if path.is_absolute() {
            path
        } else {
            self.workspace_folder.join(path)
        }
    }
}
