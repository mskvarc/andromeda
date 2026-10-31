//! `Prefer: respond-async` on a batch operation (IETF RFC 7240 section 4.1), an Andromeda extension no typed
//! operation writes: the header joins a typed request, and the answer states it was applied.

use crate::problem::{AnsweredMatcher, answered};
use http::{HeaderName, HeaderValue, StatusCode};
use ngsi_ld::http::UncheckedRequest;

/// The token of the preference.
const RESPOND_ASYNC: &str = "respond-async";

/// `request` asking the broker to answer before the work is done.
#[must_use]
pub fn respond_async(request: UncheckedRequest) -> UncheckedRequest {
    request.append_header(HeaderName::from_static("prefer"), HeaderValue::from_static(RESPOND_ASYNC))
}

/// An answer with `status` that carries `Preference-Applied: respond-async`.
#[must_use]
pub fn async_applied(status: StatusCode) -> AnsweredMatcher {
    answered(status).with_header(HeaderName::from_static("preference-applied"), RESPOND_ASYNC)
}

#[cfg(test)]
mod tests {
    use crate::respond_async::async_applied;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Response, StatusCode};
    use ngsi_ld::client::ClientError;
    use std::{io, result::Result as StdResult};

    /// The answer of an unchecked request in these tests.
    type Answer = StdResult<Response<Bytes>, ClientError<io::Error>>;

    fn accepted(preference: Option<&str>) -> Result<Answer> {
        let mut answer = Response::builder().status(StatusCode::NO_CONTENT);
        if let Some(preference) = preference {
            answer = answer.header("preference-applied", preference);
        }
        Ok(Ok(answer.body(Bytes::new())?))
    }

    #[gtest]
    fn an_answer_naming_the_preference_applied_matches() -> Result<()> {
        expect_that!(&accepted(Some("respond-async"))?, async_applied(StatusCode::NO_CONTENT));
        expect_that!(&accepted(None)?, not(async_applied(StatusCode::NO_CONTENT)));
        verify_that!(&accepted(Some("wait=10"))?, not(async_applied(StatusCode::NO_CONTENT)))
    }
}
