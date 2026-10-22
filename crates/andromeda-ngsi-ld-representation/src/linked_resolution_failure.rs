//! Why a Linked Entity did not reach the response (clause 4.5.23).
//!
//! A join that cannot resolve one of the entities it was asked for still
//! answers: clause 4.5.23 leaves the Relationship naming a plain URI rather
//! than failing the retrieval. What the client is owed instead is a statement
//! of what the body is missing, and that statement is the same whether the
//! join walked entities (clause 4.5.23) or their temporal evolution (the same
//! clause applied to the temporal API), which is why the vocabulary for it sits
//! beside [`crate::join_mode`] rather than inside either traversal.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::fmt;
use strum::Display;

/// What stood between a Linked Entity and its resolution.
///
/// The four cases are the ones a caller can act on differently, and they are
/// what an HTTP binding needs in order to pick the warning code clause 6.3.17
/// puts on the response. The condition is named rather than the code: a
/// traversal has no business knowing about response headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LinkedResolutionFailureKind {
    /// The data that answered was served from a cache and may be out of date.
    StaleData,
    /// Something answered, but what it sent back could not be used.
    UnusableResponse,
    /// Nothing answered in time.
    Unreachable,
    /// The answer was a refusal that a retry is not expected to change.
    Refused,
}

/// One Linked Entity that a join could not resolve, and why.
///
/// A failure never fails the retrieval — clause 4.5.23 leaves an unresolvable
/// Relationship as a plain URI — so it travels back to the caller as a
/// description of what the response is missing rather than as an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedResolutionFailure {
    /// The Linked Entity that stayed unresolved.
    pub id: Identifier,
    /// What stood in the way.
    pub kind: LinkedResolutionFailureKind,
    /// Human-readable detail, safe to put in front of a client.
    pub detail: String,
}

impl LinkedResolutionFailure {
    /// The Linked Entity `id` could not be fetched at all, for the reason
    /// `cause` states.
    #[must_use]
    pub fn unreachable(id: &Identifier, cause: &impl fmt::Display) -> LinkedResolutionFailure {
        LinkedResolutionFailure::stating(id, LinkedResolutionFailureKind::Unreachable, cause)
    }

    /// The Linked Entity `id` was answered for, but the answer could not be
    /// read, for the reason `cause` states.
    #[must_use]
    pub fn unusable(id: &Identifier, cause: &impl fmt::Display) -> LinkedResolutionFailure {
        LinkedResolutionFailure::stating(id, LinkedResolutionFailureKind::UnusableResponse, cause)
    }

    /// The failure of `kind` on `id`, detailed by what `cause` displays.
    fn stating(id: &Identifier, kind: LinkedResolutionFailureKind, cause: &impl fmt::Display) -> LinkedResolutionFailure {
        LinkedResolutionFailure {
            id: id.clone(), // clone: the failure outlives the candidate it names
            kind,
            detail: cause.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    #[test]
    fn each_constructor_names_its_condition_and_carries_the_cause() {
        let id: Identifier = "urn:ngsi-ld:Building:1".parse().expect("valid URI");
        pretty_assertions::assert_eq!(
            [
                LinkedResolutionFailure::unreachable(&id, &"timed out"),
                LinkedResolutionFailure::unusable(&id, &"no id")
            ],
            [
                LinkedResolutionFailure {
                    id: id.clone(), // clone: the expectation and the built failure both name it
                    kind: LinkedResolutionFailureKind::Unreachable,
                    detail: "timed out".to_owned(),
                },
                LinkedResolutionFailure {
                    id: id.clone(), // clone: same
                    kind: LinkedResolutionFailureKind::UnusableResponse,
                    detail: "no id".to_owned(),
                },
            ]
        );
    }

    /// The conditions render in kebab-case, which is what a log line and a
    /// warning detail both carry.
    #[test]
    fn each_condition_renders_in_kebab_case() {
        let rendered: Vec<String> = [
            LinkedResolutionFailureKind::StaleData,
            LinkedResolutionFailureKind::UnusableResponse,
            LinkedResolutionFailureKind::Unreachable,
            LinkedResolutionFailureKind::Refused,
        ]
        .into_iter()
        .map(|kind| kind.to_string())
        .collect();

        pretty_assertions::assert_eq!(rendered, vec!["stale-data", "unusable-response", "unreachable", "refused"]);
    }
}
