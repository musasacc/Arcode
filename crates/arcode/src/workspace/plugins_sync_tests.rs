//! Two windows, one set of plugin files: what one window changes reaches
//! the other at once and is never undone by it.

use std::fs;

use arcode_plugin_host::PluginId;
use gpui::{TestAppContext, VisualTestContext};

use super::tests::open;
use super::*;

const MANIFEST: &str = r#"
[plugin]
id = "acme.tools"
name = "Acme Tools"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"

[[contributes.commands]]
id = "run"
title = "Acme: Run"
"#;

fn acme() -> PluginId {
    PluginId::parse("acme.tools").expect("id")
}

#[gpui::test]
fn disabling_in_one_window_reaches_the_other(cx: &mut TestAppContext) {
    let base = tempfile::tempdir().expect("tempdir");
    let bundled = base.path().join("bundled");
    fs::create_dir_all(bundled.join("acme.tools")).expect("mkdir");
    fs::write(bundled.join("acme.tools/plugin.toml"), MANIFEST).expect("write");
    fs::write(bundled.join("acme.tools/plugin.wasm"), b"not wasm").expect("write");
    let mut paths = crate::plugins::paths::plugin_paths(
        None,
        &base.path().join("config"),
        &base.path().join("data"),
        &base.path().join("cache"),
        None,
    );
    paths.bundled = vec![bundled];

    let (a, cx) = open(cx);
    let b = cx.update(|_, cx| {
        cx.open_window(Default::default(), |window, cx| {
            cx.new(|cx| Workspace::new(window, cx))
        })
        .expect("window")
    });
    let mut cx_b = VisualTestContext::from_window(b.into(), cx);
    let b = b.root(&mut cx_b).expect("root");
    for (ws, cx) in [(&a, &mut *cx), (&b, &mut cx_b)] {
        ws.update_in(cx, |ws, window, cx| {
            ws.lsp.update(cx, |lsp, _| lsp.set_enabled(false));
            assert!(
                ws.plugins
                    .client
                    .ensure_with(paths.clone(), Vec::new(), true)
            );
            ws.plugins_loaded(window, cx);
        });
        cx.run_until_parked();
    }
    let commands = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| ws.plugins.contributions.commands.len())
    };
    assert_eq!(commands(&b, &mut cx_b), 1);

    a.update_in(cx, |ws, window, cx| {
        ws.plugins_set_enabled(&acme(), false, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(commands(&b, &mut cx_b), 0, "window B dropped its commands");
    let enabled = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| {
            ws.plugins
                .client
                .manager()
                .is_some_and(|m| m.is_enabled(&acme()))
        })
    };
    assert!(!enabled(&b, &mut cx_b));

    // Window B enables it again; window A follows.
    b.update_in(&mut cx_b, |ws, window, cx| {
        ws.plugins_set_enabled(&acme(), true, window, cx)
    });
    cx_b.run_until_parked();
    assert!(enabled(&a, cx));
    assert_eq!(commands(&a, cx), 1);
}
