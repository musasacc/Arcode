//! The badge drawn before each completion label: a short glyph tinted with
//! the theme's syntax color for that kind (files and folders use icons).

use arcode_completion::CandidateKind;
use arcode_ui_kit::{IconName, icon, settings, theme};
use gpui::{
    AnyElement, App, FontWeight, Hsla, IntoElement, ParentElement, Rgba, SharedString, Styled, div,
    px,
};

/// Glyph and syntax capture of a kind.
pub(crate) fn kind_glyph(kind: CandidateKind) -> (&'static str, &'static str) {
    use CandidateKind::*;
    match kind {
        Text => ("t", "comment"),
        Method => ("m", "function.method"),
        Function => ("f", "function"),
        Constructor => ("c", "constructor"),
        Field | Property => ("p", "property"),
        Variable => ("v", "variable"),
        Class => ("C", "type"),
        Interface => ("I", "type"),
        Module => ("M", "module"),
        Unit => ("u", "number"),
        Value => ("=", "constant"),
        Enum => ("E", "type"),
        Keyword => ("k", "keyword"),
        Snippet => ("s", ""),
        Color => ("#", "string"),
        File => ("f", "string"),
        Reference => ("&", "variable"),
        Folder => ("d", "string"),
        EnumMember => ("e", "constant"),
        Constant => ("K", "constant"),
        Struct => ("S", "type"),
        Event => ("!", "keyword"),
        Operator => ("+", "operator"),
        TypeParameter | Type => ("T", "type"),
        Word => ("ab", "comment"),
    }
}

/// The badge color of `kind`.
pub(crate) fn kind_color(kind: CandidateKind, cx: &App) -> Hsla {
    let t = theme(cx);
    match kind {
        CandidateKind::Snippet => t.colors.accent,
        CandidateKind::Word | CandidateKind::Text => t.colors.text_muted,
        _ => {
            let (_, capture) = kind_glyph(kind);
            t.syntax(capture).color.unwrap_or(t.colors.text)
        }
    }
}

/// The 16px badge for `kind`.
pub(crate) fn kind_badge(kind: CandidateKind, cx: &App) -> AnyElement {
    let color = kind_color(kind, cx);
    match kind {
        CandidateKind::File => {
            return icon(IconName::File)
                .size(px(14.))
                .color(color)
                .into_any_element();
        }
        CandidateKind::Folder => {
            return icon(IconName::Folder)
                .size(px(14.))
                .color(color)
                .into_any_element();
        }
        _ => {}
    }
    let (glyph, _) = kind_glyph(kind);
    let mono = SharedString::from(settings(cx).editor.font_family.clone());
    let fill = tint(color, theme(cx).colors.panel_raised(), 0.16);
    div()
        .flex_none()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(fill)
        .text_color(color)
        .font_family(mono)
        .font_weight(FontWeight::BOLD)
        .text_size(px(if glyph.len() > 1 { 9. } else { 11. }))
        .line_height(px(16.))
        .child(glyph)
        .into_any_element()
}

/// `color` laid over `base` at `amount`, as one opaque color (the chrome
/// never stacks alpha, docs/DESIGN.md 2).
pub(crate) fn tint(color: Hsla, base: Hsla, amount: f32) -> Hsla {
    let (a, b) = (Rgba::from(color), Rgba::from(base));
    let mix = |x: f32, y: f32| y + (x - y) * amount;
    Hsla::from(Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: 1.,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_glyph() {
        for n in 1..=25 {
            let (glyph, _) = kind_glyph(CandidateKind::from_lsp(n));
            assert!(!glyph.is_empty() && glyph.len() <= 2);
        }
        assert_eq!(kind_glyph(CandidateKind::Function).0, "f");
        assert_eq!(kind_glyph(CandidateKind::Word).0, "ab");
    }

    #[test]
    fn tints_are_opaque_blends() {
        let white = Hsla::from(Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: 1.,
        });
        let black = Hsla::from(Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        });
        let grey = Rgba::from(tint(white, black, 0.25));
        assert!((grey.r - 0.25).abs() < 1e-3 && (grey.b - 0.25).abs() < 1e-3);
        assert_eq!(grey.a, 1.);
        assert_eq!(Rgba::from(tint(white, black, 0.)), Rgba::from(black));
    }
}
