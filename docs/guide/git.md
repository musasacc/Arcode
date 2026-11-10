# Git

Arcode works with your local git repository through the `git` command,
which must be installed. It never talks to a remote: there is no fetch,
pull or push in Arcode. Use the terminal for those (`git pull`,
`git push`), and Arcode picks up the result.

## In the editor

- **Change bars** in the gutter (and matching marks at the left edge of
  the minimap) show added, modified and deleted lines compared to the
  staged version of the file. They are off by default: turn them on with
  **Settings > Editor > Git > Change bars** or `[git] gutter = true`. Click
  a bar to see the old text, with **Stage**, **Unstage** and **Revert**
  buttons for that single hunk. With the bars off, the file tree and the
  Source Control view still show which files changed.
- **Inline blame** shows the author, date and summary of the cursor line
  at its end, after the cursor rests for half a second
  (`[git] inline_blame`, `inline_blame_delay_ms`). Low-end mode turns it
  off unless you set it explicitly.
- **Blame column**: **Git: Toggle Blame** (Ctrl+Alt+B, Cmd+Option+B on
  macOS) shows who last changed every line.
- **Merge conflicts** are highlighted, with **Accept Current | Accept
  Incoming | Accept Both | Compare** actions above each conflict. Alt+]
  and Alt+[ (**Git: Next Conflict** / **Previous Conflict**) jump between
  them.

## The Source Control view

Open it from the activity bar or with Ctrl+Shift+G (Cmd+Shift+G).

- The **branch** at the top. Click it, or the branch in the title bar or
  status bar, to switch to another local branch or create a new one
  (**Git: Switch Branch**).
- The **commit box**: write a message and press Ctrl+Enter (Cmd+Enter)
  or click **Commit**. Tick **Amend** to amend the last commit.
- The changed files, grouped as **Merge Conflicts**, **Staged**,
  **Changes** and **Untracked**, with stage, unstage and discard actions
  per file and per group. Discarding asks first.
- Click a file to open its **diff**: both versions interleaved in a
  read-only tab, removed lines in red, added lines in green. **Git: Open
  Changes** does the same for the current file.

Without a repository, the view offers **Git: Initialize Repository**.
Repositories in sub-folders of the project are found too.

The view updates when files change, whether in Arcode, in the terminal or
in another program. **Git: Refresh** forces it.

## What Arcode runs

Arcode reads the branch and repository layout directly from the `.git`
folder, and runs `git` (with prompts disabled) for status, diffs, blame,
staging, commits, branches and discarding changes, always off the UI
thread. It never runs a command that contacts a remote.

## Settings

```toml
[git]
gutter = false                # change bars in the gutter and minimap
inline_blame = true           # blame of the cursor line
inline_blame_delay_ms = 500
```
