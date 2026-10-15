use har_viewer::{
    har::{Capture, SortColumn},
    session::Session,
};

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
