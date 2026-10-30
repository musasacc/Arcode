//! The command palette (Ctrl+Shift+P, or `>` in the file finder): every
//! registered command with its key binding, fuzzy filtered, recently used
//! first. It is built while the previous focus is still in place, so it
//! lists the commands available there and shows the bindings that apply
//! there; the chosen command runs back on that focus.

use std::ops::Range;

use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, HighlightStyle,
    ScrollStrategy, SharedString, StyledText, Subscription, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use super::{Candidate, Ranked, binding_label, commands, rank_commands};
use crate::finder::{CONTEXT, Confirm, Dismiss, SelectNext, SelectPrev};
use crate::text_field::{Backspace, TextChanged, TextField};

const ROW_HEIGHT: f32 = 30.;
const MAX_VISIBLE_ROWS: usize = 12;

/// A command as captured when the palette opened.
pub struct Entry {
    /// `"Category: Name"`.
    pub label: SharedString,
    /// Length of the `"Category: "` prefix in bytes (drawn muted).
    pub category_len: usize,
    /// The binding that runs it where focus was, e.g. `Ctrl+Shift+G`.
    pub binding: Option<SharedString>,
    /// Position in the recently-used list.
    pub recent: Option<usize>,
    action: Box<dyn Action>,
}

/// Events emitted by the palette.
pub enum PaletteEvent {
    /// Run `action` (where focus was before the palette opened); `label`
    /// is remembered as recently used.
    Run {
        /// The command label.
        label: SharedString,
        /// The action to dispatch.
        action: Box<dyn Action>,
    },
    /// The palette should close without running anything.
    Dismissed,
    /// The `>` prefix was deleted: show the file finder with this query.
    SwitchToFiles(String),
}

/// The palette view.
pub struct CommandPalette {
    input: Entity<TextField>,
    entries: Vec<Entry>,
    matches: Vec<Ranked>,
    selected: usize,
    scroll: UniformListScrollHandle,
    return_focus: Option<FocusHandle>,
    _sub: Subscription,
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

/// Snapshot of the commands available at the current focus, with their
/// bindings there. Call it before moving focus into the palette.
pub fn capture_entries(recent: &[String], window: &mut Window, cx: &mut App) -> Vec<Entry> {
    let focus = window.focused(cx);
    let listed: Vec<(String, usize, bool, Box<dyn Action>)> = commands(cx)
        .iter()
        .map(|c| (c.label(), c.name.len(), c.global, c.action()))
        .collect();
    let mut entries = Vec::new();
    for (label, name_len, global, action) in listed {
        if !global && !window.is_action_available(action.as_ref(), cx) {
            continue;
        }
        // The first binding registered is the primary one (Ctrl+C rather
        // than Ctrl+Insert for Copy).
        let binding = match &focus {
            Some(focus) => window.bindings_for_action_in(action.as_ref(), focus),
            None => window.bindings_for_action(action.as_ref()),
        }
        .into_iter()
        .next();
        entries.push(Entry {
            category_len: label.len() - name_len,
            recent: recent.iter().position(|r| *r == label),
            label: label.into(),
            binding: binding.map(|b| binding_label(&b).into()),
            action,
        });
    }
    entries
}

impl CommandPalette {
    /// Creates the palette over `entries` (see [`capture_entries`]) with an
    /// initial `query`; `return_focus` is where commands run.
    pub fn new(
        entries: Vec<Entry>,
        query: &str,
        return_focus: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut field = TextField::new("Type a command", cx);
            field.set_text(query, cx);
            field
        });
        let sub = cx.subscribe(&input, |palette, _, _: &TextChanged, cx| {
            palette.refilter(cx)
        });
        let mut palette = Self {
            input,
            entries,
            matches: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            return_focus,
            _sub: sub,
        };
        palette.refilter(cx);
        palette
    }

    /// Where the chosen command runs.
    pub fn return_focus(&self) -> Option<&FocusHandle> {
        self.return_focus.as_ref()
    }

    /// Labels of the listed commands, in order.
    pub fn visible_labels(&self) -> Vec<SharedString> {
        self.matches
            .iter()
            .map(|m| self.entries[m.index].label.clone())
            .collect()
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        let candidates: Vec<Candidate> = self
            .entries
            .iter()
            .map(|e| Candidate {
                label: &e.label,
                recent: e.recent,
            })
            .collect();
        self.matches = rank_commands(&candidates, &query);
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            self.selected = ix.min(self.matches.len() - 1);
            self.scroll
                .scroll_to_item(self.selected, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.matches.len();
        self.select((self.selected + n.max(1) - 1) % n.max(1), cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select((self.selected + 1) % self.matches.len().max(1), cx);
    }

    /// Runs the command in row `ix`.
    pub fn confirm_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.matches.get(ix).map(|m| &self.entries[m.index]) {
            cx.emit(PaletteEvent::Run {
                label: entry.label.clone(),
                action: entry.action.boxed_clone(),
            });
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.confirm_index(self.selected, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PaletteEvent::Dismissed);
    }

    /// Backspace in an empty query removes the `>`: back to file search.
    fn on_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).text().is_empty() {
            cx.stop_propagation();
            cx.emit(PaletteEvent::SwitchToFiles(String::new()));
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let m = &self.matches[ix];
        let entry = &self.entries[m.index];
        let strong = HighlightStyle {
            color: Some(c.accent),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let muted = HighlightStyle {
            color: Some(c.text_muted),
            ..Default::default()
        };
        let label = StyledText::new(entry.label.clone()).with_highlights(label_highlights(
            entry.category_len,
            &m.highlights,
            muted,
            strong,
        ));
        let selected = ix == self.selected;
        let first_other = ix > 0 && self.matches[ix - 1].recent && !m.recent;
        div()
            .id(ix)
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_color(c.text)
            .when(first_other, |el| el.border_t_1().border_color(c.border))
            .when(selected, |el| el.bg(c.selection))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when(m.recent && ix == 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child("recently used"),
                )
            })
            .when_some(entry.binding.clone(), |el, b| {
                el.child(div().flex_none().child(kbd(b)))
            })
            .on_click(cx.listener(move |palette, _, _, cx| palette.confirm_index(ix, cx)))
            .into_any_element()
    }
}

/// Highlights for a label: the category prefix muted, matched chars
/// strong (a match inside the category stays strong).
fn label_highlights(
    category_len: usize,
    matched: &[Range<usize>],
    muted: HighlightStyle,
    strong: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for r in matched {
        if pos < category_len.min(r.start) {
            out.push((pos..category_len.min(r.start), muted));
        }
        out.push((r.clone(), strong));
        pos = r.end;
    }
    if pos < category_len {
        out.push((pos..category_len, muted));
    }
    out
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let count = self.matches.len();
        let list_height = px(ROW_HEIGHT * count.clamp(1, MAX_VISIBLE_ROWS) as f32 + 8.);
        let input_row = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(40.))
            .px_3()
            .border_b_1()
            .border_color(c.border)
            .child(icon(IconName::Command).size(px(15.)).color(c.text_muted))
            .child(div().text_color(c.accent).child(">"))
            .child(self.input.clone())
            .child(kbd("Esc"));
        let body = if count == 0 {
            div()
                .h(list_height)
                .flex()
                .items_center()
                .px_4()
                .text_sm()
                .text_color(c.text_muted)
                .child("No matching commands")
                .into_any_element()
        } else {
            uniform_list(
                "palette-results",
                count,
                cx.processor(|palette, range: Range<usize>, _, cx| {
                    range
                        .map(|ix| palette.render_row(ix, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .p_1()
            .h(list_height)
            .text_sm()
            .into_any_element()
        };
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .capture_action(cx.listener(Self::on_backspace))
            .w(px(600.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .shadow_md()
            .child(input_row)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_is_muted_around_matches() {
        let m = HighlightStyle::default();
        let s = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let got = label_highlights(5, &[(2..3), (7..9)], m, s);
        let ranges: Vec<_> = got.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(ranges, vec![0..2, 2..3, 3..5, 7..9]);
        let ranges: Vec<_> = label_highlights(4, &[], m, s)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        assert_eq!(ranges, vec![0..4]);
    }
}
