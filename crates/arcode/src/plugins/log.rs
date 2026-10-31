//! "Plugins: Show Log": every plugin's log (`host.log`, guest stdout and
//! stderr, kept by the host in a capped ring buffer) plus what the app
//! noted about plugins (notifications, crashes, unloads), in one read-only
//! tab. Reopening the command refreshes it.

use arcode_plugin_host::bindings::Level;
use arcode_plugin_host::net::{NetworkEntry, NetworkOutcome};
use arcode_plugin_host::plugin_log::{LogLine, LogSource};
use arcode_plugin_host::{PluginId, PluginManager};

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

/// The tab key of one plugin's log.
pub fn plugin_log_key(plugin: &PluginId) -> String {
    format!("{LOG_TAB_KEY}\u{0}{plugin}")
}

/// One plugin's log, then its network log (time, method, host, port and
/// outcome; bodies, paths and headers are never recorded).
pub fn render_plugin(manager: Option<&PluginManager>, plugin: &PluginId) -> String {
    let Some(manager) = manager else {
        return "Plugins are not loaded.\n".to_owned();
    };
    let Some(registered) = manager.registry().get(plugin) else {
        return format!("No plugin {plugin}.\n");
    };
    let mut out = format!(
        "== {} ({}, {:?}) ==\n",
        registered.manifest.plugin.name, plugin, registered.source
    );
    let Ok(instance) = manager.instance(plugin) else {
        out.push_str("(disabled: no log)\n");
        return out;
    };
    let lines = instance.log().lines();
    if lines.is_empty() {
        out.push_str("(no output)\n");
    }
    for line in &lines {
        out.push_str(&format_line(line));
        out.push('\n');
    }
    out.push_str("\n== Network ==\n");
    let entries = instance.network_log().entries();
    if entries.is_empty() {
        out.push_str("(no requests)\n");
    }
    for entry in &entries {
        out.push_str(&format_request(entry));
        out.push('\n');
    }
    out
}

/// One network log entry as shown.
pub fn format_request(entry: &NetworkEntry) -> String {
    let secs = entry
        .at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let outcome = match &entry.outcome {
        NetworkOutcome::Status(code) => code.to_string(),
        NetworkOutcome::Refused(why) => format!("refused: {why}"),
        NetworkOutcome::Failed(why) => format!("failed: {why}"),
    };
    format!(
        "{h:02}:{m:02}:{s:02} UTC  {} {}:{}  {outcome}",
        entry.method, entry.host, entry.port
    )
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
    fn requests_show_time_method_host_and_outcome() {
        let entry = NetworkEntry {
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(3600 + 2 * 60 + 3),
            method: "GET".into(),
            host: "example.com".into(),
            port: 443,
            outcome: NetworkOutcome::Status(200),
        };
        assert_eq!(
            format_request(&entry),
            "01:02:03 UTC  GET example.com:443  200"
        );
        let refused = NetworkEntry {
            outcome: NetworkOutcome::Refused("no consent".into()),
            ..entry
        };
        assert!(format_request(&refused).ends_with("refused: no consent"));
        let plugin = PluginId::parse("acme.tools").unwrap();
        assert!(render_plugin(None, &plugin).contains("not loaded"));
        assert_ne!(plugin_log_key(&plugin), LOG_TAB_KEY);
    }

    #[test]
    fn without_a_manager_only_host_lines() {
        let text = render(None, &["HTTP Client: started".into()]);
        assert!(text.contains("HTTP Client: started"));
        assert!(text.contains("not loaded"));
    }
}
