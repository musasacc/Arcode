//! Tests of [`super`]: builds, cancelling, adapters and the GDB locale.

use super::*;

#[test]
fn shell_build_output_is_logged_and_failures_reported() {
    let dir = tempfile::tempdir().unwrap();
    let lines = Mutex::new(Vec::new());
    let log = |l: String| lines.lock().unwrap().push(l);
    run_shell("echo built", dir.path(), &Cancel::default(), &log).unwrap();
    assert!(lines.lock().unwrap().iter().any(|l| l.trim() == "built"));
    let err = run_shell("exit 3", dir.path(), &Cancel::default(), &log).unwrap_err();
    assert!(err.contains("failed"), "{err}");
}

#[test]
fn a_cancelled_build_stops() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = Cancel::default();
    cancel.cancel();
    let err = run_shell("echo x", dir.path(), &cancel, &|_| {}).unwrap_err();
    assert_eq!(err, "Cancelled");
}

#[cfg(unix)]
#[test]
fn gdb_gets_a_utf8_locale_only_when_there_is_none() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |k: &str| {
            vars.iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| v.to_string())
        }
    };
    let fix = utf8_locale_fix("gdb", &env(&[])).expect("no locale");
    assert_eq!(fix.0, "LC_CTYPE");
    assert!(fix.1.ends_with("UTF-8"), "{fix:?}");
    assert_eq!(
        utf8_locale_fix("gdb", &env(&[("LANG", "en_US.UTF-8")])),
        None
    );
    assert_eq!(
        utf8_locale_fix("gdb", &env(&[("LC_CTYPE", "C.utf8")])),
        None
    );
    let fix = utf8_locale_fix("gdb", &env(&[("LC_ALL", "C"), ("LANG", "de_DE.UTF-8")]));
    assert_eq!(fix.map(|f| f.0).as_deref(), Some("LC_ALL"));
    assert_eq!(utf8_locale_fix("lldb-dap", &env(&[])), None);

    let mut config = DebugConfig::new(
        KnownAdapter::Gdb.spec(),
        arcode_dap::DebugRequest::Launch,
        json!({}),
    );
    add_adapter_env(&mut config, ("LC_CTYPE".into(), "C.UTF-8".into()));
    let AdapterTransport::Stdio { env, .. } = &config.adapter.transport else {
        panic!("gdb runs over stdio");
    };
    assert!(env.contains(&("LC_CTYPE".into(), "C.UTF-8".into())));
}

#[test]
fn missing_adapters_name_what_to_install() {
    let settings = DebugSettings {
        adapter_paths: KnownAdapter::ALL
            .iter()
            .map(|a| (a.name().to_string(), "/no/such/adapter".to_string()))
            .collect(),
        ..Default::default()
    };
    let err = find("python", &settings).unwrap_err();
    assert!(err.contains("debugpy"), "{err}");
}
