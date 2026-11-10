//! Tests of the terminal panel.

use super::{short_title, truncate};

#[test]
fn shell_titles_are_shortened() {
    assert_eq!(short_title("me@laptop: /srv/www/demo"), "demo");
    assert_eq!(short_title("me@host: ~"), "~");
    assert_eq!(short_title("me@host: ~/code/app/"), "app");
    assert_eq!(short_title("/"), "/");
    assert_eq!(short_title("C:\\Users\\me\\proj"), "proj");
    assert_eq!(short_title("vim main.rs"), "vim main.rs");
    assert_eq!(short_title("Terminal"), "Terminal");
}

#[test]
fn long_titles_are_truncated() {
    assert_eq!(truncate("bash", 24).as_ref(), "bash");
    assert_eq!(truncate("abcdef", 4).as_ref(), "abc…");
}

mod programs {
    use std::cell::RefCell;
    use std::rc::Rc;

    use arcode_project::deps::PackageCommand;
    use gpui::{Entity, TestAppContext, VisualTestContext};

    use super::super::{OnExit, TerminalPanel};
    use crate::test_support::{init, settle};

    type Calls = Rc<RefCell<Vec<Option<i32>>>>;

    /// A program that runs far longer than a test.
    fn long_program() -> PackageCommand {
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("ping", &["-n", "60", "127.0.0.1"])
        } else {
            ("sleep", &["60"])
        };
        PackageCommand {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: std::env::temp_dir(),
            display: format!("{program} {}", args.join(" ")),
        }
    }

    fn start(cx: &mut TestAppContext) -> (Entity<TerminalPanel>, Calls, &mut VisualTestContext) {
        init(cx);
        let (panel, cx) = cx.add_window_view(|_, cx| TerminalPanel::new(cx));
        let calls: Calls = Rc::default();
        let record = calls.clone();
        let on_exit: OnExit = Box::new(move |code, _| record.borrow_mut().push(code));
        panel
            .update_in(cx, |p, window, cx| {
                p.run_program(&long_program(), Some(on_exit), window, cx)
            })
            .expect("the program starts");
        cx.run_until_parked();
        assert!(calls.borrow().is_empty(), "it is still running");
        (panel, calls, cx)
    }

    // Closing the tab of a running install must end the install: before,
    // the callback was dropped unrun and every later Install was refused.
    #[gpui::test]
    fn closing_a_running_program_tab_reports_it_stopped(cx: &mut TestAppContext) {
        let (panel, calls, cx) = start(cx);
        panel.update_in(cx, |p, window, cx| p.close(0, window, cx));
        settle(cx);
        assert_eq!(*calls.borrow(), [None], "reported once, as stopped");
        assert!(panel.read_with(cx, |p, _| p.terminals().is_empty()));
    }

    #[gpui::test]
    fn changing_project_reports_running_programs_stopped(cx: &mut TestAppContext) {
        let (panel, calls, cx) = start(cx);
        let other = tempfile::tempdir().unwrap();
        panel.update(cx, |p, cx| p.set_cwd(Some(other.path().to_path_buf()), cx));
        settle(cx);
        assert_eq!(*calls.borrow(), [None]);
    }
}
