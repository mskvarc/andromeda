//! The exact success status of a typed answer: a typed call accepts every status the specification allows for its
//! success (a query page reads 200, 201 and 203 alike), so a test that pins one of them reads the answer with its
//! status and requires it. `with_heads()` is enough: the status is all it reads.

use http::StatusCode;
use ngsi_ld::client::{Answered, Received};

/// The broker answered a success other than the one required.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered {actual}, not {expected}")]
pub struct UnexpectedStatus {
    /// The status required.
    pub expected: StatusCode,
    /// The status the broker answered.
    pub actual: StatusCode,
}

/// A typed answer received with its status.
pub trait ExpectedStatus {
    /// The typed outcome.
    type Outcome;

    /// The outcome, when the broker answered `expected`.
    ///
    /// # Errors
    ///
    /// Returns [`UnexpectedStatus`] when it answered another status.
    fn expecting(self, expected: StatusCode) -> Result<Self::Outcome, UnexpectedStatus>;
}

impl<V> ExpectedStatus for Answered<V> {
    type Outcome = V;

    fn expecting(self, expected: StatusCode) -> Result<V, UnexpectedStatus> {
        if self.status == expected {
            Ok(self.outcome)
        } else {
            Err(UnexpectedStatus { expected, actual: self.status })
        }
    }
}

impl<V> ExpectedStatus for Received<V> {
    type Outcome = V;

    fn expecting(self, expected: StatusCode) -> Result<V, UnexpectedStatus> {
        if self.status == expected {
            Ok(self.outcome)
        } else {
            Err(UnexpectedStatus { expected, actual: self.status })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::received_status::ExpectedStatus;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::StatusCode;
    use ngsi_ld::{
        client::{Answered, Received},
        http::ResponseHead,
    };

    fn received(status: StatusCode) -> Received<u8> {
        Received::new(7, status, ResponseHead::default(), Bytes::new())
    }

    #[gtest]
    fn an_answer_with_its_head_is_held_to_its_status() -> Result<()> {
        let answered = Answered::new(7_u8, StatusCode::CREATED, ResponseHead::default());
        verify_that!(answered.expecting(StatusCode::CREATED)?, eq(7))
    }

    #[gtest]
    fn the_required_status_yields_the_outcome() -> Result<()> {
        verify_that!(received(StatusCode::CREATED).expecting(StatusCode::CREATED)?, eq(7))
    }

    #[gtest]
    fn another_success_is_refused() -> Result<()> {
        let refused = received(StatusCode::OK).expecting(StatusCode::CREATED);
        verify_that!(refused.map_err(|error| error.actual), err(eq(StatusCode::OK)))
    }
}
