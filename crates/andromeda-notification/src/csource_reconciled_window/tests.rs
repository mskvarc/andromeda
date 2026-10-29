use crate::csource_reconciled_window::ReconciledWindow;
use andromeda_ngsi_ld_model::{notification::CsrChange, registration::ContextSourceRegistration};
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serde_json::json;

fn instant(raw: &str) -> DateTime<Utc> {
    raw.parse().expect("valid instant")
}

fn registration(modified_at: &str) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:1",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "https://example.com/Vehicle"}]}],
        "endpoint": "https://source.example.com/",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": modified_at
    }))
    .expect("registration parses")
}

fn updated(modified_at: &str) -> CsrChange {
    CsrChange::Updated {
        before: Box::new(registration("2024-01-01T00:00:00Z")),
        written: Box::new(registration(modified_at)),
    }
}

/// A change committed while the reconciliation ran was read by it, so its own
/// event must not be reported again.
#[test]
fn an_update_written_inside_the_window_is_covered() {
    let mut window = ReconciledWindow::through(Some(instant("2024-01-03T00:00:00Z")));

    assert_eq!(window.covers(&updated("2024-01-03T00:00:00Z")), true);
    assert_eq!(window.covers(&updated("2024-01-02T00:00:00Z")), true);
}

/// The first change past the window shows the worker has moved on, and closes
/// it so a lagging clock cannot hide later changes.
#[test]
fn an_update_past_the_window_is_evaluated_and_closes_it() {
    let mut window = ReconciledWindow::through(Some(instant("2024-01-03T00:00:00Z")));

    assert_eq!(window.covers(&updated("2024-01-04T00:00:00Z")), false);
    assert_eq!(window.covers(&updated("2024-01-02T00:00:00Z")), false);
}

/// A deletion writes no registration, so it is evaluated whatever the window.
#[test]
fn a_deletion_is_never_covered() {
    let mut window = ReconciledWindow::through(Some(instant("2024-01-03T00:00:00Z")));
    let deleted = CsrChange::Deleted {
        before: Some(Box::new(registration("2024-01-02T00:00:00Z"))),
    };

    assert_eq!(window.covers(&deleted), false);
    assert_eq!(window.covers(&updated("2024-01-02T00:00:00Z")), true);
}

/// Without a reconciliation no window is open and nothing is covered.
#[test]
fn a_closed_window_covers_nothing() {
    let mut window = ReconciledWindow::default();

    assert_eq!(window.covers(&updated("2024-01-02T00:00:00Z")), false);
}
