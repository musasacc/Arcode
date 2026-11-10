//! The `[run]` section of `config.toml`: where Run Current File and Debug
//! Current File put the programs they compile from a single source file.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// Where a program compiled from one source file (`rustc test.rs`,
/// `cc main.c`) goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutput {
    /// Next to the source file (`test.rs` → `test`, `test.exe` on
    /// Windows), run from the file's folder. A name already used by a
    /// folder, a file that is not a program, or a program Arcode did not
    /// build is never overwritten: the program then goes to the cache.
    #[default]
    NextToSource,
    /// A folder per source file in Arcode's cache, out of the project.
    Cache,
}

impl RunOutput {
    /// Every choice, in the order the Settings page lists them.
    pub const ALL: [Self; 2] = [Self::NextToSource, Self::Cache];

    /// The TOML value (`"next_to_source"`, `"cache"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NextToSource => "next_to_source",
            Self::Cache => "cache",
        }
    }
}

/// `[run]`: running single files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RunSettings {
    /// Where compiled single-file programs go.
    pub output: RunOutput,
}

impl_section!(RunSettings { output });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn next_to_source_by_default_and_cache_on_request() {
        assert_eq!(Config::default().run.output, RunOutput::NextToSource);
        for choice in RunOutput::ALL {
            let toml = format!("[run]\noutput = \"{}\"\n", choice.as_str());
            let (config, warnings) = Config::from_toml_str(&toml);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(config.run.output, choice);
        }
    }

    #[test]
    fn an_unknown_value_warns_and_keeps_the_default() {
        let (config, warnings) = Config::from_toml_str("[run]\noutput = \"desktop\"\n");
        assert_eq!(config.run.output, RunOutput::NextToSource);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("run.output"), "{warnings:?}");
    }
}
