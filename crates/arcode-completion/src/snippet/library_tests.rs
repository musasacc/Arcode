use super::*;
use crate::snippet::expand::expand;
use crate::snippet::variables::VariableContext;

const USER: &str = r#"
[[snippet]]
prefix = "fn"
body = "fn ${1:name}(${2}) {\n\t$0\n}"
description = "my fn"
languages = ["rust"]

[[snippet]]
name = "todo"
prefix = "todo"
body = ["// TODO($1):", "// $0"]
"#;

#[test]
fn loads_toml() {
    let defs = load_snippets(USER).expect("parse");
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].prefix, "fn");
    assert_eq!(defs[0].languages, ["rust"]);
    assert_eq!(defs[1].body, "// TODO($1):\n// $0");
    assert!(defs[1].languages.is_empty());
    assert!(defs[1].matches_language("anything"));
    assert!(defs[0].matches_language("Rust"));
    assert!(!defs[0].matches_language("go"));
}

#[test]
fn rejects_bad_toml() {
    assert!(load_snippets("[[snippet]]\nbody = \"x\"").is_err());
    assert!(load_snippets("[[snippet]]\nprefix = \"\"\nbody = \"x\"").is_err());
    assert!(load_snippets("[[snippet]]\nprefix = \"a\"\nbody = \"x\"\nbogus = 1").is_err());
    assert!(load_snippets("not toml [").is_err());
    assert!(load_snippets("").expect("empty ok").is_empty());
}

#[test]
fn builtins_per_language() {
    for (lang, prefixes) in [
        (
            "rust",
            &["fn", "main", "test", "if", "for", "match", "struct", "impl"][..],
        ),
        ("python", &["def", "class", "ifmain"][..]),
        ("typescript", &["function", "arrow", "log"][..]),
        ("javascript", &["function", "arrow", "log"][..]),
        ("go", &["func", "main", "iferr"][..]),
    ] {
        let c = snippets_for(lang);
        for p in prefixes {
            assert!(c.iter().any(|c| c.label == *p), "{lang}: {p}");
        }
        assert!(c.iter().all(|c| c.insert.is_snippet()));
        assert!(c.iter().all(|c| c.kind == CandidateKind::Snippet));
    }
    assert!(snippets_for("plaintext").is_empty());
}

#[test]
fn builtins_expand_cleanly() {
    for def in builtin_snippets() {
        let e = expand(&def.body, &VariableContext::default(), "    ", "    ");
        assert!(!e.text.contains('$'), "{}: {}", def.prefix, e.text);
        assert_eq!(e.tabstops.last().map(|t| t.index), Some(0));
    }
}

#[test]
fn user_snippets_shadow_builtins() {
    let mut lib = SnippetLibrary::builtin();
    let builtin_count = lib.snippets_for("rust").len();
    lib.add(load_snippets(USER).expect("parse"));
    let c = lib.snippets_for("rust");
    assert_eq!(c.len(), builtin_count + 1); // "todo" added, "fn" replaced
    let fn_cand = c.iter().find(|c| c.label == "fn").expect("fn");
    assert_eq!(fn_cand.detail.as_deref(), Some("my fn"));
    let def = lib.get(fn_cand.data).expect("def");
    assert_eq!(def.description.as_deref(), Some("my fn"));
    let todo = c.iter().find(|c| c.label == "todo").expect("todo");
    assert_eq!(todo.detail.as_deref(), Some("todo"));
    assert!(lib.get(u64::MAX).is_none());
    // Go still sees the builtin "func" and the language-less "todo".
    assert!(lib.snippets_for("go").iter().any(|c| c.label == "todo"));
}
