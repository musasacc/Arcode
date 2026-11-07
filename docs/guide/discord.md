# Discord presence

Arcode can show what you are working on in your Discord status, for
example **Playing Arcode — Editing a Rust file**. It is off by default.

## What is sent, and to whom

Arcode talks only to the Discord app running on your computer, over its
local connection (a socket in your runtime or temporary folder on Linux
and macOS, a named pipe on Windows). Arcode itself never contacts the
network for this. The Discord app then shows the status to your Discord
friends and anyone who can see your profile, as it does for games.

What Arcode sends, depending on the switches below:

| Shown | Example | When |
|---|---|---|
| What you are doing | "Editing a Rust file", "Browsing files", "Choosing a project", "Idle" | Always while presence is on. "Idle" after 5 minutes without a key press or mouse movement. |
| The file's name | "Editing main.rs" | Only with **File name** on. The name of the file in the active editor, never its folder or path. |
| The project's name | "In my-app" | Only with **Project name** on. The project folder's name, never its path. |
| Elapsed time | "12:34 elapsed" | With **Elapsed time** on (the default): counted from when the current project was opened. |
| Images | the Arcode logo, the file's language icon | Always while presence is on. |

Arcode never sends file contents, paths, git branch names, or the name of
any file other than the one in the active editor. With several windows
open, the presence follows the window you used last.

The status is updated at most once every 15 seconds (Arcode's own
spacing; Discord allows a little more), and always ends with the latest
state. When Discord is not running,
Arcode tries again every 30 seconds without any message; when Discord
starts or restarts, the status comes back on its own. When you turn
presence off or quit Arcode, the status is cleared.

## Turning it on

- Settings → Workspace → **Discord presence** → **Show in Discord**, or
- **Preferences: Toggle Discord Rich Presence** in the command palette, or
- `enabled = true` in the `[discord]` section of `config.toml`.

Changes apply at once, without a restart.

Discord shows a status for a *Discord application*, whose name is what
appears after "Playing". Arcode does not have a built-in application yet,
so for now you need your own: create an application named "Arcode" in
the Discord developer portal and put its application ID into
**Application ID** (`discord.application_id`). Without an ID, turning
presence on does nothing. If Discord does not accept the ID, the
Application ID row in Settings says so; a corrected ID is tried at once. To show the logo and language icons, upload
images to the application's Rich Presence assets with the keys `arcode`,
`rust`, `python`, `typescript` and so on (the language ids of
[Languages](languages.md); C uses `clang`).

## Settings

```toml
[discord]
enabled = false            # show the presence at all
show_file_name = false     # "Editing main.rs" instead of "Editing a Rust file"
show_project_name = false  # "In my-app"; off: the project is never named
show_elapsed = true        # time since the project was opened
application_id = ""        # the Discord application; empty: built-in (none yet)
```

See also [Configuration](configuration.md#discord) and
[Privacy](privacy.md#discord-presence).
