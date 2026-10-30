//! Recursive view builders.
//!
//! Plugins describe a panel as an ordinary tree of [`Element`]s. Each element
//! has a key, either explicit (`.key("status")`) or its index among its
//! siblings. Keys give nodes stable ids across renders, so the diff can send
//! one `upsert` when a single label changes (docs/PLUGINS.md §7).

use super::{
    Axis, BadgeProps, ButtonProps, CheckboxProps, CodeProps, Column, InputProps, SelectProps,
    SplitProps, StackProps, TableProps, TabsProps, TextProps, Tone, TreeItem, Widget,
};

/// One node of a view tree, before it is flattened for the host.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub(crate) key: Option<String>,
    pub(crate) widget: Widget,
    pub(crate) children: Vec<Element>,
    pub(crate) grow: bool,
}

impl Element {
    /// An element with no children.
    pub fn new(widget: Widget) -> Self {
        Self {
            key: None,
            widget,
            children: Vec::new(),
            grow: false,
        }
    }

    /// Sets an explicit key, unique among siblings. Keyed nodes keep their
    /// id when siblings are inserted or removed before them.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Lets this element take the remaining space of its parent stack.
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    /// Appends one child.
    pub fn child(mut self, child: Element) -> Self {
        self.children.push(child);
        self
    }

    /// Appends several children.
    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        self.children.extend(children);
        self
    }

    /// The widget of this element.
    pub fn widget(&self) -> &Widget {
        &self.widget
    }

    /// Sets the tone of a `text` or `badge`; ignored for other widgets.
    pub fn tone(mut self, tone: Tone) -> Self {
        match &mut self.widget {
            Widget::Text(p) => p.tone = tone,
            Widget::Badge(p) => p.tone = tone,
            _ => {}
        }
        self
    }

    /// Monospace `text`.
    pub fn mono(self) -> Self {
        self.map_text(|p| p.mono = true)
    }

    /// Bold `text`.
    pub fn bold(self) -> Self {
        self.map_text(|p| p.bold = true)
    }

    /// Wrapping `text` (default: single line, truncated).
    pub fn wrap(self) -> Self {
        self.map_text(|p| p.wrap = true)
    }

    /// Marks a `button` as primary.
    pub fn primary(self) -> Self {
        self.map_button(|p| p.primary = true)
    }

    /// Sets a host icon name on a `button`.
    pub fn icon(self, icon: impl Into<String>) -> Self {
        let icon = icon.into();
        self.map_button(|p| p.icon = Some(icon))
    }

    /// Enables or disables a `button`.
    pub fn enabled(self, enabled: bool) -> Self {
        self.map_button(|p| p.enabled = enabled)
    }

    /// Makes an `input` multiline.
    pub fn multiline(self) -> Self {
        self.map_input(|p| p.multiline = true)
    }

    /// Highlights an `input` with an arcode-syntax language.
    pub fn language(self, language: impl Into<String>) -> Self {
        let language = language.into();
        self.map_input(|p| p.language = Some(language))
    }

    /// An `input` that sends `submit` on Enter (Ctrl+Enter when multiline).
    pub fn submit_on_enter(self) -> Self {
        self.map_input(|p| p.submit_on_enter = true)
    }

    /// An `input` that sends debounced `change` events.
    pub fn notify_changes(self) -> Self {
        self.map_input(|p| p.notify_changes = true)
    }

    fn map_text(mut self, f: impl FnOnce(&mut TextProps)) -> Self {
        if let Widget::Text(p) = &mut self.widget {
            f(p);
        }
        self
    }

    fn map_button(mut self, f: impl FnOnce(&mut ButtonProps)) -> Self {
        if let Widget::Button(p) = &mut self.widget {
            f(p);
        }
        self
    }

    fn map_input(mut self, f: impl FnOnce(&mut InputProps)) -> Self {
        if let Widget::Input(p) = &mut self.widget {
            f(p);
        }
        self
    }
}

fn stack(axis: Axis, children: impl IntoIterator<Item = Element>) -> Element {
    Element::new(Widget::Stack(StackProps {
        axis,
        gap: 2,
        padding: 0,
    }))
    .children(children)
}

/// A vertical stack (gap 8 px).
pub fn vstack(children: impl IntoIterator<Item = Element>) -> Element {
    stack(Axis::Vertical, children)
}

/// A horizontal stack (gap 8 px).
pub fn hstack(children: impl IntoIterator<Item = Element>) -> Element {
    stack(Axis::Horizontal, children)
}

/// A stack with explicit gap and padding, in 4 px units.
pub fn stack_with(
    axis: Axis,
    gap: u8,
    padding: u8,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    Element::new(Widget::Stack(StackProps { axis, gap, padding })).children(children)
}

/// Two panes with a draggable divider; `ratio` is the first pane's share.
pub fn split(axis: Axis, ratio: f32, first: Element, second: Element) -> Element {
    Element::new(Widget::Split(SplitProps { axis, ratio }))
        .child(first)
        .child(second)
}

/// Tabs: one child per title. `active` is the initially shown tab.
pub fn tabs(active: u32, pages: impl IntoIterator<Item = (String, Element)>) -> Element {
    let (titles, children): (Vec<_>, Vec<_>) = pages.into_iter().unzip();
    Element::new(Widget::Tabs(TabsProps { titles, active })).children(children)
}

/// A scroll container around one child.
pub fn scroll(child: Element) -> Element {
    Element::new(Widget::Scroll).child(child)
}

/// Plain, selectable text.
pub fn text(text: impl Into<String>) -> Element {
    Element::new(Widget::Text(TextProps {
        text: text.into(),
        tone: Tone::Normal,
        mono: false,
        bold: false,
        wrap: false,
    }))
}

/// A small coloured label.
pub fn badge(text: impl Into<String>, tone: Tone) -> Element {
    Element::new(Widget::Badge(BadgeProps {
        text: text.into(),
        tone,
    }))
}

/// A button; clicking it sends a `click` event (a user gesture).
pub fn button(label: impl Into<String>) -> Element {
    Element::new(Widget::Button(ButtonProps {
        label: label.into(),
        icon: None,
        primary: false,
        enabled: true,
    }))
}

/// A single-line text input (uncontrolled: the host owns the edited text).
pub fn input(value: impl Into<String>, placeholder: impl Into<String>) -> Element {
    Element::new(Widget::Input(InputProps {
        value: value.into(),
        placeholder: placeholder.into(),
        multiline: false,
        language: None,
        submit_on_enter: false,
        notify_changes: false,
    }))
}

/// A drop-down.
pub fn select(options: impl IntoIterator<Item = String>, selected: Option<u32>) -> Element {
    Element::new(Widget::Select(SelectProps {
        options: options.into_iter().collect(),
        selected,
    }))
}

/// A checkbox.
pub fn checkbox(label: impl Into<String>, checked: bool) -> Element {
    Element::new(Widget::Checkbox(CheckboxProps {
        label: label.into(),
        checked,
    }))
}

/// A two-column, copyable list of pairs.
pub fn key_values(pairs: impl IntoIterator<Item = (String, String)>) -> Element {
    Element::new(Widget::KeyValues(pairs.into_iter().collect()))
}

/// A table column with default width, not numeric, not sortable.
pub fn column(title: impl Into<String>) -> Column {
    Column {
        title: title.into(),
        width: None,
        numeric: false,
        sortable: false,
    }
}

/// A virtualized table whose rows the host pulls through `fetch-rows`.
pub fn table(columns: Vec<Column>, row_count: Option<u64>, generation: u32) -> Element {
    Element::new(Widget::Table(TableProps {
        columns,
        row_count,
        generation,
    }))
}

/// A flat, virtualized tree (items carry their depth).
pub fn tree(items: Vec<TreeItem>) -> Element {
    Element::new(Widget::Tree(items))
}

/// Read-only highlighted code.
pub fn code(text: impl Into<String>, language: Option<String>) -> Element {
    Element::new(Widget::Code(CodeProps {
        text: text.into(),
        language,
        line_numbers: true,
    }))
}

/// A foldable JSON view of raw JSON text.
pub fn json(text: impl Into<String>) -> Element {
    Element::new(Widget::Json(text.into()))
}

/// A spinner with a label.
pub fn spinner(label: impl Into<String>) -> Element {
    Element::new(Widget::Spinner(label.into()))
}

/// A centered title and hint for empty panels.
pub fn empty_state(title: impl Into<String>, hint: impl Into<String>) -> Element {
    Element::new(Widget::EmptyState((title.into(), hint.into())))
}

/// A thin separator line.
pub fn divider() -> Element {
    Element::new(Widget::Divider)
}

/// Flexible empty space.
pub fn spacer() -> Element {
    Element::new(Widget::Spacer).grow()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_only_touch_matching_widgets() {
        assert_eq!(
            text("a").tone(Tone::Danger).bold().widget(),
            &Widget::Text(TextProps {
                text: "a".into(),
                tone: Tone::Danger,
                mono: false,
                bold: true,
                wrap: false
            })
        );
        assert_eq!(divider().bold().primary().multiline(), divider());
        let Widget::Button(b) = button("Run").primary().icon("play").widget().clone() else {
            panic!("button")
        };
        assert!(b.primary && b.enabled && b.icon.as_deref() == Some("play"));
    }

    #[test]
    fn tabs_split_titles_and_children() {
        let t = tabs(
            1,
            [("A".to_owned(), text("a")), ("B".to_owned(), text("b"))],
        );
        assert_eq!(t.children.len(), 2);
        assert!(matches!(t.widget(), Widget::Tabs(p) if p.titles == ["A", "B"] && p.active == 1));
    }
}
