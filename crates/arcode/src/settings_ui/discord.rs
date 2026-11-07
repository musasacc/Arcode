//! The Discord presence group of the Workspace section (`[discord]`).

use arcode_presence::Status;
use arcode_ui_kit::settings;
use gpui::{Context, Div, Subscription, div, prelude::*};

use super::{SettingsPage, filter, group_heading};
use crate::presence::DiscordStatus;

/// Description of the application ID row while there is none to use.
pub(crate) const NEEDS_ID: &str =
    "Presence needs an application ID from the Discord developer portal.";

/// Description of the application ID row when Discord refused the ID.
pub(crate) const REFUSED_ID: &str =
    "Discord did not accept this application ID; check it in the developer portal.";

/// What the application ID row says for the setting `id` while the
/// presence is in `status`.
pub(crate) fn id_description(id: &str, status: Status) -> &'static str {
    let builtin = arcode_presence::DEFAULT_APPLICATION_ID;
    if status == Status::Rejected {
        REFUSED_ID
    } else if !id.trim().is_empty() {
        "The Discord application whose name is shown after \"Playing\"."
    } else if builtin.is_empty() {
        NEEDS_ID
    } else {
        "Empty: Arcode's own Discord application."
    }
}

/// Redraws the page when the presence's status changes (rarely).
pub(super) fn observe_status(cx: &mut Context<SettingsPage>) -> Subscription {
    cx.observe_global::<DiscordStatus>(|_, cx| cx.notify())
}

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let d = settings(cx).discord.clone();
    let status = crate::presence::status(cx);
    let before = filter::hits();
    let rows = div()
        .child(page.bool_row(
            "Show in Discord",
            "Your Discord status shows what you are working on. Arcode only talks to the \
             Discord app on this computer.",
            "discord.enabled",
            d.enabled,
            cx,
        ))
        .child(page.bool_row(
            "File name",
            "\"Editing main.rs\" instead of \"Editing a Rust file\".",
            "discord.show_file_name",
            d.show_file_name,
            cx,
        ))
        .child(page.bool_row(
            "Project name",
            "Off: the project is never named.",
            "discord.show_project_name",
            d.show_project_name,
            cx,
        ))
        .child(page.bool_row(
            "Elapsed time",
            "How long the current project has been open.",
            "discord.show_elapsed",
            d.show_elapsed,
            cx,
        ))
        .child(page.text_row(
            "Application ID",
            id_description(&d.application_id, status),
            "discord.application_id",
            cx,
        ));
    // While filtering, the heading stays with matching rows: "File name"
    // alone would not say whose.
    let shown = !filter::active() || filter::hits() > before;
    let heading = if shown {
        group_heading("Discord presence", cx).debug_selector(|| "setting-group-discord".into())
    } else {
        div()
    };
    div().child(heading).child(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_row_says_what_the_id_needs() {
        let empty = id_description("", Status::Off);
        assert_eq!(
            empty == NEEDS_ID,
            arcode_presence::DEFAULT_APPLICATION_ID.is_empty()
        );
        let set = id_description("123456789012345678", Status::Connected);
        assert_ne!(set, NEEDS_ID, "an ID is set");
        assert_eq!(
            id_description("123456789012345678", Status::Rejected),
            REFUSED_ID
        );
        for text in [empty, set, REFUSED_ID] {
            assert!(text.ends_with('.') && text.len() < 100, "one sentence");
        }
    }
}
