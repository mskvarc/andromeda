//! The answers a test's context source gives in turn, and the answers a
//! source gives that the tests script more than once.

use serde_json::json;
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use wiremock::{Request, Respond, ResponseTemplate};

/// Answers each request with the next answer it holds, and every request past
/// them with its last one.
pub(crate) struct InTurn {
    /// The answers not given yet, in order.
    answers: Mutex<VecDeque<ResponseTemplate>>,
    /// The answer once the others are given.
    last: ResponseTemplate,
}

impl InTurn {
    /// `first` answered once, then `last` to every request after it.
    pub(crate) fn new(first: ResponseTemplate, last: ResponseTemplate) -> InTurn {
        InTurn {
            answers: Mutex::new(VecDeque::from([first])),
            last,
        }
    }
}

impl Respond for InTurn {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.answers
            .lock()
            .expect("the answers are never poisoned")
            .pop_front()
            .unwrap_or_else(|| self.last.clone())
    }
}

/// An acceptance that comes two seconds late, past the forwarding timeout a
/// test gives its registration, so the placement gets no answer in time while
/// the source may hold the copy.
pub(crate) fn late_acceptance() -> ResponseTemplate {
    ResponseTemplate::new(201).set_delay(Duration::from_secs(2))
}

/// The answer of a source asked to create a subscription it already holds
/// (`AlreadyExists`, clause 5.5.2).
pub(crate) fn already_exists() -> ResponseTemplate {
    ResponseTemplate::new(409).set_body_json(json!({
        "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists",
        "title": "Already exists",
        "detail": "a subscription with this id already exists"
    }))
}
