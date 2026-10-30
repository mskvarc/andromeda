//! The error answer a forwarded operation acts on as a step of its own.
//!
//! Clause 6.3.17 lets the broker hold a source back after failures or
//! timeouts. An error answer the operation expects and follows up is neither:
//! the source behaved as the operation requires, so it counts as an answer
//! that succeeded and opens no cooldown window.

/// The error answer a forwarded request's operation acts on, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActedOnAnswer {
    /// The operation acts on no error answer: every error status is a
    /// failure of the source.
    NoErrorAnswer,
    /// Clause 5.6.8.4: a batch upsert driven entity by entity follows an
    /// `AlreadyExists` answer to its Create Entity with the Replace Entity or
    /// Update Attributes request its update mode calls for.
    AlreadyExists,
}

impl ActedOnAnswer {
    /// Whether an answer of `status` is the one the operation acts on.
    pub(crate) const fn covers(self, status: reqwest::StatusCode) -> bool {
        match self {
            ActedOnAnswer::NoErrorAnswer => false,
            ActedOnAnswer::AlreadyExists => status.as_u16() == reqwest::StatusCode::CONFLICT.as_u16(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::acted_on_answer::ActedOnAnswer;

    #[test]
    fn already_exists_covers_only_a_409() {
        assert!(ActedOnAnswer::AlreadyExists.covers(reqwest::StatusCode::CONFLICT));
        assert!(!ActedOnAnswer::AlreadyExists.covers(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!ActedOnAnswer::AlreadyExists.covers(reqwest::StatusCode::UNPROCESSABLE_ENTITY));
    }

    #[test]
    fn no_error_answer_covers_no_status() {
        assert!(!ActedOnAnswer::NoErrorAnswer.covers(reqwest::StatusCode::CONFLICT));
        assert!(!ActedOnAnswer::NoErrorAnswer.covers(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
    }
}
