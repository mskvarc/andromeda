//! The `cursor` parameter a request carries (clause 5.5.9.1).

use andromeda_ngsi_ld_query::cursor::{CursorDecodeError, PageCursor};
use thiserror::Error;

/// What a request's `cursor` parameter asks for.
///
/// The token inside a `prev`/`next`/`last` pointer is broker-emitted and
/// opaque, so the only thing a client writes itself is the empty value: a
/// `cursor=` with nothing after it starts a cursor walk from the beginning of
/// the collection, which is how a client opts into the clause 5.5.9.1 pointers
/// where the operator made offset pagination the default, and what the
/// `first` pointer of every cursor page carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorRequest {
    /// Begin a cursor walk at the start of the collection.
    Start,
    /// Continue the walk from the position a followed pointer carried.
    Continue(PageCursor),
}

/// A `cursor` token that names no position (clause 5.5.9.1).
///
/// The token is broker-emitted and followed unchanged, so a token that does
/// not decode was corrupted or forged; it is input that does not meet the
/// requirements of the operation, a `BadRequestData` (clause 6.3.20).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CursorRequestError {
    /// The token did not decode to a cursor.
    #[error("invalid pagination cursor: {0}")]
    Undecodable(#[source] CursorDecodeError),
}

impl TryFrom<&str> for CursorRequest {
    type Error = CursorRequestError;

    /// Read the value of a `cursor` parameter: empty starts a walk, anything
    /// else is the opaque token of a position.
    ///
    /// # Errors
    ///
    /// Returns [`CursorRequestError::Undecodable`] for a non-empty token that
    /// does not decode to a [`PageCursor`].
    fn try_from(value: &str) -> Result<CursorRequest, CursorRequestError> {
        if value.is_empty() {
            return Ok(CursorRequest::Start);
        }
        PageCursor::try_from(value)
            .map(CursorRequest::Continue)
            .map_err(CursorRequestError::Undecodable)
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination::cursor_request::{CursorRequest, CursorRequestError};
    use andromeda_ngsi_ld_query::cursor::PageCursor;
    use pretty_assertions::assert_eq;

    /// An empty `cursor=` starts a walk rather than naming a position.
    #[test]
    fn an_empty_cursor_starts_a_walk() {
        assert_eq!(CursorRequest::try_from(""), Ok(CursorRequest::Start));
    }

    /// A broker-emitted token continues the walk from its position.
    #[test]
    fn a_token_continues_a_walk() {
        let cursor = PageCursor::After("urn:e:1".parse().expect("valid URI"));
        assert_eq!(CursorRequest::try_from(cursor.encode().as_str()), Ok(CursorRequest::Continue(cursor)));
    }

    /// A token that does not decode is refused.
    #[test]
    fn a_malformed_token_is_refused() {
        assert!(matches!(CursorRequest::try_from("%%%"), Err(CursorRequestError::Undecodable(_))));
    }
}
