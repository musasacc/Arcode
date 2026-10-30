use super::*;
use crate::config::PORT_PLACEHOLDER;

fn options() -> LaunchOptions {
    LaunchOptions {
        program: PathBuf::from("/work/target/debug/app"),
        args: vec!["--flag".into()],
        cwd: PathBuf::from("/work"),
        env: BTreeMap::from([("RUST_LOG".to_string(), "debug".to_string())]),
        stop_on_entry: false,
    }
}

#[test]
fn rust_prefers_codelldb_over_tcp_with_port_placeholder() {
    let spec = adapter_for_language("rust").unwrap();
    assert_eq!(spec.name, "codelldb");
    assert_eq!(spec.adapter_id, "lldb");
    let AdapterTransport::Tcp {
        command: Some(cmd),
        host,
        port,
        ..
    } = &spec.transport
    else {
        panic!("codelldb should use TCP: {spec:?}");
    };
    assert_eq!(cmd.command, "codelldb");
    assert!(cmd.args.iter().any(|a| a == PORT_PLACEHOLDER));
    assert_eq!((host.as_str(), *port), ("127.0.0.1", 0));
    assert_eq!(adapters_for_language("cpp")[1], KnownAdapter::LldbDap);
}

#[test]
fn codelldb_launch_template() {
    let v = KnownAdapter::CodeLldb.launch_arguments(&options());
    assert_eq!(v["type"], "lldb");
    assert_eq!(v["program"], "/work/target/debug/app");
    assert_eq!(v["args"], json!(["--flag"]));
    assert_eq!(v["cwd"], "/work");
    assert_eq!(v["env"]["RUST_LOG"], "debug");
}

#[test]
fn lldb_dap_env_is_a_list() {
    let v = KnownAdapter::LldbDap.launch_arguments(&options());
    assert_eq!(v["env"], json!(["RUST_LOG=debug"]));
}

#[test]
fn debugpy_uses_the_integrated_terminal() {
    let spec = adapter_for_language("python").unwrap();
    assert_eq!(spec.name, "debugpy");
    let AdapterTransport::Stdio { command, args, .. } = &spec.transport else {
        panic!("debugpy should use stdio");
    };
    assert!(command.starts_with("python"));
    assert_eq!(args, &["-m", "debugpy.adapter"]);
    let v = KnownAdapter::Debugpy.launch_arguments(&options());
    assert_eq!(v["console"], "integratedTerminal");
    assert_eq!(v["program"], "/work/target/debug/app");
}

#[test]
fn delve_listens_on_a_local_port() {
    let spec = adapter_for_language("go").unwrap();
    let AdapterTransport::Tcp {
        command: Some(cmd), ..
    } = &spec.transport
    else {
        panic!("delve should use TCP");
    };
    assert_eq!(cmd.args, ["dap", "--listen=127.0.0.1:{port}"]);
    assert_eq!(KnownAdapter::Delve.launch_arguments(&options())["mode"], "debug");
}

#[test]
fn gdb_uses_its_dap_interpreter() {
    let AdapterTransport::Stdio { command, args, .. } = KnownAdapter::Gdb.spec().transport else {
        panic!("gdb should use stdio");
    };
    assert_eq!(command, "gdb");
    assert!(args.contains(&"--interpreter=dap".to_string()));
}

#[test]
fn javascript_is_a_documented_placeholder() {
    assert_eq!(adapters_for_language("typescript"), [KnownAdapter::JsDebug]);
    assert!(!KnownAdapter::JsDebug.is_supported());
    assert_eq!(adapter_for_language("javascript"), None);
    assert!(matches!(
        find_adapter("javascript"),
        Err(DapError::Unsupported(_))
    ));
    assert_eq!(
        KnownAdapter::JsDebug.launch_arguments(&options())["type"],
        "pwa-node"
    );
    assert!(adapter_for_language("cobol").is_none());
}

#[test]
fn not_found_names_the_preferred_adapter_and_pack() {
    let none = |_| false;
    assert_eq!(
        find_adapter_with("rust", none),
        Err(DapError::NotFound(
            "codelldb not found — install the Rust language pack".into()
        ))
    );
    assert_eq!(
        find_adapter_with("cpp", none).unwrap_err().to_string(),
        "codelldb not found — install the C/C++ language pack"
    );
    assert!(
        find_adapter_with("go", none)
            .unwrap_err()
            .to_string()
            .starts_with("dlv not found — install the Go language pack")
    );
}

#[test]
fn falls_back_to_the_first_installed_adapter() {
    let only_gdb = |a| a == KnownAdapter::Gdb;
    assert_eq!(find_adapter_with("rust", only_gdb), Ok(KnownAdapter::Gdb));
    let lldb_and_gdb = |a| a != KnownAdapter::CodeLldb;
    assert_eq!(
        find_adapter_with("c", lldb_and_gdb),
        Ok(KnownAdapter::LldbDap)
    );
}

#[test]
fn launch_config_and_spec_hint() {
    let config = KnownAdapter::Gdb.launch_config(&options());
    assert_eq!(config.request, DebugRequest::Launch);
    assert!(config.breakpoints.is_empty());
    assert_eq!(
        KnownAdapter::CodeLldb.spec().install_hint.as_deref(),
        Some("install the Rust language pack")
    );
    for a in KnownAdapter::ALL {
        assert!(!a.name().is_empty());
        assert!(a.spec().executable().is_some());
    }
}
