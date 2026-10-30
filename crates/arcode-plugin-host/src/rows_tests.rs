use super::*;

fn page(start: u64, n: u64, at_end: bool) -> RowPage {
    RowPage {
        start,
        rows: (start..start + n)
            .map(|i| vec![Cell::Int(i as i64)])
            .collect(),
        at_end,
    }
}

/// Answers every fetch the cache asks for (from `total` rows) until it is
/// satisfied; returns the starts it fetched.
fn settle(cache: &mut RowCache, total: u64) -> Vec<u64> {
    let mut fetched = Vec::new();
    while let Some(req) = cache.next_fetch() {
        let n = u64::from(req.count).min(total.saturating_sub(req.start));
        let at_end = req.start + n >= total;
        assert!(cache.page_arrived(req, page(req.start, n, at_end)));
        fetched.push(req.start);
        assert!(fetched.len() < 100, "runaway fetching");
    }
    fetched
}

#[test]
fn fetches_visible_pages_then_one_ahead() {
    let mut c = RowCache::new(Some(1_000_000));
    c.set_viewport(0, 40);
    assert_eq!(settle(&mut c, 1_000_000), [0, 200]);
    assert_eq!(c.row(5), Some(&[Cell::Int(5)][..]));
    assert_eq!(c.row(399), Some(&[Cell::Int(399)][..]));
    assert_eq!(c.row(400), None);
    // A viewport spanning two pages fetches both, plus one ahead.
    c.set_viewport(390, 430);
    assert_eq!(settle(&mut c, 1_000_000), [400, 600]);
}

#[test]
fn one_fetch_at_a_time_until_the_viewport_moves_away() {
    let mut c = RowCache::new(Some(1_000_000));
    c.set_viewport(0, 10);
    let first = c.next_fetch().unwrap();
    assert_eq!(first.start, 0);
    assert_eq!(c.next_fetch(), None, "page 0 is still wanted and in flight");
    // Scrolling far: a new fetch is issued (the old one gets coalesced).
    c.set_viewport(500_000, 500_030);
    let far = c.next_fetch().unwrap();
    assert_eq!(far.start, 500_000);
    // The stale answer is still welcome (same generation).
    assert!(c.page_arrived(first, page(0, 200, false)));
    c.page_failed(first);
    assert_eq!(c.next_fetch(), None, "the far page is in flight");
    // Superseded: the app reports failure and the cache asks again.
    c.page_failed(far);
    assert_eq!(c.next_fetch().map(|r| r.start), Some(500_000));
}

#[test]
fn lru_keeps_at_most_six_pages() {
    let mut c = RowCache::new(Some(1_000_000));
    for top in (0..20).map(|p| p * 1000) {
        c.set_viewport(top, top + 30);
        settle(&mut c, 1_000_000);
        assert!(c.cached_pages().len() <= MAX_CACHED_PAGES);
        assert!(c.cached_rows() <= MAX_CACHED_PAGES * PAGE_ROWS as usize);
    }
    // The current viewport's page survived; the first one was evicted.
    assert!(c.row(19_000).is_some());
    assert!(c.row(0).is_none());
    // Reading a row refreshes its page.
    c.set_viewport(19_000, 19_000);
    let order = c.cached_pages();
    assert_eq!(order[0], 95);
}

#[test]
fn unknown_row_count_pages_until_at_end() {
    let mut c = RowCache::new(None);
    c.set_viewport(0, 300);
    assert_eq!(settle(&mut c, 450), [0, 200, 400]);
    assert_eq!(c.row_count(), Some(450));
    c.set_viewport(400, 449);
    assert_eq!(
        settle(&mut c, 450),
        Vec::<u64>::new(),
        "nothing past the end"
    );
}

#[test]
fn small_tables_never_fetch_past_their_count() {
    let mut c = RowCache::new(Some(150));
    c.set_viewport(0, 149);
    assert_eq!(settle(&mut c, 150), [0]);
    let mut empty = RowCache::new(Some(0));
    assert_eq!(empty.next_fetch(), None);
}

#[test]
fn reset_drops_pages_and_ignores_old_answers() {
    let mut c = RowCache::new(Some(1000));
    c.set_viewport(0, 10);
    settle(&mut c, 1000);
    let late = PageRequest {
        start: 400,
        count: PAGE_ROWS,
        epoch: 0,
    };
    let pending = c.next_fetch();
    assert_eq!(pending, None);
    c.reset(Some(10));
    assert!(c.cached_pages().is_empty());
    assert!(!c.page_arrived(late, page(400, 200, false)));
    assert_eq!(settle(&mut c, 10), [0]);
    assert_eq!(c.row_count(), Some(10));
    // Shrinking without a reset drops pages past the end.
    let mut c = RowCache::new(Some(1000));
    c.set_viewport(0, 300);
    settle(&mut c, 1000);
    c.set_row_count(Some(100));
    assert_eq!(c.cached_pages(), [0]);
}

#[test]
fn pages_are_checked_and_cells_cut() {
    let long = "é".repeat(5000);
    let p = RowPage {
        start: 10,
        rows: vec![vec![Cell::Text(long), Cell::Int(1)]; 3],
        at_end: false,
    };
    let checked = check_page(p.clone(), 10, 2).unwrap();
    assert_eq!(checked.rows.len(), 2, "extra rows dropped");
    let Cell::Text(t) = &checked.rows[0][0] else {
        panic!()
    };
    assert!(t.len() <= MAX_CELL_BYTES && t.ends_with('…'));
    assert!(check_page(p, 11, 2).unwrap_err().contains("request at 11"));
    let huge = RowPage {
        start: 0,
        rows: vec![vec![Cell::Text("x".repeat(MAX_CELL_BYTES))]; 1025],
        at_end: true,
    };
    assert!(check_page(huge, 0, 2000).unwrap_err().contains("limit"));
}
