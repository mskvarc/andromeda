//! What a broker answered a call with, read the same way whatever the call: the status, the problem and body of an
//! error response, the header fields, and whether a body came at all. A typed call's success keeps no status, since
//! its outcome states it.

use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use ngsi_ld::{
    client::{Answered, ClientError, Received},
    http::ResponseError,
    model::{ProblemDetails, RawJson},
};

/// The status a call was answered with, when the broker answered.
///
/// It is the status of the error response for a failed call, and of the response for an unchecked request that
/// succeeded; a typed call that succeeded has no status here, since its outcome states it.
pub trait AnsweredStatus {
    /// The status, if the broker answered and the answer keeps it.
    fn answered_status(&self) -> Option<StatusCode>;
    /// The problem of the error response, if the broker reported one.
    fn answered_problem(&self) -> Option<&ProblemDetails>;
    /// The JSON body of the error response, as received, if the broker reported a problem.
    fn answered_body(&self) -> Option<&RawJson>;
    /// The header fields of the answer, if the broker answered.
    fn answered_headers(&self) -> Option<&HeaderMap>;
    /// Whether the answer had no body, when the answer tells.
    fn answered_without_body(&self) -> Option<bool>;
}

impl<E> AnsweredStatus for Result<Response<Bytes>, ClientError<E>> {
    fn answered_status(&self) -> Option<StatusCode> {
        match self {
            Ok(response) => Some(response.status()),
            Err(error) => error_status(error),
        }
    }

    fn answered_problem(&self) -> Option<&ProblemDetails> {
        self.as_ref().err().and_then(ClientError::problem)
    }

    fn answered_body(&self) -> Option<&RawJson> {
        self.as_ref().err().and_then(error_body)
    }

    fn answered_headers(&self) -> Option<&HeaderMap> {
        match self {
            Ok(response) => Some(response.headers()),
            Err(error) => error_headers(error),
        }
    }

    fn answered_without_body(&self) -> Option<bool> {
        match self {
            Ok(response) => Some(response.body().is_empty()),
            Err(error) => error_without_body(error),
        }
    }
}

impl<V, E> AnsweredStatus for Result<Received<V>, ClientError<E>> {
    fn answered_status(&self) -> Option<StatusCode> {
        match self {
            Ok(received) => Some(received.status),
            Err(error) => error_status(error),
        }
    }

    fn answered_problem(&self) -> Option<&ProblemDetails> {
        self.as_ref().err().and_then(ClientError::problem)
    }

    fn answered_body(&self) -> Option<&RawJson> {
        self.as_ref().err().and_then(error_body)
    }

    fn answered_headers(&self) -> Option<&HeaderMap> {
        match self {
            Ok(received) => Some(received.head.headers()),
            Err(error) => error_headers(error),
        }
    }

    fn answered_without_body(&self) -> Option<bool> {
        match self {
            Ok(received) => Some(received.body.is_empty()),
            Err(error) => error_without_body(error),
        }
    }
}

impl<V, E> AnsweredStatus for Result<Answered<V>, ClientError<E>> {
    fn answered_status(&self) -> Option<StatusCode> {
        match self {
            Ok(answered) => Some(answered.status),
            Err(error) => error_status(error),
        }
    }

    fn answered_problem(&self) -> Option<&ProblemDetails> {
        self.as_ref().err().and_then(ClientError::problem)
    }

    fn answered_body(&self) -> Option<&RawJson> {
        self.as_ref().err().and_then(error_body)
    }

    fn answered_headers(&self) -> Option<&HeaderMap> {
        match self {
            Ok(answered) => Some(answered.head.headers()),
            Err(error) => error_headers(error),
        }
    }

    fn answered_without_body(&self) -> Option<bool> {
        self.as_ref().err().and_then(error_without_body)
    }
}

impl<E> AnsweredStatus for ClientError<E> {
    fn answered_status(&self) -> Option<StatusCode> {
        error_status(self)
    }

    fn answered_problem(&self) -> Option<&ProblemDetails> {
        self.problem()
    }

    fn answered_body(&self) -> Option<&RawJson> {
        error_body(self)
    }

    fn answered_headers(&self) -> Option<&HeaderMap> {
        error_headers(self)
    }

    fn answered_without_body(&self) -> Option<bool> {
        error_without_body(self)
    }
}

/// The header fields of an error response.
fn error_headers<E>(error: &ClientError<E>) -> Option<&HeaderMap> {
    if let ClientError::Response(response) = error {
        Some(response.head().headers())
    } else {
        None
    }
}

/// Whether an error response had no body, when its decoding tells: a problem and a 406 carry one, a bare status
/// none.
const fn error_without_body<E>(error: &ClientError<E>) -> Option<bool> {
    if matches!(error, ClientError::Response(ResponseError::Status { .. })) {
        Some(true)
    } else if matches!(
        error,
        ClientError::Response(ResponseError::Problem { .. } | ResponseError::NotAcceptable { .. })
    ) {
        Some(false)
    } else {
        None
    }
}

/// The status of an error response.
const fn error_status<E>(error: &ClientError<E>) -> Option<StatusCode> {
    if let ClientError::Response(response) = error {
        Some(response_status(response))
    } else {
        None
    }
}

/// The JSON body of an error response that reports a problem.
const fn error_body<E>(error: &ClientError<E>) -> Option<&RawJson> {
    if let ClientError::Response(ResponseError::Problem { body, .. }) = error {
        Some(body)
    } else {
        None
    }
}

/// The status of `response`.
const fn response_status(response: &ResponseError) -> StatusCode {
    response.status()
}
