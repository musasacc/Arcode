use super::*;

#[test]
fn refresh_methods_name_their_feature() {
    assert_eq!(
        refresh_feature("workspace/inlayHint/refresh"),
        Some("inlayHint")
    );
    assert_eq!(
        refresh_feature("workspace/semanticTokens/refresh"),
        Some("semanticTokens")
    );
    assert_eq!(refresh_feature("workspace/refresh"), None);
    assert_eq!(refresh_feature("workspace//refresh"), None);
    assert_eq!(refresh_feature("window/inlayHint/refresh"), None);
}

#[test]
fn edit_outcomes_become_results() {
    let ok: Result<Result<EditOutcome, ()>, ()> = Ok(Ok(EditOutcome {
        applied: true,
        failure_reason: None,
    }));
    assert_eq!(edit_outcome(ok), json!({ "applied": true }));
    let dropped: Result<Result<EditOutcome, ()>, ()> = Ok(Err(()));
    assert_eq!(edit_outcome(dropped)["applied"], false);
    let late: Result<Result<EditOutcome, ()>, ()> = Err(());
    assert!(
        edit_outcome(late)["failureReason"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
}

#[test]
fn config_lookup() {
    let opts = json!({ "cargo": { "features": "all" }, "a.b": 1, "x": { "y": { "z": 2 } } });
    assert_eq!(config_value(None, Some("x")), Value::Null);
    assert_eq!(config_value(Some(&opts), None), opts);
    assert_eq!(config_value(Some(&opts), Some("a.b")), json!(1));
    assert_eq!(config_value(Some(&opts), Some("x.y.z")), json!(2));
    assert_eq!(config_value(Some(&opts), Some("rust-analyzer")), opts);
    assert_eq!(
        config_value(Some(&opts), Some("rust-analyzer.cargo")),
        json!({ "features": "all" })
    );
    assert_eq!(config_value(Some(&opts), Some("nope.missing")), Value::Null);
}
