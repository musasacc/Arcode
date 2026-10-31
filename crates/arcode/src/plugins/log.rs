//! "Plugins: Show Log": every plugin's log (`host.log`, guest stdout and
//! stderr, kept by the host in a capped ring buffer) plus what the app
//! noted about plugins (notifications, crashes, unloads), in one read-only
//! tab. Reopening the command refreshes it.

use arcode_plugin_host::PluginManager;
use arcode_plugin_host::bindings::Level;
use arcode_plugin_host::plugin_log::{LogLine, LogSource};

/// The tab key of the plugin log.
pub const LOG_TAB_KEY: &str = "plugins-log";

fn level_label(level: Level) -> &'static str {
    match level {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    }
}

/// One plugin log line as shown.
pub fn format_line(line: &LogLine) -> String {
    let source = match line.source {
        LogSource::Host => "",
        LogSource::Stdout => " stdout",
        LogSource::Stderr => " stderr",
    };
    format!("[{}{source}] {}", level_label(line.level), line.text)
}

/// The whole log: the host's notes, then each plugin's lines.
pub fn render(manager: Option<&PluginManager>, host: &[String]) -> String {
    let mut out = String::from("== Arcode ==\n");
    if host.is_empty() {
        out.push_str("(nothing yet)\n");
    }
    for line in host {
        out.push_str(line);
        out.push('\n');
    }
    let Some(manager) = manager else {
        out.push_str("\nPlugins are not loaded.\n");
        return out;
    };
    let mut plugins: Vec<_> = manager.registry().plugins().collect();
    plugins.sort_by(|a, b| a.id().cmp(b.id()));
    for plugin in plugins {
        out.push_str(&format!(
            "\n== {} ({}, {:?}) ==\n",
            plugin.manifest.plugin.name,
            plugin.id(),
            plugin.source
        ));
        let Ok(instance) = manager.instance(plugin.id()) else {
            out.push_str("(disabled)\n");
            continue;
        };
        let lines = instance.log().lines();
        if lines.is_empty() {
            out.push_str("(no output)\n");
        }
        let dropped = instance.log().dropped_lines();
        if dropped > 0 {
            out.push_str(&format!("({dropped} older lines dropped)\n"));
        }
        for line in &lines {
            out.push_str(&format_line(line));
            out.push('\n');
        }
    }
    for problem in manager.registry().problems() {
        out.push_str(&format!(
            "\nSkipped {}: {}\n",
            problem.dir.display(),
            problem.error
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_show_level_and_stream() {
        let line = LogLine {
            source: LogSource::Stderr,
            level: Level::Warn,
            text: "oops".into(),
        };
        assert_eq!(format_line(&line), "[warn stderr] oops");
        let host = LogLine {
            source: LogSource::Host,
            level: Level::Info,
            text: "sent".into(),
        };
        assert_eq!(format_line(&host), "[info] sent");
    }

    #[test]
    fn without_a_manager_only_host_lines() {
        let text = render(None, &["HTTP Client: started".into()]);
        assert!(text.contains("HTTP Client: started"));
        assert!(text.contains("not loaded"));
    }
}
