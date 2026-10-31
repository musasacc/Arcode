//! The signature help popup: the signature with its active parameter
//! highlighted, an `n/m` counter for overloads and the documentation of the
//! parameter and the signature.

use std::ops::Range;

use arcode_ui_kit::metrics::WEIGHT_SEMIBOLD;
use arcode_ui_kit::{popover, readout, settings, theme, type_scale, ui_font_family};
use gpui::{
    Context, HighlightStyle, IntoElement, ParentElement, Render, SharedString, Styled, StyledText,
    UnderlineStyle, Window, div, prelude::*, px,
};

use super::docs::{blocks_for, render_blocks};
use super::{SignatureHelp, SignatureInfo};

/// The popup view; the editor pushes the help to show.
#[derive(Default)]
pub(crate) struct SignatureView {
    help: Option<SignatureHelp>,
    shown: usize,
}

impl SignatureView {
    pub fn set_help(&mut self, help: SignatureHelp, shown: usize, cx: &mut Context<Self>) {
        if self.help.as_ref() != Some(&help) || self.shown != shown {
            self.help = Some(help);
            self.shown = shown;
            cx.notify();
        }
    }
}

/// The active parameter of `sig` (its own, else the help's).
pub(crate) fn active_parameter(help: &SignatureHelp, sig: &SignatureInfo) -> Option<usize> {
    sig.active_parameter.or(help.active_parameter)
}

/// Byte range in `label` of the char range `chars`.
pub(crate) fn byte_range(label: &str, chars: &Range<usize>) -> Range<usize> {
    let byte = |n: usize| label.char_indices().nth(n).map_or(label.len(), |(b, _)| b);
    byte(chars.start)..byte(chars.end)
}

impl Render for SignatureView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        let config = settings(cx);
        let mono = SharedString::from(config.editor.font_family.clone());
        let Some(help) = &self.help else {
            return div().into_any_element();
        };
        let Some(sig) = help.signatures.get(self.shown) else {
            return div().into_any_element();
        };
        let active = active_parameter(help, sig).and_then(|i| sig.parameters.get(i));
        let highlight = HighlightStyle {
            color: Some(colors.accent),
            font_weight: Some(WEIGHT_SEMIBOLD),
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(colors.accent),
                wavy: false,
            }),
            ..Default::default()
        };
        let highlights: Vec<(Range<usize>, HighlightStyle)> = active
            .map(|p| byte_range(&sig.label, &p.label))
            .filter(|r| !r.is_empty())
            .map(|r| (r, highlight))
            .into_iter()
            .collect();
        let label =
            StyledText::new(SharedString::from(sig.label.clone())).with_highlights(highlights);
        let counter = (help.signatures.len() > 1)
            .then(|| readout(format!("{}/{}", self.shown + 1, help.signatures.len())));
        let param_docs = active
            .and_then(|p| p.documentation.as_ref())
            .map(|d| render_blocks(&blocks_for(d), cx));
        let sig_docs = sig
            .documentation
            .as_ref()
            .map(|d| render_blocks(&blocks_for(d), cx));
        let mut body = popover(cx)
            .id("signature-help")
            .occlude()
            .max_w(px(600.))
            .max_h(px(220.))
            .overflow_y_scroll()
            .px(px(10.))
            .py(px(6.))
            .gap(px(4.))
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(colors.text)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .children(counter)
                    .child(div().font_family(mono).child(label)),
            );
        if let Some(docs) = param_docs {
            body = body.child(div().flex().flex_col().gap_1().children(docs));
        }
        if let Some(docs) = sig_docs {
            body = body.child(
                div()
                    .pt_1()
                    .border_t_1()
                    .border_color(colors.border)
                    .text_color(colors.text_muted)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(docs),
            );
        }
        body.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_ranges_are_chars() {
        assert_eq!(byte_range("fn ä(a: i32)", &(5..11)), 6..12);
        assert_eq!(byte_range("ab", &(1..9)), 1..2);
        let sig = SignatureInfo {
            label: "f(a, b)".into(),
            documentation: None,
            parameters: Vec::new(),
            active_parameter: None,
        };
        let help = SignatureHelp {
            signatures: vec![sig.clone()],
            active_signature: 0,
            active_parameter: Some(1),
        };
        assert_eq!(active_parameter(&help, &sig), Some(1));
        let own = SignatureInfo {
            active_parameter: Some(0),
            ..sig
        };
        assert_eq!(active_parameter(&help, &own), Some(0));
    }
}
