//! Debugger and run actions, their key bindings and palette names.

use gpui::{App, KeyBinding, actions};

use crate::actions::WORKSPACE_CONTEXT;

actions!(
    debug,
    [
        /// Starts the selected launch configuration, or continues a paused
        /// session (F5).
        StartOrContinue,
        /// Continues a paused session.
        Continue,
        /// Pauses the running debuggee (F6).
        Pause,
        /// Steps over the current line (F10).
        StepOver,
        /// Steps into the call on the current line (F11).
        StepInto,
        /// Steps out of the current function (Shift+F11).
        StepOut,
        /// Stops and starts the session again (Ctrl+Shift+F5).
        RestartDebugging,
        /// Stops debugging (Shift+F5).
        StopDebugging,
        /// Toggles a breakpoint on the cursor line (F9).
        ToggleBreakpoint,
        /// Edits the condition, hit count or log message of the breakpoint
        /// on the cursor line.
        EditBreakpoint,
        /// Removes every breakpoint of the project.
        RemoveAllBreakpoints,
        /// Shows the Debug panel.
        ShowDebugPanel,
        /// Shows the Run and Debug view.
        ShowRunAndDebug,
        /// Opens (creating it if needed) `.arcode/launch.toml`.
        OpenLaunchConfig,
        /// Opens the title bar's run command menu.
        PickRunCommand,
    ]
);

/// Palette entries.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    vec![
        ("Debug: Start / Continue", Box::new(StartOrContinue)),
        ("Debug: Continue", Box::new(Continue)),
        ("Debug: Pause", Box::new(Pause)),
        ("Debug: Step Over", Box::new(StepOver)),
        ("Debug: Step Into", Box::new(StepInto)),
        ("Debug: Step Out", Box::new(StepOut)),
        ("Debug: Restart", Box::new(RestartDebugging)),
        ("Debug: Stop", Box::new(StopDebugging)),
        ("Debug: Toggle Breakpoint", Box::new(ToggleBreakpoint)),
        ("Debug: Edit Breakpoint", Box::new(EditBreakpoint)),
        (
            "Debug: Remove All Breakpoints",
            Box::new(RemoveAllBreakpoints),
        ),
        ("Debug: Show Debug Panel", Box::new(ShowDebugPanel)),
        ("Debug: Show Run and Debug", Box::new(ShowRunAndDebug)),
        ("Debug: Open launch.toml", Box::new(OpenLaunchConfig)),
        ("Run: Pick Run Command", Box::new(PickRunCommand)),
    ]
}

/// Key bindings (workspace-wide, so they work from the terminal and the
/// Debug panel too).
pub fn bind_keys(cx: &mut App) {
    let ws = Some(WORKSPACE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("f5", StartOrContinue, ws),
        KeyBinding::new("f6", Pause, ws),
        KeyBinding::new("f10", StepOver, ws),
        KeyBinding::new("f11", StepInto, ws),
        KeyBinding::new("shift-f11", StepOut, ws),
        KeyBinding::new("shift-f5", StopDebugging, ws),
        KeyBinding::new("secondary-shift-f5", RestartDebugging, ws),
        KeyBinding::new("f9", ToggleBreakpoint, ws),
        KeyBinding::new("secondary-shift-d", ShowRunAndDebug, ws),
        // The editor binds the same keys to Duplicate Line in its deeper
        // context; Run and Debug wins (like the other views' Ctrl+Shift
        // keys). Duplicate Line stays in the palette.
        KeyBinding::new(
            "secondary-shift-d",
            ShowRunAndDebug,
            Some(arcode_editor::KEY_CONTEXT),
        ),
    ]);
}
