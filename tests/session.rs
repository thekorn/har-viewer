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
