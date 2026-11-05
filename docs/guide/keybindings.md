# Key bindings

These are Arcode's default shortcuts. The command palette (Ctrl+Shift+P,
Cmd+Shift+P on macOS) shows the shortcut next to every command, so it is
the quickest way to look one up.

On macOS, Cmd usually takes the place of Ctrl, and Alt is the Option
key. The tables list both columns wherever they differ. A shortcut like
"Ctrl+K Ctrl+T" is a chord: press Ctrl+K, release, then Ctrl+T.

The tables are generated from the bindings in the source
(`crates/arcode/src/actions.rs` and the modules it registers, plus
`crates/arcode-editor/src/actions.rs` and
`crates/arcode-terminal-view/src/lib.rs`).

## Customizing

Arcode does not read a custom key binding file yet. What you can change:

- **Vim mode**: `keymap = "vim"` or `vim_mode = true` under `[editor]` in
  `config.toml`, the first-run dialog, or **Editor: Toggle Vim Mode** for
  the session. Vim mode handles keys in the editor; the shortcuts below
  keep working outside it.
- **Plugins** can bring default keys for their commands (for example the
  HTTP Client binds Ctrl+Enter / Cmd+Enter in `.http` files). A plugin
  key never replaces a built-in one.

Every command is in the palette, with or without a shortcut.

## General

| Action | Linux / Windows | macOS | Command |
|---|---|---|---|
| Command palette | Ctrl+Shift+P, F1 | Cmd+Shift+P, F1 | View: Command Palette |
| Find file | Ctrl+P | Cmd+P | File: Find File |
| Open folder | Ctrl+O | Cmd+O | File: Open Folder |
| New window | Ctrl+Shift+N | Cmd+Shift+N | File: New Window |
| Close window | Ctrl+Shift+W | Cmd+Shift+W | File: Close Window |
| Close tab | Ctrl+W | Cmd+W | |
| Settings | Ctrl+, | Cmd+, | Preferences: Open Settings |
| Select theme | Ctrl+K Ctrl+T | Cmd+K Cmd+T | Preferences: Select Theme |
| Change the file's language | Ctrl+K M | Cmd+K M | Editor: Change Language Mode |
| Toggle sidebar | Ctrl+B | Cmd+B | View: Toggle Sidebar |
| Toggle bottom panel | Ctrl+J | Cmd+J | View: Toggle Panel |
| Split right | Ctrl+\\ | Cmd+\\ | View: Split Right |
| Reveal active file in the file tree | Ctrl+K Ctrl+R | Cmd+K Cmd+R | File: Reveal Active File in Explorer |
| Quit | Ctrl+Q | Cmd+Q | Application: Quit |

## Menu bar

On Linux and Windows the menus (File, Edit, Selection, View, Go, Run,
Terminal, Git, Help) sit in the title bar after the logo; every item
shows its shortcut. On macOS they are in the system menu bar and these
keys do not apply.

| Action | Keys |
|---|---|
| Activate the menu bar | Alt (press and release) |
| Open a menu | Alt + its underlined letter: Alt+F File, Alt+E Edit, Alt+S Selection, Alt+V View, Alt+G Go, Alt+R Run, Alt+T Terminal, Alt+I Git, Alt+H Help |
| Move between items / menus | Up / Down, Left / Right |
| Open a submenu | Right or Enter |
| Run the selected item | Enter or Space |
| Run an item | its underlined letter |
| Close a submenu, then the menu | Esc |

An Alt+letter key that the focused view uses keeps working there: in
the terminal Alt+F and Alt+B still move by word, and in the find bar
Alt+C, Alt+W and Alt+R still toggle its options. A tap on Alt opens the
menus from anywhere, except while a dialog or picker (such as the command
palette) is open. Alt used with the mouse is not a tap: Alt+click (add a
cursor), Alt+Shift+drag (column selection) and moving the window with
Alt+drag leave the menu bar alone.

Items that do not apply right now are greyed: Step Over without a paused
debug session, Go to Definition and Reload from Disk without an open
file, Back and Forward with nowhere to go. Editor items show the editor's
shortcut even while another part of the window has focus.

## Editing

| Action | Linux / Windows | macOS |
|---|---|---|
| Save | Ctrl+S | Cmd+S |
| Undo | Ctrl+Z | Cmd+Z |
| Redo | Ctrl+Shift+Z, Ctrl+Y | Cmd+Shift+Z, Cmd+Y |
| Cut / copy / paste | Ctrl+X / Ctrl+C / Ctrl+V | Cmd+X / Cmd+C / Cmd+V |
| Cut / copy / paste (alternative) | Shift+Delete / Ctrl+Insert / Shift+Insert | |
| Select all | Ctrl+A | Cmd+A |
| Toggle line comment | Ctrl+/ | Cmd+/ |
| Move line up / down | Alt+Up / Alt+Down | Option+Up / Option+Down |
| Duplicate line down / up | Ctrl+Alt+Shift+Down / Up | Cmd+Option+Shift+Down / Up |
| Indent / outdent | Tab / Shift+Tab | Tab / Shift+Tab |
| New line | Enter, Shift+Enter | Enter, Shift+Enter |
| Delete word left | Ctrl+Backspace | Option+Backspace, Cmd+Backspace |
| Delete word right | Ctrl+Delete | Option+Delete |
| Cancel (collapse to one cursor, close popups) | Esc | Esc |
| Context menu | Shift+F10, Menu key | Shift+F10 |

## Cursor movement and selection

| Action | Linux / Windows | macOS |
|---|---|---|
| Move by character / line | Arrow keys | Arrow keys |
| Select by character / line | Shift+Arrow keys | Shift+Arrow keys |
| Move by word | Ctrl+Left / Ctrl+Right | Option+Left / Option+Right |
| Select by word | Ctrl+Shift+Left / Right | Option+Shift+Left / Right |
| Start / end of line | Home / End | Home / End, Cmd+Left / Cmd+Right, Ctrl+A / Ctrl+E |
| Select to start / end of line | Shift+Home / Shift+End | Shift+Home / Shift+End, Cmd+Shift+Left / Right |
| Start / end of file | Ctrl+Home / Ctrl+End | Cmd+Up / Cmd+Down |
| Select to start / end of file | Ctrl+Shift+Home / End | Cmd+Shift+Up / Down |
| Page up / down | PageUp / PageDown | PageUp / PageDown |
| Select a page up / down | Shift+PageUp / PageDown | Shift+PageUp / PageDown |

## Multiple cursors

| Action | Linux / Windows | macOS |
|---|---|---|
| Add next occurrence | Ctrl+D | Cmd+D |
| Select all occurrences | Ctrl+Shift+L | Cmd+Shift+L |
| Add cursor above / below | Ctrl+Alt+Up / Down, Alt+Shift+Up / Down | Cmd+Option+Up / Down, Option+Shift+Up / Down |
| Undo last cursor change | Ctrl+U | Cmd+U |
| Add a cursor | Alt+click | Option+click |
| Box selection | Alt+Shift+drag | Option+Shift+drag |

## Code intelligence

| Action | Linux / Windows | macOS |
|---|---|---|
| Trigger completion | Ctrl+Space | Ctrl+Space |
| Parameter hints | Ctrl+Shift+Space | Ctrl+Shift+Space |
| Show hover | Ctrl+K Ctrl+I | Cmd+K Cmd+I |
| Go to definition | F12 | F12 |
| Go to implementation | Ctrl+F12 | Cmd+F12 |
| Find all references | Shift+F12 | Shift+F12 |
| Code actions | Ctrl+. | Cmd+. |
| Refactor | Ctrl+Shift+R | Cmd+Shift+R |
| Rename symbol | F2 | F2 |
| Format document | Shift+Alt+F | Shift+Option+F |
| Format selection | Ctrl+K Ctrl+F | Cmd+K Cmd+F |
| Go to symbol in file | Ctrl+Shift+O | Cmd+Shift+O |
| Go to symbol in workspace | Ctrl+T | Cmd+T |
| Go back / forward | Alt+Left / Alt+Right | Ctrl+- / Ctrl+Shift+- |

## Search

| Action | Linux / Windows | macOS |
|---|---|---|
| Find in file | Ctrl+F | Cmd+F |
| Replace in file | Ctrl+H | Cmd+Option+F |
| Find next / previous | F3 / Shift+F3, Enter / Shift+Enter in the find bar | same |
| Next field in the find bar | Tab, Shift+Tab | Tab, Shift+Tab |
| Toggle case / whole word / regex | Alt+C / Alt+W / Alt+R | Option+C / Option+W / Option+R |
| Replace next (in the replace field) | Enter | Enter |
| Replace all in file | Ctrl+Alt+Enter | Cmd+Option+Enter |
| Close the find bar | Esc | Esc |
| Find in files | Ctrl+Shift+F | Cmd+Shift+F |
| Replace in files | Ctrl+Shift+H | Cmd+Shift+H |
| Replace all matches (Search view) | Ctrl+Alt+Enter | Cmd+Option+Enter |

## Terminal

| Action | Linux / Windows | macOS |
|---|---|---|
| Focus terminal | Ctrl+\` | Ctrl+\` |
| New terminal | Ctrl+Shift+\` | Cmd+Shift+\` |
| Copy / paste | Ctrl+Shift+C / Ctrl+Shift+V | Cmd+C / Cmd+V |
| Clear | | Cmd+K |
| Scroll a page | Shift+PageUp / Shift+PageDown | Shift+PageUp / Shift+PageDown |

Other keys go to the program running in the terminal.

## Run and debug

| Action | Linux / Windows | macOS | Command |
|---|---|---|---|
| Run project (the current file when nothing was detected) | Ctrl+F5 | Cmd+F5 | Run: Run Project |
| Run / debug the current file | | | Run: Run Current File, Debug: Debug Current File |
| Run and Debug view | Ctrl+Shift+D | Cmd+Shift+D | Debug: Show Run and Debug |
| Start / continue (the current file without a launch configuration) | F5 | F5 | Debug: Start / Continue |
| Pause | F6 | F6 | Debug: Pause |
| Step over / into / out | F10 / F11 / Shift+F11 | F10 / F11 / Shift+F11 | |
| Stop | Shift+F5 | Shift+F5 | Debug: Stop |
| Restart | Ctrl+Shift+F5 | Cmd+Shift+F5 | Debug: Restart |
| Toggle breakpoint | F9 | F9 | Debug: Toggle Breakpoint |

In the debug console, Enter evaluates and Up / Down browse the history.
In the Watch list, Enter adds the expression. In the breakpoint dialog,
Enter saves, Esc cancels and Tab / Shift+Tab move between fields.

On macOS, F-keys may need the Fn key, depending on your keyboard settings.

## Git

| Action | Linux / Windows | macOS | Command |
|---|---|---|---|
| Source Control view | Ctrl+Shift+G | Cmd+Shift+G | Git: Show Source Control |
| Toggle blame column | Ctrl+Alt+B | Cmd+Option+B | Git: Toggle Blame |
| Next / previous conflict | Alt+] / Alt+[ | Option+] / Option+[ | Git: Next Conflict / Previous Conflict |
| Commit (in the commit box) | Ctrl+Enter | Cmd+Enter | Git: Commit |

## File tree

When the file tree has the focus:

| Action | Linux / Windows | macOS |
|---|---|---|
| Move selection | Up / Down, Home / End | Up / Down, Home / End |
| Expand / collapse folder | Right / Left | Right / Left |
| Open | Enter, Space | Enter, Space |
| Rename | F2 | F2 |
| Delete | Delete | Cmd+Backspace |
| Copy path / relative path | Ctrl+Alt+C / Ctrl+Alt+Shift+C | Cmd+Option+C / Cmd+Option+Shift+C |
| Context menu | Shift+F10, Menu key | Shift+F10 |
| Cancel | Esc | Esc |

## Lists, pickers and dialogs

In the file finder, the command palette, the Search view results and
other pickers: Up / Down move, Enter confirms, Esc closes. In the guided
tour, Right or Enter goes to the next step, Left to the previous one, Esc
ends it.

On the welcome screen, typing starts filtering the recent projects (when
there are more than eight); Up / Down select a project, Enter opens it
(or the first match), Esc clears the filter.

## Plugins

| Action | Keys |
|---|---|
| Cancel the newest running plugin task | Esc (while a task is shown in the status bar), or **Plugins: Cancel Task** |
| HTTP Client: send the request at the cursor (in `.http` files) | Ctrl+Enter / Cmd+Enter |

In a plugin's permission dialog, Tab / Shift+Tab and the arrow keys move
between the buttons, Enter or Space presses one, and Esc closes the
dialog without deciding.
