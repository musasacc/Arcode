# Running and debugging

Arcode runs programs in its integrated terminal and debugs them through
the Debug Adapter Protocol (`arcode-dap`). Nothing is installed or
downloaded: Arcode uses the debug adapters you have on `PATH`, and says
what to install when none is found.

## Run buttons

A play icon in the gutter marks every runnable item the tree-sitter
queries of `arcode-syntax` find: Rust `fn main`, `#[test]` functions and
`mod tests`; Python `if __name__ == "__main__":`, pytest functions and
classes; JavaScript / TypeScript `describe` / `it` / `test`; Go
`func main`, `TestX` and `BenchmarkX`. Clicking it opens a small menu:

- **Run** runs the item's command (from `arcode_project::command_for_runnable`,
  e.g. `cargo test --package app --bin app -- tests::adds --exact
  --nocapture` or `python -m pytest tests/test_x.py::test_a`) in a
  terminal tab named after it (`test tests::adds`, `run main.rs`). An idle
  tab of that name is reused.
- **Debug** builds the item if needed and starts a debug session.

Turn the buttons off with `[debug] run_buttons = false`.

The title bar's **Run** button (Ctrl+F5, Cmd+F5 on macOS) runs the
project's default command from `arcode_project::default_run_command`
(`cargo run`, an npm `start` script, `go run .`, ...). Its arrow lists
every detected run, test and build command; picking one makes it the Run
button's command for this project (kept with the session). **Edit Run
Command…** lets you type any command line.

## Debug adapters

| language | adapters tried in order |
|----------|-------------------------|
| Rust, C, C++ | `codelldb`, `lldb-dap` (also `lldb-vscode` and versioned names like `lldb-dap-18`), `gdb` 14 or newer |
| Python | `debugpy` (`python3 -m debugpy.adapter`; checks that the module imports) |
| Go | `dlv dap` |
| JavaScript / TypeScript | js-debug: not supported yet (it needs child sessions) |

Pick another adapter or point to one that is not on `PATH` in
`config.toml`:

```toml
[debug]
adapters = { rust = "gdb" }
adapter_paths = { "lldb-dap" = "/usr/lib/llvm-18/bin/lldb-dap" }
open_panel_on_start = true
```

GDB is given `LC_CTYPE=C.UTF-8` when the environment has no UTF-8 locale
(without one, GDB cannot start a program whose arguments are not ASCII).

When no adapter is found, the toast and the debug console name what to
install, e.g. "No debug adapter for python was found. Install debugpy for
Python (`python3 -m pip install debugpy`). Nothing is downloaded
automatically."

## Launch configurations

The Run and Debug view (bug icon, Ctrl+Shift+D) lists the configurations.
Generated ones come from the project's run commands (each Cargo binary,
`main.py`, a Go main package); a Cargo program is built with
`cargo build --message-format=json` first and the executable Cargo
reports is debugged. Debugging a Rust test builds it with
`cargo test --no-run` and passes the test filter to the test binary.

Your own configurations go in `.arcode/launch.toml` ("Debug: Open
launch.toml" creates it from a template). They are listed first and read
again whenever the file changes, from Arcode or outside it.

```toml
[[configuration]]
name = "Debug server"          # shown in the Run and Debug view
adapter = "lldb-dap"           # codelldb, lldb-dap, gdb, debugpy, delve;
                               # or leave it out and set `language`
language = "rust"              # the first installed adapter for it
request = "launch"             # or "attach" (with `pid`)
program = "${workspaceFolder}/target/debug/server"
args = ["--port", "8080"]
cwd = "${workspaceFolder}"     # default: the project folder
env = { RUST_LOG = "debug" }
stop_on_entry = false
cargo = ["build", "--bin", "server"]  # build first; debug what it built
build = "make debug"           # or any shell command run before launching
module = "pytest"              # Python: run a module instead of `program`
pid = 4242                     # attach: the process id

[configuration.extra]          # merged into the adapter's launch arguments
justMyCode = false

[configuration.adapter_command]  # an adapter Arcode does not know
command = "my-dap"
args = ["--port", "{port}"]
transport = "tcp"              # or "stdio" (default)
```

Variables: `${workspaceFolder}`, `${workspaceFolderBasename}`, `${file}`
(the active file), `${fileDirname}`, `${fileBasename}`,
`${fileBasenameNoExtension}`, `${env:NAME}`. Relative `program` and `cwd`
paths are relative to the project folder. A broken entry is skipped and
the view shows why ("launch.toml: configuration `x`: unknown adapter
`y`").

## Breakpoints

Click left of a line number (the breakpoint lane) or press F9 to toggle a
breakpoint. Right-click the lane (or "Debug: Edit Breakpoint") to set a
condition, a hit count or a log message (a log point prints the message,
with `{expression}` interpolation, instead of stopping). Dots: filled
red for a breakpoint, a bar through it for a condition or hit count, blue
for a log point, a grey ring when it is unchecked in the list or the
adapter could not set it.

Breakpoints follow edits (a moved line takes its breakpoint along; one is
removed only when its line's text is deleted), are kept per project (in
`state.toml`, even when sessions are not restored) and are listed in the Run and Debug view,
where a checkbox enables or disables each, a click opens it and x removes
it.

## Debugging

| key | action |
|-----|--------|
| F5 | start the selected configuration, or continue |
| F6 | pause |
| F10 / F11 / Shift+F11 | step over / into / out |
| Ctrl+Shift+F5 | restart (stops, rebuilds and starts again) |
| Shift+F5 | stop |
| F9 | toggle a breakpoint on the cursor line |
| Ctrl+Shift+D | Run and Debug view (also from an editor, where it replaces Duplicate Line; that stays in the palette) |

Stepping keys pressed while a step is still running are ignored, so a
quick series of F10 presses never steps past where the program stops.
Stop (or closing the window) during a build stops the build and every
process it started.

The same buttons are in the Debug panel's toolbar. The panel shows:

- **Call stack**: threads (when there are several) and the frames of the
  selected thread. The innermost frame with a source file is selected;
  clicking a frame opens its line.
- **Variables**: the scopes of the selected frame (locals and arguments
  open, registers and expensive scopes closed) as a tree that loads
  children when expanded. What is open stays open across steps.
- **Watch**: expressions evaluated in the selected frame at every stop.
- **Debug console**: build output, program output, adapter messages and a
  REPL (Enter evaluates in the selected frame, Up / Down browse the
  history). With GDB the input is first run as a GDB command
  (`info locals`, `print x`); input that is not a GDB command (`a + b`)
  is evaluated as an expression. A carriage return redraws the current
  output line (progress bars), and very long lines are wrapped.

The stopped line gets an arrow in the breakpoint lane and a highlight.
Programs that adapters start in a terminal (`runInTerminal`, e.g. debugpy
and CodeLLDB) get a terminal tab named `Debug: <configuration>`.
