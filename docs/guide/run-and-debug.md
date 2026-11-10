# Run and debug

Arcode runs your programs in its integrated terminal and debugs them
through the Debug Adapter Protocol, using debug adapters you have
installed. Nothing is downloaded for you. This page is an overview; every
detail, including all `launch.toml` keys, is in
[docs/DEBUGGING.md](../DEBUGGING.md).

## The Run button

The **Run** button in the title bar (Ctrl+F5, Cmd+F5 on macOS) runs the
project's main command in a terminal tab:

| Project | Detected commands |
|---|---|
| `Cargo.toml` | `cargo run` for each binary, `cargo test`, `cargo build` |
| `package.json` | its scripts (`npm run start`, `npm run test`, ...) |
| `go.mod` | `go run .`, `go test ./...`, `go build` |
| Python | `python main.py` (or `app.py`, `manage.py runserver`, `__main__.py`), `python -m pytest` when the project has tests |
| `justfile` | its recipes |
| `Makefile` | its targets |

Sources are checked in this order, and the Run button uses the first
"run" command found. Housekeeping scripts and targets (`clean`,
`install`, `deploy`, `publish`, `lint`, `format`, ...) are listed but
never chosen on their own.

When the project has no detected command (a folder of loose files, a
single file opened on its own), the Run button runs the current file
instead; the button shows its name.

The arrow next to Run lists every detected run, test and build command.
The one you pick becomes the Run button's command for this project
(remembered with the session). **Run current file** and **Debug current
file** are always listed below them. **Edit Run Command…** lets you type
any command line. **Run: Pick Run Command** does the same from the
palette.

The terminal tab shows whether the command is still running, its exit
code and how long it took. Running again reuses the tab when it is idle.

## Run Current File

**Run: Run Current File** (in the palette and the Run menu) runs the
active editor's file on its own, in a terminal tab named `run file`
(every file runs in that tab; a second one opens only while the last
run is still going), so its output and any compile errors show there. It uses the toolchain
found on `PATH`:

| Language | Command |
|---|---|
| Python | `python3 file.py` (the project's `.venv`, `venv` or `env` interpreter when there is one) |
| Rust | `rustc --edition 2024 -g file.rs -o file && ./file`; `cargo run` for a file of a Cargo package (its `build.rs` too, which only runs inside a cargo build) |
| C / C++ | `cc -g` / `c++ -g` (else `gcc`, `clang` / `g++`, `clang++`), then the program |
| Go | `go run file.go`; the file's package inside a Go module |
| JavaScript | `node file.js` (else Bun or `deno run`) |
| TypeScript | `node --experimental-strip-types file.ts` with Node.js 22.6 or newer, else Bun or `deno run` |
| Java | `java File.java` (JDK 11 or newer; single-file source launch, so no `.class` files are written) |
| Ruby, PHP, Lua | `ruby`, `php`, `lua` |
| Shell | the shell its `#!` line names, else `bash` |

C and C++ headers (`.h`, `.hpp`, ...) do not run on their own: run the
source file that includes them.

A compiled program (Rust, C, C++) is written next to its source and run
from there: `test.rs` becomes `test` (`test.exe` on Windows) in the same
folder, built and run from that folder, and the terminal shows the short
command with paths relative to it:

```sh
rustc --edition 2024 -g test.rs -o test && ./test
```

The program then shows in the file tree like any other new file. Git
lists it as untracked unless you ignore it (add `test` to `.gitignore`,
or `*.exe` on Windows).

Built with debug information, the program may get companions from the
compiler: on Windows the linker writes `test.pdb` (and `test.ilk` when it
links incrementally) beside `test.exe`, and on macOS `rustc` and `clang`
write a `test.dSYM` folder beside `test`. They belong to the program and
follow the same rule as the program below; ignore them in git as well
(`*.pdb`, `*.ilk`, `*.dSYM`). On Linux the debug information stays inside
the program.

Arcode never writes over something it did not build. When a folder has
the program's name, or a file that is not a program, or a program that
Arcode did not build (Arcode keeps a small record of the programs it
built, in its cache folder), or there are debug files (`test.pdb`,
`test.dSYM`) beside a program Arcode did not build, the program is built
in Arcode's cache folder instead, the existing files are left as they
are, and a one-line note says why, for example "Built test.rs in
Arcode's cache: test is a folder". When a build fails, Arcode forgets it
as soon as the run is seen to end, so a program you then build there
yourself is not taken for Arcode's. (On Windows the end of a command in
the terminal is not watched, so for up to ten minutes after a failed
build a new program of that name still counts as the build's.)

To keep compiled programs out of your folders altogether, set
`run.output = "cache"` (Settings > Advanced > Run and debug > Compiled
programs): they then go to a build folder in Arcode's cache folder (one
per source file, `~/.cache/arcode/run` on Linux), and the command runs
from the project folder.

The file is saved first when it has unsaved changes. Interpreted files
(and compiled ones with `run.output = "cache"`) run from the project
folder (the file's own folder when it is outside the project or no
folder is open); a reused tab whose shell was moved with `cd` goes back
there first. A compiled file's build and run steps are joined for the
terminal's shell (`[terminal] shell`): `&&` in POSIX shells and
`cmd.exe`, `if ($?)` in PowerShell, `LAST_EXIT_CODE` in Nushell.

**Debug: Debug Current File** builds and debugs the file the same way:
Python with debugpy, Rust, C and C++ compiled with `-g` into the same
program as Run Current File (next to the source, or in the cache) and
debugged with the native adapter (see below), Go with Delve. Other languages cannot be
debugged yet; Arcode says so.

When nothing can run, a notice says why and what to do, for example
"Install Python 3 to run test.py", or "Nothing to run: open a file to
run it, or set a run command" with a **Set run command** button (for
Debug: a button that creates `.arcode/launch.toml`; the file it creates
holds commented examples only, so F5 keeps debugging the current file
until you add a configuration). Run and Debug never do nothing silently.

## Run buttons in the gutter

A play icon in the gutter marks every runnable item Arcode finds:

- Rust: `fn main`, `#[test]` functions, `mod tests`;
- Python: `if __name__ == "__main__":`, pytest functions and classes;
- JavaScript / TypeScript: `describe`, `it` and `test` blocks;
- Go: `func main`, `TestX` and `BenchmarkX` functions.

Click it and choose **Run** (runs just that item, for example
`cargo test ... -- tests::adds --exact`, in a terminal tab named after
it) or **Debug**. A `fn main` in a loose Rust file (no `Cargo.toml`) and
a Go `func main` outside a module run the file on its own, as Run Current
File does. Run and Debug save the file first when it has unsaved
changes. Turn the buttons off with `[debug] run_buttons = false`.

## Debug adapters

| Language | Adapters, in the order Arcode tries them |
|---|---|
| Rust, C, C++ | CodeLLDB (`codelldb`), `lldb-dap` (also `lldb-vscode` and versioned names like `lldb-dap-18`), GDB 14 or newer |
| Python | `debugpy` (run as `python3 -m debugpy.adapter`) |
| Go | Delve (`dlv dap`) |
| JavaScript / TypeScript | not supported yet |

Install the one you want (the Languages page shows the command for your
system). To prefer another adapter, or to use one that is not on `PATH`:

```toml
[debug]
adapters = { rust = "gdb" }
adapter_paths = { "lldb-dap" = "/usr/lib/llvm-18/bin/lldb-dap" }
```

When no adapter is found, Arcode says which one to install.

## Starting a debug session

- Press **F5**, or click **Debug** on a run button. When the project has
  no launch configuration, F5 debugs the current file.
- The **Run and Debug** view (bug icon in the activity bar, Ctrl+Shift+D)
  lists configurations. Arcode generates them from the project (each
  Cargo binary, `main.py`, a Go main package) and builds the program
  first when needed (`cargo build`, `cargo test --no-run` for tests).
- Your own configurations go in `.arcode/launch.toml` in the project.
  **Debug: Open launch.toml** creates it from a commented template (with
  a configuration for the program of a Cargo project, examples only
  otherwise). Your
  configurations are listed first and reloaded whenever the file
  changes.

```toml
[[configuration]]
name = "Debug server"
language = "rust"                      # or adapter = "lldb-dap"
request = "launch"                     # or "attach" with `pid`
program = "${workspaceFolder}/target/debug/server"
args = ["--port", "8080"]
env = { RUST_LOG = "debug" }
cargo = ["build", "--bin", "server"]   # build first, debug what it built
```

Variables such as `${workspaceFolder}` and `${file}`, Python modules,
attaching to a process, extra adapter arguments and adapters Arcode does
not know are all covered in [docs/DEBUGGING.md](../DEBUGGING.md#launch-configurations).

## While debugging

| Key | Action |
|---|---|
| F5 | Continue |
| F6 | Pause |
| F10 / F11 / Shift+F11 | Step over / into / out |
| Shift+F5 | Stop |
| Ctrl+Shift+F5 (Cmd+Shift+F5) | Restart (stop, rebuild, start) |
| F9 | Toggle a breakpoint on the cursor line |

The **Debug** panel shows the call stack, variables (as a tree that
loads children when you expand them), watch expressions and the debug
console, which is also a REPL that evaluates in the selected frame.
**Debug: Show Debug Panel** brings it back if you closed it.

**Breakpoints**: click left of a line number or press F9. Right-click
for a condition, a hit count or a log message (a log point prints instead
of stopping). Breakpoints move with your edits and are kept per project.
**Debug: Remove All Breakpoints** clears every breakpoint in the project.

## Debugging, step by step: a Rust program

1. Install an adapter, for example `lldb-dap` (from LLVM) or GDB 14+.
2. Open the project and press F5. Arcode builds the binary with Cargo and
   starts it under the debugger.
3. Set a breakpoint with F9 on a line in `main`, then restart with
   Ctrl+Shift+F5.
4. When it stops, look at the variables, step with F10, and type
   expressions in the debug console.
