//! Node manifests with npm, pnpm, Yarn and Bun lockfiles.

use super::*;

#[test]
fn npm_workspace_with_package_lock() {
    let (_dir, root) = project();
    write(&root, "package.json", fx!("package.json"));
    write(&root, "package-lock.json", fx!("package-lock.json"));
    write(
        &root,
        "packages/my-lib/package.json",
        "{\n  \"name\": \"my-lib\",\n  \"dependencies\": {\n    \"lodash\": \"^3.10.0\"\n  }\n}\n",
    );
    write(
        &root,
        "node_modules/react/package.json",
        "{\"name\": \"react\"}",
    );
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["npm package.json", "npm packages/my-lib/package.json"]
    );

    let app = &found[0];
    assert!(app.workspace_root);
    assert_eq!(app.name.as_deref(), Some("web-app"));
    let react = dep(app, "react");
    assert_eq!(
        (
            react.requested.as_str(),
            react.locked.as_deref(),
            react.line
        ),
        ("^18.2.0", Some("18.2.0"), Some(9))
    );
    assert_eq!(
        dep(app, "@tanstack/react-query").locked.as_deref(),
        Some("5.17.9")
    );
    assert_eq!(dep(app, "lodash").locked.as_deref(), Some("4.17.21"));
    assert_eq!(dep(app, "my-lib").source, DepSource::Workspace);
    let alias = dep(app, "string-width-cjs");
    assert_eq!(
        (alias.package.as_str(), alias.locked.as_deref()),
        ("string-width", Some("4.2.3"))
    );
    assert!(matches!(dep(app, "left-pad").source, DepSource::Git(_)));
    let ts = dep(app, "typescript");
    assert_eq!(
        (ts.kind, ts.locked.as_deref(), ts.line),
        (DepKind::Dev, Some("5.3.3"), Some(17))
    );
    assert_eq!(dep(app, "react-dom").kind, DepKind::Peer);
    assert_eq!(dep(app, "fsevents").kind, DepKind::Optional);

    let lib = &found[1];
    assert_eq!(lib.manager, Manager::Npm, "decided by the root's lockfile");
    assert_eq!(
        dep(lib, "lodash").locked.as_deref(),
        Some("3.10.1"),
        "the member's nested copy"
    );
    assert_eq!(
        lib.lockfile.as_deref(),
        Some(root.join("package-lock.json").as_path())
    );
}

#[test]
fn pnpm_importers_and_v6_lockfiles() {
    let (_dir, root) = project();
    write(
        &root,
        "package.json",
        "{\"name\": \"mono\", \"dependencies\": {\"react\": \"^18.2.0\"}, \"devDependencies\": {\"@types/react\": \"^18.2.48\", \"typescript\": \"^5.3.3\"}}",
    );
    write(
        &root,
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n",
    );
    write(&root, "pnpm-lock.yaml", fx!("pnpm_v9.yaml"));
    write(
        &root,
        "packages/ui/package.json",
        "{\"dependencies\": {\"react-dom\": \"^18.2.0\"}}",
    );
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["pnpm package.json", "pnpm packages/ui/package.json"]
    );
    assert!(found[0].workspace_root);
    assert_eq!(
        dep(&found[0], "@types/react").locked.as_deref(),
        Some("18.2.48")
    );
    assert_eq!(
        dep(&found[1], "react-dom").locked.as_deref(),
        Some("18.2.0")
    );

    let (_dir6, root6) = project();
    write(
        &root6,
        "package.json",
        "{\"dependencies\": {\"express\": \"^4.18.2\"}, \"devDependencies\": {\"jest\": \"^29\"}}",
    );
    write(&root6, "pnpm-lock.yaml", fx!("pnpm_v6.yaml"));
    let found = scan_manifests(&root6);
    assert_eq!(dep(&found[0], "express").locked.as_deref(), Some("4.18.2"));
    assert_eq!(dep(&found[0], "jest").locked.as_deref(), Some("29.7.0"));
}

#[test]
fn yarn_classic_berry_and_bun() {
    let (_dir, root) = project();
    write(
        &root,
        "classic/package.json",
        "{\"dependencies\": {\"lodash\": \"^4.17.21\", \"@babel/code-frame\": \"^7.22.13\"}}",
    );
    write(&root, "classic/yarn.lock", fx!("yarn_classic.lock"));
    write(
        &root,
        "berry/package.json",
        "{\"dependencies\": {\"lodash\": \"^4.17.21\"}}",
    );
    write(&root, "berry/yarn.lock", fx!("yarn_berry.lock"));
    write(
        &root,
        "bunny/package.json",
        "{\"dependencies\": {\"hono\": \"^4.0.0\"}}",
    );
    write(&root, "bunny/bun.lock", fx!("bun.lock"));
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        [
            "yarn berry/package.json",
            "bun bunny/package.json",
            "yarn classic/package.json"
        ]
    );
    assert!(found[0].yarn_berry);
    assert_eq!(dep(&found[0], "lodash").locked.as_deref(), Some("4.17.21"));
    assert_eq!(dep(&found[1], "hono").locked.as_deref(), Some("4.0.5"));
    let classic = &found[2];
    assert!(!classic.yarn_berry);
    assert_eq!(
        dep(classic, "lodash").locked.as_deref(),
        Some("4.17.21"),
        "matched by range, not 3.x"
    );
    assert_eq!(
        dep(classic, "@babel/code-frame").locked.as_deref(),
        Some("7.23.5")
    );
}

#[test]
fn package_manager_field_wins() {
    let (_dir, root) = project();
    write(
        &root,
        "package.json",
        "{\"packageManager\": \"pnpm@9.1.0\", \"dependencies\": {\"a\": \"1\"}}",
    );
    write(&root, "package-lock.json", "{\"packages\": {}}");
    let found = scan_manifests(&root);
    assert_eq!(found[0].manager, Manager::Pnpm);
    assert_eq!(found[0].lockfile, None);
}
