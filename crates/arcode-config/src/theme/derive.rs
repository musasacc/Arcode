//! `[ui]` keys a theme may leave out, computed from the theme's own colors.
//!
//! Keys added after the theme format was published (`canvas`, `pill`,
//! `border_strong`, `selection_inactive`, `paused_line`) are derived from
//! older keys when a theme table sets those, so a third-party theme that
//! only knows the old keys keeps a consistent look instead of picking up
//! the base theme's values for them.

use super::Color;

/// The derived keys and the keys each one is computed from.
const DERIVED: [(&str, &[&str]); 5] = [
    ("canvas", &["title_bar"]),
    ("pill", &["surface_raised"]),
    ("border_strong", &["text_muted", "border"]),
    ("selection_inactive", &["line_highlight"]),
    ("paused_line", &["warning", "background"]),
];

/// Every key that can be derived, in the order of [`DERIVED`].
#[cfg(test)]
pub(super) fn derived_keys() -> impl Iterator<Item = &'static str> {
    DERIVED.iter().map(|(key, _)| *key)
}

/// The derived value of `key`, or `None` when `key` is not derivable or the
/// table sets none of its source keys (the caller then inherits it).
///
/// `table_sets` says whether the theme table itself sets a key; `resolved`
/// returns a key's final color (the table's or the inherited one).
pub(super) fn derive(
    key: &str,
    table_sets: impl Fn(&str) -> bool,
    resolved: impl Fn(&str) -> Option<Color>,
) -> Option<Color> {
    let (_, sources) = DERIVED.iter().find(|(name, _)| *name == key)?;
    if !sources.iter().any(|source| table_sets(source)) {
        return None;
    }
    // Translucent colors are composited over the background first, so the
    // derived control outline and paused band are always opaque.
    let background = resolved("background").map(|bg| bg.with_alpha(255));
    let opaque = |color: Color| background.map_or(color.with_alpha(255), |bg| color.over(bg));
    match key {
        "canvas" => resolved("title_bar"),
        "pill" => resolved("surface_raised"),
        "border_strong" => Some(Color::mix(
            opaque(resolved("text_muted")?),
            opaque(resolved("border")?),
            0.5,
        )),
        "selection_inactive" => resolved("line_highlight"),
        "paused_line" => Some(Color::mix(opaque(resolved("warning")?), background?, 0.14)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(key: &str) -> Option<Color> {
        Some(match key {
            "title_bar" => Color::rgb(1, 2, 3),
            "surface_raised" => Color::rgb(4, 5, 6),
            "text_muted" => Color::rgb(200, 200, 200),
            "border" => Color::rgb(100, 100, 100),
            "line_highlight" => Color::rgb(7, 8, 9),
            "warning" => Color::rgb(200, 100, 0),
            "background" => Color::BLACK,
            _ => return None,
        })
    }

    #[test]
    fn derives_only_when_a_source_is_set() {
        let all = |_: &str| true;
        assert_eq!(derive("canvas", all, lookup), Some(Color::rgb(1, 2, 3)));
        assert_eq!(derive("pill", all, lookup), Some(Color::rgb(4, 5, 6)));
        assert_eq!(
            derive("border_strong", all, lookup),
            Some(Color::rgb(150, 150, 150))
        );
        assert_eq!(
            derive("selection_inactive", all, lookup),
            Some(Color::rgb(7, 8, 9))
        );
        assert_eq!(
            derive("paused_line", all, lookup),
            Some(Color::rgb(28, 14, 0))
        );
        assert_eq!(derive("canvas", |_| false, lookup), None);
        assert_eq!(derive("text", all, lookup), None);
        // One source of two is enough.
        let only_border = |k: &str| k == "border";
        assert!(derive("border_strong", only_border, lookup).is_some());
        assert!(derive("paused_line", only_border, lookup).is_none());
        assert_eq!(derived_keys().count(), 5);
    }

    #[test]
    fn translucent_sources_become_opaque() {
        let lookup = |key: &str| match key {
            "warning" => Some(Color::rgba(255, 255, 255, 128)),
            "text_muted" => Some(Color::rgba(255, 255, 255, 0)),
            "border" => Some(Color::rgba(255, 255, 255, 255)),
            "background" => Some(Color::BLACK),
            _ => None,
        };
        let band = derive("paused_line", |_| true, lookup).unwrap();
        assert_eq!(band.a, 255);
        let outline = derive("border_strong", |_| true, lookup).unwrap();
        assert_eq!(outline, Color::rgb(128, 128, 128));
    }
}
