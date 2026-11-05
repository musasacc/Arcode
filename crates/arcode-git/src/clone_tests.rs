use std::ffi::OsStr;

use super::*;

#[test]
fn folder_names_follow_git() {
    for (url, name) in [
        ("https://github.com/owner/repo.git", "repo"),
        ("https://github.com/owner/repo", "repo"),
        ("https://github.com/owner/repo/", "repo"),
        ("https://github.com/owner/repo.git/", "repo"),
        ("git@github.com:owner/repo.git", "repo"),
        ("git@host:repo.git", "repo"),
        ("ssh://git@host:2222/owner/my-tool.git", "my-tool"),
        ("file:///srv/git/demo.git", "demo"),
        ("/srv/git/demo/", "demo"),
        (r"C:\src\demo.git", "demo"),
        (r"\\?\C:\Users\me\AppData\Local\Temp\demo-repo", "demo-repo"),
        ("/srv/what?/demo#1", "demo#1"),
        ("  https://example.com/a/b.git?ref=x  ", "b"),
        ("", "repository"),
        ("git@host:", "repository"),
        ("https://", "repository"),
    ] {
        assert_eq!(folder_name_from_url(url), name, "{url}");
    }
}

#[test]
fn clone_command_never_prompts() {
    let target = Path::new("/tmp/x/repo");
    let cmd = clone_command("https://example.com/r.git", target, true);
    assert_eq!(cmd.get_program(), OsStr::new("git"));
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(
        args,
        [
            "clone",
            "--progress",
            "--",
            "https://example.com/r.git",
            "/tmp/x/repo"
        ]
        .map(OsStr::new)
    );
    let envs: Vec<_> = cmd.get_envs().collect();
    let set = |k: &str, v: &str| envs.contains(&(OsStr::new(k), Some(OsStr::new(v))));
    assert!(set("GIT_TERMINAL_PROMPT", "0"));
    assert!(set("GIT_SSH_COMMAND", BATCH_SSH_COMMAND));
    assert!(
        envs.contains(&(OsStr::new("GIT_DIR"), None)),
        "GIT_DIR removed"
    );
    // The network blocks of the local commands do not apply here.
    assert!(!envs.iter().any(|(k, _)| *k == "GIT_ALLOW_PROTOCOL"));

    // A user's own ssh command is left alone.
    let cmd = clone_command("git@host:r.git", target, false);
    assert!(!cmd.get_envs().any(|(k, _)| k == "GIT_SSH_COMMAND"));
}

#[test]
fn progress_lines_parse() {
    assert_eq!(
        parse_progress("Receiving objects:  45% (450/1000), 1.20 MiB | 2.00 MiB/s"),
        Some(CloneProgress {
            phase: "Receiving objects".into(),
            percent: Some(45),
        })
    );
    assert_eq!(
        parse_progress("remote: Counting objects: 100% (5/5), done."),
        Some(CloneProgress {
            phase: "Counting objects".into(),
            percent: Some(100),
        })
    );
    assert_eq!(
        parse_progress("remote: Enumerating objects: 5, done."),
        Some(CloneProgress {
            phase: "Enumerating objects".into(),
            percent: None,
        })
    );
    assert_eq!(parse_progress("Cloning into 'repo'..."), None);
    assert_eq!(parse_progress("fatal: repository not found"), None);
    assert_eq!(parse_progress("warning: You appear to have cloned"), None);
    assert_eq!(parse_progress(""), None);
}

#[test]
fn failures_are_classified() {
    use CloneFailure::*;
    let cases = [
        (
            "Cloning into 'r'...\nfatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
            AuthRequired,
        ),
        (
            "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            AuthRequired,
        ),
        (
            "Host key verification failed.\nfatal: Could not read from remote repository.",
            UnknownHostKey,
        ),
        (
            "remote: Repository not found.\nfatal: repository 'https://github.com/a/b.git/' not found",
            NotFound,
        ),
        ("fatal: repository '/srv/nope' does not exist", NotFound),
        (
            "fatal: unable to access 'https://nohost.invalid/r.git/': Could not resolve host: nohost.invalid",
            Network,
        ),
        (
            "fatal: unable to access 'https://h/r.git/': CONNECT tunnel failed, response 502",
            Network,
        ),
        (
            "fatal: unable to access 'https://h/r.git/': Could not resolve proxy: proxy.lan",
            Network,
        ),
        (
            "fatal: unable to access 'https://h/r.git/': Received HTTP code 407 from proxy after CONNECT",
            Network,
        ),
        (
            "fatal: unable to access 'https://h/r.git/': Failed to receive SOCKS5 connect request ack.",
            Network,
        ),
        (
            "fatal: destination path 'r' already exists and is not an empty directory.",
            TargetNotEmpty,
        ),
        (
            "Cloning into 'r'...\nerror: something odd\nfatal: early EOF\n",
            Other("early EOF".into()),
        ),
        ("", Other("git clone failed".into())),
    ];
    for (stderr, want) in cases {
        assert_eq!(clone_failure(stderr), want, "{stderr}");
    }
}
