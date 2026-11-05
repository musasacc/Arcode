# Getting started

This page walks through the parts of Arcode you will use every day.
Shortcuts are given for Linux and Windows; on macOS use Cmd where it says
Ctrl unless noted. The full list is on the [key bindings](keybindings.md)
page.

## The window

```
+-------------------------------------------------------------------+
| File Edit ... | project v  branch  [ Search files ]   Run v  ... |  title bar
+--+---------------+------------------------------------------------+
|  |               |  tabs                                          |
|a |   sidebar     |                                                |
|c |  (files,      |  editor                                        |
|t |   search,     |                                                |
|i |   git, ...)   +------------------------------------------------+
|v |               |  bottom panel: Terminal, Problems, TODO, Debug |
+--+---------------+------------------------------------------------+
| branch  errors warnings      Ln, Col  Spaces  UTF-8  LF  Language  |  status bar
+-------------------------------------------------------------------+
```

- The **menu bar** in the title bar (File, Edit, Selection, View, Go,
  Run, Terminal, Git, Help) lists the commands with their shortcuts.
  Click a menu or press Alt and its underlined letter (Alt+F for File).
  In a narrow window, or with "Menu bar" off in Settings, a single menu
  button holds them. On macOS the menus are in the system menu bar.
- The **activity bar** on the left switches the sidebar between Files,
  Search, Source Control, Run and Debug, Plugins, TODO and Packages.
  Plugins can add their own entries below these. Ctrl+B shows or hides
  the sidebar.
- The **bottom panel** holds the terminals, the Problems list, the TODO
  list and the debug console. Ctrl+J shows or hides it.
- The **status bar** shows the branch, error and warning counts, the
  cursor position, indentation, encoding, line endings, the language and
  the language servers. Most entries can be clicked to change them.

## Open a project

A project is a folder. Open one with:

- **File: Open Folder** (Ctrl+O), or a recent project on the welcome
  screen;
- the command line: `arcode path/to/folder`, or `arcode path/to/file.rs`
  to open the file and the project around it (the nearest folder that is
  a git repository, else the nearest one with a project file such as
  `Cargo.toml` or `package.json`, else the file's own folder);
- **File: New Project** to create a folder from a template;
- **Git: Clone Repository…** to clone a repository (see
  [below](#clone-a-repository)).

Each window shows one project. **File: New Window** (Ctrl+Shift+N) and
**File: Open Folder in New Window** open more. **File: Close Project**
closes the project but keeps the window; Arcode asks first if a file has
unsaved changes.

### The welcome screen

A window without a project shows the welcome screen: Open Folder…, New
Project… and Clone Repository… at the top, your recent projects below
them.

- Click a project to open it; the arrow button opens it in a new window.
- Hover a project and click the **pin** to keep it at the top, in the
  **Pinned** group. Pinned projects are never dropped from the list,
  however many others you open.
- The **x** removes a project from the list. The folder itself is not
  touched.
- Right-click a project for the same actions, **Copy Path** and **Clear
  Recent Projects**, which removes every project that is not pinned
  (also in the File > Open Recent menu and the command palette as
  **File: Clear Recent Projects**).
- With more than eight projects a filter field appears at the top of the
  list. Just start typing: the filter matches project names fuzzily, best
  match first, and the folders they are in by the start of their name
  (`work` finds everything under `~/work`; type a `/` to match the path as
  written). Up and Down select a project, Enter opens it (or the best
  match), Esc clears the filter.
- A project whose folder no longer exists (deleted, moved, or on a drive
  that is not mounted) stays in the list, dimmed and marked "Folder not
  found", until you remove it. If the folder comes back, so does the
  project, with its session: clicking it looks again and opens it.

### Clone a repository

**Clone Repository…** on the welcome screen, in the File menu or as
**Git: Clone Repository…** asks for:

- the **repository URL**: `https://host/owner/repo.git`,
  `git@host:owner/repo.git`, or a local path (which may contain spaces);
- the **location**, the folder the clone goes into (the folder of your
  most recent project, else `~/Projects` if you have one, else your home
  folder; `workspace.projects_folder` in
  [Configuration](configuration.md#workspace) overrides it);
- the **folder name**, taken from the URL until you change it.

Tab and Shift+Tab move between the fields, Enter starts the clone and Esc
closes the dialog.

**Clone** runs your system `git clone`, shows its progress and opens the
clone when it is done. **Cancel** stops it and removes what it had
downloaded. This is the only network access Arcode starts for git, and
only when you click Clone. You need git installed; Arcode tells you if it
is not.

Arcode cannot type a password for git. If the server asks you to sign
in, either set up an SSH key (loaded in your ssh agent) or a git
[credential helper](https://git-scm.com/docs/gitcredentials), or click
**Clone in Terminal**: it runs the same `git clone` in a terminal tab,
where git can ask for your user name, password or token and where ssh
can ask you to accept a new server's host key. Open the folder when it
is done.

### Sessions

When you reopen a project, its tabs, splits, cursors, scroll positions
and panels come back. When you start Arcode without a path, the projects
that were open when you quit come back, one window each. Both can be
turned off in [Configuration](configuration.md#workspace).

## Find a file

Press **Ctrl+P** (or click the search field in the title bar) and type
part of a file name. Matching is fuzzy: `wsmod` finds
`workspace/mod.rs`. Enter opens the file; Esc closes the finder. Files
ignored by `.gitignore` are not listed.

## The file tree

The **Files** view in the sidebar shows the project. Right-click a file
or folder for:

- **New File…** and **New Folder…** (inside the folder you clicked, or
  next to the file);
- **Rename…** and **Delete**;
- **Copy Path** and **Copy Relative Path**;
- **Reveal in File Manager** (Finder on macOS, File Explorer on Windows);
- **Open in Terminal**, which opens a terminal tab in that folder.

The same actions are in the command palette as **Explorer: …** commands
and work on the selected entry; the file tree's own shortcuts are on the
[key bindings](keybindings.md#file-tree) page.

**Delete** moves files to the system trash (the Recycle Bin on Windows),
so you can restore them from there. Arcode asks before deleting unless
you turn that off with `confirm_delete` in
[Configuration](configuration.md#workspace). When a file cannot go to the
trash (for example on a file system without one), Arcode asks whether to
delete it permanently instead.

## The command palette

**Ctrl+Shift+P** or **F1** opens the command palette. Typing `>` in the
file finder switches to it too. Every command in Arcode is listed with
its shortcut, and only the commands that apply where you are show up.
Commands are named `Category: Action` ("Git: Commit", "Editor: Format
Document"), so typing the category narrows the list quickly.

## Edit

The editor has what you would expect, and a few things worth knowing:

- **Multiple cursors.** Ctrl+D adds the next occurrence of the selection,
  Ctrl+Shift+L selects all occurrences, Ctrl+Alt+Up/Down adds a cursor
  above or below, Alt+click adds a cursor, Alt+Shift+drag makes a box
  selection. Ctrl+U undoes the last cursor change.
- **Code intelligence** comes from language servers (see
  [Languages](languages.md)): completion (Ctrl+Space), parameter hints,
  hover (Ctrl+K Ctrl+I), go to definition (F12), find references
  (Shift+F12), rename (F2), code actions (Ctrl+.), formatting
  (Shift+Alt+F). Without a language server you still get completion from
  words in open files, file path completion and an outline for symbol
  search.
- **Snippets.** Built-in snippets appear in the completion list; add your
  own in `snippets.toml` (see [Configuration](configuration.md#snippets)).
- **Vim mode.** Turn it on in the first-run dialog, in Settings, or for the
  session with **Editor: Toggle Vim Mode**.
- **Split** the editor with Ctrl+\\. Close a tab with Ctrl+W.
- **Large files** stay responsive: above 4 MiB or 50,000 lines only the
  visible part is parsed for highlighting.

Files are saved with Ctrl+S. When a file changes on disk while it has
unsaved changes in Arcode, you choose between **File: Reload from Disk**,
**File: Keep My Changes (Ignore Disk Change)** and **File: Compare with Disk**.

## Search

- **Ctrl+F** finds in the current file, **Ctrl+H** (Cmd+Option+F on macOS)
  replaces. Alt+C, Alt+W and Alt+R toggle case-sensitive, whole-word and
  regular-expression matching.
- **Ctrl+Shift+F** searches the whole project, **Ctrl+Shift+H** replaces
  across files. Results stream in as they are found and respect
  `.gitignore`. The **files to include** and **files to exclude** fields
  below the search box take comma-separated globs or folder names, for
  example `src, *.rs` or `tests, *.min.js`.
- **Ctrl+Shift+O** goes to a symbol in the file, **Ctrl+T** to a symbol in
  the project. **Alt+Left** and **Alt+Right** (Ctrl+- and Ctrl+Shift+- on
  macOS) move back and forward through where you have been.
- The **TODO** view collects `TODO`, `FIXME`, `HACK`, `XXX`, `BUG` and
  `NOTE` comments from the whole project and keeps the list up to date.
  **TODO: Show TODOs** opens it in the sidebar, **TODO: Show TODO Panel**
  shows the same list in the bottom panel, and **TODO: Rescan Project**
  reads every file again.

## The terminal

**Ctrl+\`** focuses the terminal (and opens one if needed);
**Ctrl+Shift+\`** opens a new one. Terminals start in the project folder
with your shell (`$SHELL` on Linux and macOS, PowerShell on Windows; see
[`[terminal]`](configuration.md#terminal)). Copy and paste with
Ctrl+Shift+C and Ctrl+Shift+V (Cmd+C and Cmd+V on macOS).

Commands Arcode starts for you (the Run button, run buttons, package
commands) open in named terminal tabs, which are reused when idle. Arcode
asks before closing a window or project while a terminal is still running
a command.

## Run your code

- The **Run** button in the title bar (Ctrl+F5) runs the project's
  detected command: `cargo run`, an npm `start` script, `go run .`, and so
  on. Its arrow lists every detected run, test and build command; the one
  you pick becomes the Run button's command for this project. **Edit Run
  Command…** lets you type any command.
- **Run buttons** in the gutter sit next to `main` functions and tests in
  Rust, Python, JavaScript / TypeScript and Go. Click one to run or debug
  just that item.
- **F5** starts debugging. See [Run and debug](run-and-debug.md).

## Settings

**Ctrl+,** opens the Settings page, with live previews of themes and
fonts. **Ctrl+K Ctrl+T** switches the theme, and **Preferences: Select
Editor Font**, **Preferences: Select UI Font** and **Preferences: Select
Terminal Font** pick a font from a list of the installed ones. Every change is written to
`config.toml`, keeping your comments; **Preferences: Open Settings File
(config.toml)** opens the file itself. See
[Configuration](configuration.md).

## Next steps

- Check your language servers: release packages bundle them, and the
  Languages page (**Languages: Show Language Packs**) shows which are
  installed, bundled or missing, and how to install a missing one.
- Learn the [key bindings](keybindings.md).
- Try the bundled [plugins](plugins.md): open a `.http` file or a SQLite
  database.
