//! The panel, described as a tree of widgets.
//!
//! A plugin never draws anything itself. It builds a tree with the
//! builders from `arcode_plugin::ui` and hands it to `Panel::rerender`.
//! The SDK compares the tree with the one it sent last time and sends only
//! what changed, so rebuilding the whole tree on every event is fine.

use arcode_plugin::ui::{Axis, Element, Tone, button, divider, hstack, input, stack_with, text};

/// Explicit keys give nodes stable ids across renders. Use them for every
/// node you need to find again (to read an input or match a click).
pub const NAME_INPUT: &str = "name";
/// The Greet button.
pub const GREET_BUTTON: &str = "greet";

/// What the panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    /// The last greeting, or `None` before the first click.
    pub greeting: Option<String>,
    /// How often Greet was pressed, ever (kept in kv storage).
    pub clicks: u64,
}

/// The panel's whole tree for `state`.
pub fn render(state: &PanelState) -> Element {
    let greeting = match &state.greeting {
        Some(g) => text(g.clone()).bold().tone(Tone::Accent),
        None => text("Type a name and press Greet.").tone(Tone::Muted),
    };
    let times = if state.clicks == 1 { "time" } else { "times" };
    // `stack_with` takes gap and padding in 4 px units.
    stack_with(
        Axis::Vertical,
        2,
        3,
        [
            text("Hello World").bold(),
            text("This panel is drawn by Arcode from a widget tree the plugin sends.")
                .tone(Tone::Muted)
                .wrap(),
            divider(),
            hstack([
                // Inputs are uncontrolled: Arcode keeps what the user types
                // and sends it along with the next event (`event.values`).
                // The value here is only the initial text.
                input("", "Your name")
                    .submit_on_enter()
                    .key(NAME_INPUT)
                    .grow(),
                button("Greet").primary().key(GREET_BUTTON),
            ]),
            greeting.key("greeting"),
            text(format!("Greeted {} {times} so far.", state.clicks))
                .tone(Tone::Muted)
                .key("clicks"),
        ],
    )
}
