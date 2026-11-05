//! What the menu bar holds: the menus, their items and when each item
//! applies. Every item names a command of the palette registry
//! ([`crate::commands`]), so the menus run exactly what the palette runs
//! and show the same key bindings. Labels mark their mnemonic with `&`
//! (`"&File"` opens with Alt+F).

/// A menu of the bar.
pub struct MenuDef {
    /// Title with its mnemonic marked (`"&File"`).
    pub title: &'static str,
    pub items: &'static [Item],
}

/// An entry of a menu.
pub enum Item {
    /// Runs the palette command `command` (its full `"Category: Name"`
    /// label), shown as `label`.
    Command {
        label: &'static str,
        command: &'static str,
        when: When,
    },
    /// A thin line between groups.
    Separator,
    /// A nested menu.
    Submenu {
        label: &'static str,
        items: &'static [Item],
    },
    /// Recent projects (filled when the menu opens).
    Recent { label: &'static str },
    /// The installed themes (filled when the menu opens).
    Themes { label: &'static str },
}

/// When an item can be picked (besides its command being available where
/// focus is).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    /// A project is open.
    Project,
    /// The project is in a git repository.
    Repo,
    /// A project is open that is not in a git repository.
    NoRepo,
    /// No debug session runs.
    NotDebugging,
    /// A debug session runs or is being prepared.
    Debugging,
    /// The debuggee is paused.
    Paused,
    /// The debuggee runs (not paused).
    Running,
    /// An editor tab is active.
    Editor,
    /// The active editor shows a file on disk.
    File,
    /// Back has somewhere to go.
    Back,
    /// Forward has somewhere to go.
    Forward,
}

/// The state the [`When`] conditions are checked against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub project: bool,
    pub repo: bool,
    pub debugging: bool,
    pub paused: bool,
    pub editor: bool,
    pub file: bool,
    pub back: bool,
    pub forward: bool,
}

impl When {
    /// Whether the condition holds.
    pub fn holds(self, f: Facts) -> bool {
        match self {
            When::Always => true,
            When::Project => f.project,
            When::Repo => f.repo,
            When::NoRepo => f.project && !f.repo,
            When::NotDebugging => !f.debugging,
            When::Debugging => f.debugging,
            When::Paused => f.debugging && f.paused,
            When::Running => f.debugging && !f.paused,
            When::Editor => f.editor,
            When::File => f.editor && f.file,
            When::Back => f.back,
            When::Forward => f.forward,
        }
    }
}

/// Splits a label into its text without the `&` and the mnemonic: the
/// byte offset of the marked character in that text and the character,
/// lowercased. `&&` is a literal `&`.
pub fn mnemonic(label: &str) -> (String, Option<(usize, char)>) {
    let mut text = String::with_capacity(label.len());
    let mut mark = None;
    let mut chars = label.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            match chars.next() {
                Some('&') => text.push('&'),
                Some(next) => {
                    if mark.is_none() {
                        mark = Some((text.len(), next.to_ascii_lowercase()));
                    }
                    text.push(next);
                }
                None => {}
            }
        } else {
            text.push(ch);
        }
    }
    (text, mark)
}

const fn cmd(label: &'static str, command: &'static str) -> Item {
    Item::Command {
        label,
        command,
        when: When::Always,
    }
}

const fn cmd_when(label: &'static str, command: &'static str, when: When) -> Item {
    Item::Command {
        label,
        command,
        when,
    }
}

const SEP: Item = Item::Separator;

const PREFERENCES: &[Item] = &[
    cmd("&Settings", "Preferences: Open Settings"),
    cmd(
        "Settings &File (config.toml)",
        "Preferences: Open Settings File (config.toml)",
    ),
    SEP,
    cmd("Select &Theme…", "Preferences: Select Theme"),
    cmd("&Editor Font…", "Preferences: Select Editor Font"),
    cmd("&UI Font…", "Preferences: Select UI Font"),
    cmd("Te&rminal Font…", "Preferences: Select Terminal Font"),
    SEP,
    cmd("Toggle &Low-End Mode", "Preferences: Toggle Low-End Mode"),
];

const FILE: &[Item] = &[
    cmd("&New Project…", "File: New Project"),
    cmd("New &Window", "File: New Window"),
    SEP,
    cmd("&Open Folder…", "File: Open Folder"),
    cmd(
        "Open Folder in New Win&dow…",
        "File: Open Folder in New Window",
    ),
    Item::Recent {
        label: "Open &Recent",
    },
    SEP,
    cmd("&Save", "File: Save"),
    cmd_when("Reload from Dis&k", "File: Reload from Disk", When::File),
    cmd_when("&Compare with Disk", "File: Compare with Disk", When::File),
    SEP,
    cmd("&Find File…", "File: Find File"),
    cmd_when(
        "Reveal in File &Tree",
        "File: Reveal Active File in Explorer",
        When::Project,
    ),
    SEP,
    Item::Submenu {
        label: "Pr&eferences",
        items: PREFERENCES,
    },
    SEP,
    cmd_when("C&lose Project", "File: Close Project", When::Project),
    cmd("Close W&indow", "File: Close Window"),
    cmd("&Quit", "Application: Quit"),
];

const EDIT: &[Item] = &[
    cmd("&Undo", "Editor: Undo"),
    cmd("&Redo", "Editor: Redo"),
    SEP,
    cmd("Cu&t", "Editor: Cut"),
    cmd("&Copy", "Editor: Copy"),
    cmd("&Paste", "Editor: Paste"),
    SEP,
    cmd("&Find", "Search: Find"),
    cmd("R&eplace", "Search: Replace"),
    cmd("Find &Next", "Search: Find Next"),
    cmd("Find Pre&vious", "Search: Find Previous"),
    SEP,
    cmd_when("Find &in Files", "Search: Find in Files", When::Project),
    cmd_when(
        "Replace in Fi&les",
        "Search: Replace in Files",
        When::Project,
    ),
    SEP,
    cmd("Toggle Line Co&mment", "Editor: Toggle Line Comment"),
    cmd("Format &Document", "Editor: Format Document"),
    cmd("Format &Selection", "Editor: Format Selection"),
    SEP,
    cmd("Rename S&ymbol", "Editor: Rename Symbol"),
    cmd("Code &Actions", "Editor: Code Actions"),
    cmd("Refact&or…", "Editor: Refactor"),
];

const SELECTION: &[Item] = &[
    cmd("Select &All", "Editor: Select All"),
    SEP,
    cmd("Add Cursor Abo&ve", "Editor: Add Cursor Above"),
    cmd("Add Cursor &Below", "Editor: Add Cursor Below"),
    cmd("Select &Next Occurrence", "Editor: Select Next Occurrence"),
    cmd("Select All &Occurrences", "Editor: Select All Occurrences"),
    cmd("&Undo Cursor", "Editor: Cursor Undo"),
    SEP,
    cmd("Move Line U&p", "Editor: Move Line Up"),
    cmd("Move Line &Down", "Editor: Move Line Down"),
    cmd("Dupli&cate Line", "Editor: Duplicate Line"),
    cmd("Duplicate L&ine Up", "Editor: Duplicate Line Up"),
    SEP,
    cmd("Go to &Start of File", "Editor: Go to Start of File"),
    cmd("Go to &End of File", "Editor: Go to End of File"),
];

const VIEW: &[Item] = &[
    cmd("&Command Palette…", "View: Command Palette"),
    SEP,
    cmd("Toggle &Sidebar", "View: Toggle Sidebar"),
    cmd("Toggle &Panel", "View: Toggle Panel"),
    cmd("Split &Right", "View: Split Right"),
    SEP,
    cmd("S&ource Control", "Git: Show Source Control"),
    cmd("Run and &Debug", "Debug: Show Run and Debug"),
    cmd("&TODOs", "TODO: Show TODOs"),
    cmd("Pac&kages", "Packages: Show Packages"),
    cmd("&Language Packs", "Languages: Show Language Packs"),
    SEP,
    cmd("Debu&g Panel", "Debug: Show Debug Panel"),
    cmd("TODO Pa&nel", "TODO: Show TODO Panel"),
    SEP,
    Item::Themes { label: "T&heme" },
    cmd("Toggle &Inlay Hints", "Editor: Toggle Inlay Hints"),
    cmd("Toggle &Blame", "Git: Toggle Blame"),
    cmd("Toggle &Vim Mode", "Editor: Toggle Vim Mode"),
    cmd("Toggle Lo&w-End Mode", "Preferences: Toggle Low-End Mode"),
];

const GO: &[Item] = &[
    cmd_when("&Back", "Navigation: Go Back", When::Back),
    cmd_when("&Forward", "Navigation: Go Forward", When::Forward),
    SEP,
    cmd("Go to F&ile…", "File: Find File"),
    cmd_when(
        "Go to &Symbol in File…",
        "Navigation: Go to Symbol in File",
        When::Editor,
    ),
    cmd(
        "Go to Symbol in &Workspace…",
        "Navigation: Go to Symbol in Workspace",
    ),
    SEP,
    cmd_when(
        "Go to &Definition",
        "Editor: Go to Definition",
        When::Editor,
    ),
    cmd_when(
        "Go to &Type Definition",
        "Editor: Go to Type Definition",
        When::Editor,
    ),
    cmd_when(
        "Go to I&mplementation",
        "Editor: Go to Implementation",
        When::Editor,
    ),
    cmd_when(
        "Find All &References",
        "Editor: Find All References",
        When::Editor,
    ),
];

const RUN: &[Item] = &[
    cmd_when("&Run Project", "Run: Run Project", When::Project),
    cmd("Run Current &File", "Run: Run Current File"),
    cmd_when("&Pick Run Command…", "Run: Pick Run Command", When::Project),
    cmd_when("&Edit Run Command…", "Run: Edit Run Command", When::Project),
    SEP,
    cmd_when(
        "Start &Debugging",
        "Debug: Start / Continue",
        When::NotDebugging,
    ),
    cmd("Debug Current Fi&le", "Debug: Debug Current File"),
    SEP,
    cmd_when("&Continue", "Debug: Continue", When::Paused),
    cmd_when("P&ause", "Debug: Pause", When::Running),
    cmd_when("Step &Over", "Debug: Step Over", When::Paused),
    cmd_when("Step &Into", "Debug: Step Into", When::Paused),
    cmd_when("Step O&ut", "Debug: Step Out", When::Paused),
    cmd_when("Re&start Debugging", "Debug: Restart", When::Debugging),
    cmd_when("S&top Debugging", "Debug: Stop", When::Debugging),
    SEP,
    cmd_when("Toggle &Breakpoint", "Debug: Toggle Breakpoint", When::File),
    cmd_when("Edit Brea&kpoint…", "Debug: Edit Breakpoint", When::File),
    cmd("Remove All Breakpoi&nts", "Debug: Remove All Breakpoints"),
    SEP,
    cmd_when(
        "Open launch.to&ml",
        "Debug: Open launch.toml",
        When::Project,
    ),
    cmd("Show Debu&g Panel", "Debug: Show Debug Panel"),
];

const TERMINAL: &[Item] = &[
    cmd("&New Terminal", "Terminal: New Terminal"),
    cmd("&Focus Terminal", "Terminal: Focus Terminal"),
    SEP,
    cmd_when("&Run Project", "Run: Run Project", When::Project),
    cmd("Run &Current File", "Run: Run Current File"),
    SEP,
    cmd("Toggle &Panel", "View: Toggle Panel"),
];

const GIT: &[Item] = &[
    cmd("&Source Control", "Git: Show Source Control"),
    cmd_when("&Commit", "Git: Commit", When::Repo),
    SEP,
    cmd_when("Switch &Branch…", "Git: Switch Branch", When::Repo),
    cmd_when("Open C&hanges", "Git: Open Changes", When::Repo),
    cmd_when("Toggle Bl&ame", "Git: Toggle Blame", When::Repo),
    SEP,
    cmd_when("&Next Conflict", "Git: Next Conflict", When::Repo),
    cmd_when("&Previous Conflict", "Git: Previous Conflict", When::Repo),
    SEP,
    cmd_when("&Refresh", "Git: Refresh", When::Repo),
    cmd_when(
        "&Initialize Repository",
        "Git: Initialize Repository",
        When::NoRepo,
    ),
];

const HELP: &[Item] = &[
    cmd("&Start Tour", "Help: Start Tour"),
    cmd("&Welcome Setup…", "Help: Show Welcome Setup"),
    SEP,
    cmd("&Command Palette…", "View: Command Palette"),
    cmd("S&ettings", "Preferences: Open Settings"),
];

/// The menus of the bar, left to right.
pub const MENUS: &[MenuDef] = &[
    MenuDef {
        title: "&File",
        items: FILE,
    },
    MenuDef {
        title: "&Edit",
        items: EDIT,
    },
    MenuDef {
        title: "&Selection",
        items: SELECTION,
    },
    MenuDef {
        title: "&View",
        items: VIEW,
    },
    MenuDef {
        title: "&Go",
        items: GO,
    },
    MenuDef {
        title: "&Run",
        items: RUN,
    },
    MenuDef {
        title: "&Terminal",
        items: TERMINAL,
    },
    MenuDef {
        title: "G&it",
        items: GIT,
    },
    MenuDef {
        title: "&Help",
        items: HELP,
    },
];

/// The menu whose title has mnemonic `key` (lowercase), if any.
pub fn menu_for_mnemonic(key: char) -> Option<usize> {
    MENUS
        .iter()
        .position(|m| mnemonic(m.title).1.map(|(_, c)| c) == Some(key))
}

/// Every command label the menus name, with the menu path it is under
/// (for tests and the native menus).
pub fn command_labels() -> Vec<(String, &'static str)> {
    fn walk(path: &str, items: &'static [Item], out: &mut Vec<(String, &'static str)>) {
        for item in items {
            match item {
                Item::Command { label, command, .. } => {
                    out.push((format!("{path} > {}", mnemonic(label).0), command));
                }
                Item::Submenu { label, items } => {
                    walk(&format!("{path} > {}", mnemonic(label).0), items, out)
                }
                Item::Separator | Item::Recent { .. } | Item::Themes { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in MENUS {
        walk(&mnemonic(menu.title).0, menu.items, &mut out);
    }
    out
}
