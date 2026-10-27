//! The clause 6.3.17 warning of a source whose paged answer the walk could not read to its end.

use crate::source_continuation::UnfollowableNext;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// What a source did that ended the walk of its answer early.
#[derive(Debug, Error)]
pub(crate) enum IncompleteReason<'a> {
    /// The source cut its history short at an instant the walk cannot resume past.
    #[error("cut its history short at an instant it cannot be resumed past")]
    HistoryCutShort,
    /// The source's next-page pointer led back to a page it already answered.
    #[error("pointed back at a page it already answered")]
    PointedBack,
    /// The source no longer found the page it had pointed at.
    #[error("no longer found the next page of its answer")]
    NextPageGone,
    /// The source answered an empty page that still points at a next one.
    #[error("answered an empty page that still points at a next one")]
    EmptyPageWithNext,
    /// The source's next-page pointer is not followed.
    #[error("{0}")]
    Unfollowable(&'a UnfollowableNext),
}

impl IncompleteReason<'_> {
    /// The Table 6.3.17-1 code of the reason: every one of them follows a
    /// response that arrived in time, so a later page answered 404 is an error
    /// response (299) and anything else is a response that cannot be used
    /// (111).
    const fn code(&self) -> WarnCode {
        match self {
            IncompleteReason::NextPageGone => WarnCode::MiscellaneousPersistentWarning,
            IncompleteReason::HistoryCutShort | IncompleteReason::PointedBack | IncompleteReason::EmptyPageWithNext | IncompleteReason::Unfollowable(_) => {
                WarnCode::RevalidationFailed
            }
        }
    }
}

/// The walk of a source's answer stopped before its end.
#[derive(Debug, Error)]
#[error("context source {registration} {reason}; its answer is incomplete")]
struct IncompleteAnswer<'a> {
    /// The registration whose answer is incomplete.
    registration: &'a Identifier,
    /// What the source did.
    reason: IncompleteReason<'a>,
}

/// The clause 6.3.17 warning of a source whose answer the walk could not read
/// to its end, for the `reason` it stopped.
pub(crate) fn incomplete_answer(registration: &Identifier, reason: IncompleteReason<'_>) -> NgsiLdWarning {
    NgsiLdWarning::new(reason.code(), IncompleteAnswer { registration, reason })
}

/// The clause 6.3.17 warning of the source `registration` names, whose history
/// stopped inside an instant the response cannot be resumed past.
#[must_use]
pub fn history_cut_short(registration: &Identifier) -> NgsiLdWarning {
    incomplete_answer(registration, IncompleteReason::HistoryCutShort)
}
