//! `ui.text_gamma` and `ui.text_contrast`: how text edges are blended.
//!
//! GPUI's Linux renderer draws text with grayscale anti-aliasing and reads
//! two tuning values from the environment when it creates a window's
//! renderer: `ZED_FONTS_GAMMA` (clamped to 1.0 - 2.2, default 1.8) and
//! `ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST` (at least 0, default 1.0). On
//! macOS (Core Text) and Windows (DirectWrite, which follows the system's
//! ClearType settings) there is no such knob, so the settings do nothing
//! there.

use std::ops::RangeInclusive;

use super::UiSettings;

/// The environment variable GPUI reads the text gamma from.
pub const TEXT_GAMMA_ENV: &str = "ZED_FONTS_GAMMA";
/// The environment variable GPUI reads the text contrast from.
pub const TEXT_CONTRAST_ENV: &str = "ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST";
/// Which of the two variables Arcode set, each with the value it had
/// before (`ZED_FONTS_GAMMA=2.0;ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST`, the
/// second unset). Programs started from Arcode inherit its environment, so
/// an Arcode started from Arcode's terminal reads this to tell its
/// parent's settings from a value set outside Arcode.
pub const TEXT_RENDERING_OUTER_ENV: &str = "ARCODE_TEXT_RENDERING_OUTER";

/// Whether `ui.text_gamma` and `ui.text_contrast` change anything on this
/// platform (GPUI's Vulkan renderer: Linux and FreeBSD).
pub const TEXT_RENDERING_TUNABLE: bool = cfg!(any(target_os = "linux", target_os = "freebsd"));

/// How the environment changes at startup so GPUI's renderer gets the text
/// gamma and contrast of the settings. See [`UiSettings::text_rendering_env`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextRenderingEnv {
    /// Variables to set (`Some`) or remove (`None`), in order.
    pub changes: Vec<(&'static str, Option<String>)>,
    /// Values set outside Arcode that the renderer uses because the
    /// matching setting is left at its default.
    pub outside: Vec<(&'static str, String)>,
}

impl UiSettings {
    /// The default text gamma, which is also GPUI's.
    pub const DEFAULT_TEXT_GAMMA: f32 = 1.8;
    /// The gamma GPUI uses when its variable is unset.
    pub const RENDERER_TEXT_GAMMA: f32 = 1.8;
    /// The gammas GPUI accepts (it uses steps of 0.1).
    pub const TEXT_GAMMA_RANGE: RangeInclusive<f32> = 1.0..=2.2;
    /// The default text contrast. Above GPUI's 1.0: at 1.0 dark text on
    /// light themes (and muted text) draws thin and washed out; 2.0 gives
    /// it firmer edges, and light text on dark themes is not affected.
    pub const DEFAULT_TEXT_CONTRAST: f32 = 2.0;
    /// The contrast GPUI uses when its variable is unset.
    pub const RENDERER_TEXT_CONTRAST: f32 = 1.0;
    /// The contrasts accepted here. GPUI has no upper limit, but past 8
    /// glyph edges are all but solid.
    pub const TEXT_CONTRAST_RANGE: RangeInclusive<f32> = 0.0..=8.0;

    /// The environment changes that give GPUI's renderer the text gamma and
    /// contrast of these settings, given the inherited environment.
    ///
    /// A setting changed from its default is exported. A setting left at
    /// its default lets a value set outside Arcode apply, and otherwise is
    /// exported only where it differs from GPUI's own default (the
    /// contrast). A value an Arcode up the process tree exported for its
    /// own settings is taken back (to what was there before it, if
    /// anything), so one Arcode's settings do not leak into another started
    /// from its terminal. Nothing changes where the settings do nothing.
    pub fn text_rendering_env(
        &self,
        inherited: impl Fn(&str) -> Option<String>,
    ) -> TextRenderingEnv {
        let mut plan = TextRenderingEnv::default();
        if !TEXT_RENDERING_TUNABLE {
            return plan;
        }
        let record = inherited(TEXT_RENDERING_OUTER_ENV);
        let recorded = parse_outer(record.as_deref().unwrap_or_default());
        let mut new_record = Vec::new();
        for (name, value, default, renderer, text) in [
            (
                TEXT_GAMMA_ENV,
                self.text_gamma,
                Self::DEFAULT_TEXT_GAMMA,
                Self::RENDERER_TEXT_GAMMA,
                format!("{:.1}", self.text_gamma),
            ),
            (
                TEXT_CONTRAST_ENV,
                self.text_contrast,
                Self::DEFAULT_TEXT_CONTRAST,
                Self::RENDERER_TEXT_CONTRAST,
                format!("{:.2}", self.text_contrast),
            ),
        ] {
            let current = inherited(name);
            // The value from outside Arcode: the one an Arcode up the
            // process tree replaced, or else whatever is there.
            let outer = match recorded.iter().find(|(n, _)| *n == name) {
                Some((_, before)) => before.map(str::to_string),
                None => current.clone(),
            };
            let changed = (value - default).abs() > 1e-3;
            let renderer_differs = (value - renderer).abs() > 1e-3;
            if changed || (outer.is_none() && renderer_differs) {
                plan.changes.push((name, Some(text)));
                new_record.push((name, outer));
            } else {
                if outer != current {
                    plan.changes.push((name, outer.clone()));
                }
                if let Some(outer) = outer {
                    plan.outside.push((name, outer));
                }
            }
        }
        let new_record = (!new_record.is_empty()).then(|| format_outer(&new_record));
        if new_record != record {
            plan.changes.push((TEXT_RENDERING_OUTER_ENV, new_record));
        }
        plan
    }
}

/// Reads [`TEXT_RENDERING_OUTER_ENV`]: `NAME=value` for a variable that
/// had a value, `NAME` for one that was unset.
fn parse_outer(record: &str) -> Vec<(&str, Option<&str>)> {
    record
        .split(';')
        .filter(|entry| !entry.is_empty())
        .map(|entry| match entry.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (entry, None),
        })
        .collect()
}

/// Writes [`TEXT_RENDERING_OUTER_ENV`]. A value with a `;` in it is no
/// number GPUI could read, so it is recorded as unset, which GPUI treats
/// the same.
fn format_outer(entries: &[(&str, Option<String>)]) -> String {
    entries
        .iter()
        .map(|(name, value)| match value {
            Some(value) if !value.contains(';') => format!("{name}={value}"),
            _ => (*name).to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// An environment of `vars`.
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    /// Applies `plan` to `vars`, as `export_env` does to the process.
    fn apply(vars: &[(&str, &str)], plan: &TextRenderingEnv) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = vars
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        for (name, value) in &plan.changes {
            out.retain(|(n, _)| n != name);
            if let Some(value) = value {
                out.push((name.to_string(), value.clone()));
            }
        }
        out.sort();
        out
    }

    fn pairs(vars: &[(String, String)]) -> Vec<(&str, &str)> {
        vars.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect()
    }

    fn ui(gamma: f32, contrast: f32) -> UiSettings {
        UiSettings {
            text_gamma: gamma,
            text_contrast: contrast,
            ..UiSettings::default()
        }
    }

    #[test]
    fn defaults_export_only_the_contrast_gpui_lacks() {
        let defaults = UiSettings::default();
        assert_eq!(defaults.text_gamma, 1.8);
        assert_eq!(defaults.text_contrast, 2.0);
        let plan = defaults.text_rendering_env(env(&[]));
        if !TEXT_RENDERING_TUNABLE {
            assert_eq!(plan, TextRenderingEnv::default());
            return;
        }
        // The gamma is GPUI's own default: left unset.
        assert_eq!(
            pairs(&apply(&[], &plan)),
            [
                (TEXT_RENDERING_OUTER_ENV, TEXT_CONTRAST_ENV),
                (TEXT_CONTRAST_ENV, "2.00"),
            ]
        );
        assert!(plan.outside.is_empty());
        // GPUI's own contrast, set on purpose, is exported too.
        let plan = ui(1.8, 1.0).text_rendering_env(env(&[]));
        assert_eq!(
            pairs(&apply(&[], &plan)),
            [
                (TEXT_RENDERING_OUTER_ENV, TEXT_CONTRAST_ENV),
                (TEXT_CONTRAST_ENV, "1.00"),
            ]
        );
    }

    #[test]
    fn changed_values_become_variables() {
        let plan = ui(2.2, 3.5).text_rendering_env(env(&[]));
        if !TEXT_RENDERING_TUNABLE {
            assert_eq!(plan, TextRenderingEnv::default());
            return;
        }
        assert_eq!(
            pairs(&apply(&[], &plan)),
            [
                (
                    TEXT_RENDERING_OUTER_ENV,
                    "ZED_FONTS_GAMMA;ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST"
                ),
                (TEXT_GAMMA_ENV, "2.2"),
                (TEXT_CONTRAST_ENV, "3.50"),
            ]
        );
        // Only the changed one.
        let plan = ui(1.8, 0.0).text_rendering_env(env(&[]));
        assert_eq!(
            pairs(&apply(&[], &plan)),
            [
                (TEXT_RENDERING_OUTER_ENV, TEXT_CONTRAST_ENV),
                (TEXT_CONTRAST_ENV, "0.00"),
            ]
        );
    }

    #[test]
    fn a_value_set_outside_arcode_applies_at_the_default() {
        if !TEXT_RENDERING_TUNABLE {
            return;
        }
        let outside = [(TEXT_GAMMA_ENV, "2.0")];
        let plan = UiSettings::default().text_rendering_env(env(&outside));
        assert_eq!(plan.outside, [(TEXT_GAMMA_ENV, "2.0".to_string())]);
        assert_eq!(
            pairs(&apply(&outside, &plan)),
            [
                (TEXT_RENDERING_OUTER_ENV, TEXT_CONTRAST_ENV),
                (TEXT_GAMMA_ENV, "2.0"),
                (TEXT_CONTRAST_ENV, "2.00"),
            ]
        );
        // An outside contrast wins over the default contrast too.
        let outside_contrast = [(TEXT_CONTRAST_ENV, "3")];
        let plan = UiSettings::default().text_rendering_env(env(&outside_contrast));
        assert!(plan.changes.is_empty(), "{plan:?}");
        assert_eq!(plan.outside, [(TEXT_CONTRAST_ENV, "3".to_string())]);
        // A changed setting wins, and the outside value is remembered.
        let plan = ui(1.4, 2.0).text_rendering_env(env(&outside));
        assert!(plan.outside.is_empty());
        assert_eq!(
            pairs(&apply(&outside, &plan)),
            [
                (
                    TEXT_RENDERING_OUTER_ENV,
                    "ZED_FONTS_GAMMA=2.0;ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST"
                ),
                (TEXT_GAMMA_ENV, "1.4"),
                (TEXT_CONTRAST_ENV, "2.00"),
            ]
        );
    }

    /// An Arcode started from the terminal of one with other settings.
    #[test]
    fn settings_of_a_parent_arcode_do_not_leak() {
        if !TEXT_RENDERING_TUNABLE {
            return;
        }
        let parent = apply(&[], &ui(2.2, 5.0).text_rendering_env(env(&[])));
        let parent = pairs(&parent);
        // Back at the defaults: the gamma goes, the contrast is the
        // default one.
        let child = UiSettings::default().text_rendering_env(env(&parent));
        assert!(child.outside.is_empty(), "{child:?}");
        assert_eq!(
            pairs(&apply(&parent, &child)),
            [
                (TEXT_RENDERING_OUTER_ENV, TEXT_CONTRAST_ENV),
                (TEXT_CONTRAST_ENV, "2.00"),
            ]
        );
        // The gamma changed: it is set, and the record says so.
        let child = ui(2.0, 2.0).text_rendering_env(env(&parent));
        assert_eq!(
            pairs(&apply(&parent, &child)),
            [
                (
                    TEXT_RENDERING_OUTER_ENV,
                    "ZED_FONTS_GAMMA;ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST"
                ),
                (TEXT_GAMMA_ENV, "2.0"),
                (TEXT_CONTRAST_ENV, "2.00"),
            ]
        );
        // The same settings again leave the environment as it was.
        let child = ui(2.2, 5.0).text_rendering_env(env(&parent));
        assert_eq!(pairs(&apply(&parent, &child)), parent);
    }

    /// A value set outside the first Arcode survives a second one at the
    /// defaults, even though the first replaced it.
    #[test]
    fn the_outside_value_comes_back_after_a_parent_arcode() {
        if !TEXT_RENDERING_TUNABLE {
            return;
        }
        let outside = [(TEXT_CONTRAST_ENV, "3"), ("PATH", "/bin")];
        let parent = apply(&outside, &ui(1.8, 6.0).text_rendering_env(env(&outside)));
        let parent = pairs(&parent);
        assert!(parent.contains(&(TEXT_CONTRAST_ENV, "6.00")), "{parent:?}");
        let child = UiSettings::default().text_rendering_env(env(&parent));
        assert_eq!(child.outside, [(TEXT_CONTRAST_ENV, "3".to_string())]);
        assert_eq!(
            pairs(&apply(&parent, &child)),
            [("PATH", "/bin"), (TEXT_CONTRAST_ENV, "3")]
        );
    }

    #[test]
    fn odd_records_are_read_safely() {
        assert!(parse_outer("").is_empty());
        assert_eq!(
            parse_outer("A=1;;B;C="),
            [("A", Some("1")), ("B", None), ("C", Some(""))]
        );
        assert_eq!(
            format_outer(&[("A", Some("1;2".into())), ("B", Some("1.5".into()))]),
            "A;B=1.5"
        );
    }
}
