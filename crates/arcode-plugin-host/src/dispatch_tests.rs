//! Mailbox tests: coalescing, capacity, retire races.

use super::*;

fn lenses(doc: DocId, version: u64) -> Call {
    Call::CodeLenses(DocInfo {
        id: doc,
        path: None,
        language: "x".into(),
        version,
        line_count: 1,
    })
}

fn command(n: u32) -> Call {
    Call::Command {
        command: format!("c{n}"),
        args: "null".into(),
    }
}

fn rows(table: NodeId, start: u64) -> Call {
    Call::FetchRows {
        panel: 1,
        table,
        start,
        count: 200,
    }
}

fn push(m: &Mailbox, call: Call) -> (Pushed, async_channel::Receiver<CallResult>) {
    let (d, rx) = Dispatch::new(call, false);
    (m.push(d), rx)
}

fn next(m: &Mailbox) -> Call {
    match m.recv_timeout(Duration::ZERO) {
        Recv::Call(d) => d.call,
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn lenses_coalesce_per_document_and_rows_per_table() {
    let m = Mailbox::new(64);
    let (first, old_doc1) = push(&m, lenses(1, 1));
    assert_eq!(first, Pushed::NeedsThread);
    let (_, doc2) = push(&m, lenses(2, 1));
    let (again, _) = push(&m, lenses(1, 2));
    assert_eq!(again, Pushed::Queued);
    let (_, old_rows) = push(&m, rows(5, 0));
    push(&m, rows(6, 0));
    push(&m, rows(5, 800));
    assert_eq!(old_doc1.try_recv(), Ok(Err(CallError::Superseded)));
    assert_eq!(old_rows.try_recv(), Ok(Err(CallError::Superseded)));
    assert!(doc2.try_recv().is_err(), "other documents are kept");
    assert_eq!(m.len(), 4);
    assert!(matches!(next(&m), Call::CodeLenses(d) if d.id == 2));
    assert!(matches!(next(&m), Call::CodeLenses(d) if d.id == 1 && d.version == 2));
    assert!(matches!(next(&m), Call::FetchRows { table: 6, .. }));
    assert!(matches!(
        next(&m),
        Call::FetchRows {
            table: 5,
            start: 800,
            ..
        }
    ));
    assert!(matches!(
        m.recv_timeout(Duration::from_millis(1)),
        Recv::Timeout
    ));
}

#[test]
fn full_mailbox_drops_the_oldest_coalescable_call() {
    let m = Mailbox::new(3);
    let (_, oldest) = push(&m, lenses(1, 1));
    push(&m, command(1));
    push(&m, lenses(2, 1));
    let (pushed, _) = push(&m, command(2));
    assert_eq!(pushed, Pushed::Queued);
    assert_eq!(oldest.try_recv(), Ok(Err(CallError::Superseded)));
    assert_eq!(m.len(), 3);
    // Now: command 1, lenses 2, command 2. One more drops lenses 2.
    push(&m, command(3));
    // Only commands left: the next one is refused.
    let (pushed, busy) = push(&m, command(4));
    assert_eq!(pushed, Pushed::Refused);
    assert_eq!(busy.try_recv(), Ok(Err(CallError::Busy)));
    assert!(matches!(next(&m), Call::Command { command, .. } if command == "c1"));
}

#[test]
fn retire_keeps_the_thread_if_calls_raced_in() {
    let m = Mailbox::new(4);
    assert_eq!(push(&m, command(1)).0, Pushed::NeedsThread);
    let _ = next(&m);
    assert_eq!(push(&m, command(2)).0, Pushed::Queued);
    assert!(!m.retire(), "a queued call keeps the thread alive");
    let _ = next(&m);
    assert!(m.retire());
    assert_eq!(push(&m, command(3)).0, Pushed::NeedsThread);
}

#[test]
fn close_answers_everything() {
    let m = Mailbox::new(4);
    let (_, queued) = push(&m, command(1));
    m.close();
    assert_eq!(queued.try_recv(), Ok(Err(CallError::Stopped)));
    let (pushed, late) = push(&m, command(2));
    assert_eq!(pushed, Pushed::Refused);
    assert_eq!(late.try_recv(), Ok(Err(CallError::Stopped)));
    assert!(matches!(
        m.recv_timeout(Duration::from_secs(5)),
        Recv::Closed
    ));
}

#[test]
fn recv_wakes_on_push_from_another_thread() {
    let m = std::sync::Arc::new(Mailbox::new(4));
    let m2 = m.clone();
    let t = std::thread::spawn(move || {
        matches!(m2.recv_timeout(Duration::from_secs(10)), Recv::Call(_))
    });
    std::thread::sleep(Duration::from_millis(20));
    push(&m, command(1));
    assert!(t.join().unwrap());
}
