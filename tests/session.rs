use har_viewer::{
    har::{Capture, SortColumn},
    session::{DomainSort, Session},
};
use serde_json::json;

#[test]
fn selection_follows_visible_results_not_old_row_positions() {
    let mut s = Session::new(Capture::sample());
    s.navigate(5);
    let selected = s.selected;
    s.sort_by(SortColumn::Duration);
    s.sort_by(SortColumn::Duration);
    assert!(s.descending);
    assert_eq!(s.selected, selected);
    let selected = s.selected.unwrap();
    s.filter
        .hosts
        .insert(s.capture.entries[selected].host.clone());
    s.refresh();
    assert_eq!(s.selected, Some(selected));
    s.filter.query = "this-url-does-not-exist".into();
    s.refresh();
    assert_eq!(s.selected, None);
    assert_eq!(s.summary.requests, 0);
    assert_eq!(s.navigate(1), None);
    s.filter = Default::default();
    s.refresh();
    assert_eq!(s.selected, s.visible.first().copied());
    assert_eq!(s.navigate(-100), Some(0));
    assert_eq!(s.navigate(100), Some(34));

    s.filter.query = "response:\"Amber reading lamp\"".into();
    s.refresh();
    assert_eq!(s.visible, [5, 15]);
    assert_eq!(s.selected, Some(5));
    assert_eq!(s.summary.requests, 2);
    assert_eq!(s.navigate(1), Some(1));
    assert_eq!(s.selected, Some(15));
    s.sort_by(SortColumn::Url);
    assert_eq!(s.selected, Some(15));
    s.filter.query = "request:SPRING-DEMO response:coupon_expired".into();
    s.refresh();
    assert_eq!(s.visible, [24]);
    assert_eq!(s.selected, Some(24));
    assert_eq!(s.summary.errors, 1);
}

#[test]
fn selection_and_export_respect_combined_filters_and_sort() {
    let mut s = Session::new(Capture::sample());
    s.filter.statuses.insert(5);
    s.refresh();
    assert!(!s.visible.is_empty());
    assert_eq!(s.summary.errors, s.visible.len());
    assert!(
        s.visible
            .iter()
            .all(|&i| s.capture.entries[i].status >= 500)
    );
    let export = Capture::parse(&s.capture.export(&s.visible)).unwrap();
    assert_eq!(export.entries.len(), s.visible.len());
    s.sort_by(SortColumn::Size);
    assert!(!s.descending);
    s.sort_by(SortColumn::Size);
    assert!(s.descending);
    s.sort_by(SortColumn::Status);
    assert!(!s.descending);
}

#[test]
fn histogram_selection_uses_filtered_table_order_and_exact_bucket_boundaries() {
    let entries: Vec<_> = [
        ("a", Some("1970-01-01T00:00:00.021Z"), 503),
        ("b", Some("1970-01-01T00:00:00.019Z"), 200),
        ("z", Some("1970-01-01T00:00:00.020Z"), 0),
        ("end", Some("1970-01-01T00:00:00.630Z"), 200),
        ("origin", Some("1970-01-01T00:00:00.000Z"), 200),
        ("unknown", None, 404),
    ]
    .into_iter()
    .map(|(path, start, status)| {
        json!({
            "startedDateTime": start,
            "time": 0,
            "request": { "url": format!("https://histogram.test/{path}") },
            "response": { "status": status }
        })
    })
    .collect();
    let capture = Capture::parse(&json!({"log": {"entries": entries}}).to_string()).unwrap();
    let mut s = Session::new(capture);
    let counts: Vec<_> = s
        .histogram()
        .into_iter()
        .enumerate()
        .filter(|(_, bin)| bin.requests > 0)
        .map(|(i, bin)| (i, bin.requests, bin.errors))
        .collect();
    assert_eq!(counts, [(0, 2, 1), (1, 1, 0), (2, 2, 2), (63, 1, 0)]);
    assert_eq!(s.select_histogram_bin(2), Some(2));
    assert_eq!(s.selected, Some(2)); // 20 ms precedes 21 ms, not HAR file order.
    assert_eq!(s.select_histogram_bin(1), Some(1));
    assert_eq!(s.selected, Some(1)); // 19 ms belongs to the preceding window.
    assert_eq!(s.select_histogram_bin(63), Some(4));
    assert_eq!(s.selected, Some(3)); // The capture endpoint remains selectable.
    assert_eq!(s.select_histogram_bin(3), None);
    assert_eq!(s.select_histogram_bin(64), None);
    assert_eq!(s.selected, Some(3));

    s.sort_by(SortColumn::Url);
    assert_eq!(s.select_histogram_bin(2), Some(0));
    assert_eq!(s.selected, Some(0));
    s.sort_by(SortColumn::Url);
    assert_eq!(s.select_histogram_bin(2), Some(0));
    assert_eq!(s.selected, Some(2));
    assert_eq!(s.select_histogram_bin(0), Some(1));
    assert_eq!(s.selected, Some(5)); // Missing timestamps are selectable at zero.

    s.filter.query = "/z".into();
    s.refresh();
    assert_eq!(s.histogram()[2].requests, 1); // Filtering must not rescale time.
    assert_eq!(s.histogram()[0].requests, 0);
    assert_eq!(s.select_histogram_bin(2), Some(0));
    assert_eq!(s.selected, Some(2));
    assert_eq!(s.visible, [2]); // Navigation never changes filters or ordering.
    s.filter.query = "no-such-request".into();
    s.refresh();
    assert!(s.histogram().iter().all(|bin| bin.requests == 0));
    assert_eq!(s.select_histogram_bin(2), None);
    assert_eq!(s.selected, None);
}

#[test]
fn histogram_handles_zero_duration_and_unknown_timestamps() {
    for start in [None, Some("1970-01-01T00:00:00Z")] {
        let capture = Capture::parse(
            &json!({"log": {"entries": [{
                "startedDateTime": start,
                "time": 0,
                "request": {"url": "https://histogram.test/"},
                "response": {"status": 200}
            }]}})
            .to_string(),
        )
        .unwrap();
        let mut s = Session::new(capture);
        assert_eq!(s.histogram()[0].requests, 1);
        assert!(s.histogram()[1..].iter().all(|bin| bin.requests == 0));
        assert_eq!(s.select_histogram_bin(0), Some(0));
        assert_eq!(s.selected, Some(0));
    }
}

#[test]
fn overview_domains_sort_filtered_totals_in_both_directions() {
    let entries: Vec<_> = [
        ("c.test", 3, 700),
        ("e.test", 1, 0),
        ("b.test", 10, 100),
        ("d.test", 1, -1),
        ("a.test", 2, 900),
    ]
    .into_iter()
    .flat_map(|(host, count, size)| {
        (0..count).map(move |i| {
            json!({
                "request": {
                    "url": format!("https://{host}/{i}"),
                    "method": if i == 0 { "POST" } else { "GET" }
                },
                "response": { "status": 200, "content": { "size": size } }
            })
        })
    })
    .collect();
    let capture = Capture::parse(&json!({"log": {"entries": entries}}).to_string()).unwrap();
    let mut s = Session::new(capture);
    // Domain tie-breaking must not depend on request-table ordering.
    s.sort_by(SortColumn::Url);
    s.sort_by(SortColumn::Url);
    for (sort, descending, expected) in [
        (
            DomainSort::Host,
            false,
            ["a.test", "b.test", "c.test", "d.test", "e.test"],
        ),
        (
            DomainSort::Host,
            true,
            ["e.test", "d.test", "c.test", "b.test", "a.test"],
        ),
        (
            DomainSort::Requests,
            false,
            ["d.test", "e.test", "a.test", "c.test", "b.test"],
        ),
        (
            DomainSort::Requests,
            true,
            ["b.test", "c.test", "a.test", "d.test", "e.test"],
        ),
        (
            DomainSort::Size,
            false,
            ["d.test", "e.test", "b.test", "a.test", "c.test"],
        ),
        (
            DomainSort::Size,
            true,
            ["c.test", "a.test", "b.test", "d.test", "e.test"],
        ),
    ] {
        let domains = s.domains(sort, descending);
        assert_eq!(
            domains.iter().map(|d| d.host.as_str()).collect::<Vec<_>>(),
            expected,
            "{sort:?}, descending={descending}"
        );
        for domain in domains {
            let totals = match domain.host.as_str() {
                "a.test" => (2, 1800),
                "b.test" => (10, 1000),
                "c.test" => (3, 2100),
                _ => (1, 0),
            };
            assert_eq!((domain.requests, domain.bytes), totals);
        }
    }

    s.filter.methods.insert("POST".into());
    s.refresh();
    let domains = s.domains(DomainSort::Size, true);
    assert_eq!(
        domains
            .iter()
            .map(|d| (d.host.as_str(), d.requests, d.bytes))
            .collect::<Vec<_>>(),
        [
            ("a.test", 1, 900),
            ("c.test", 1, 700),
            ("b.test", 1, 100),
            ("d.test", 1, 0),
            ("e.test", 1, 0)
        ]
    );
    s.filter.query = "no-such-request".into();
    s.refresh();
    assert!(s.domains(DomainSort::Requests, true).is_empty());
}
