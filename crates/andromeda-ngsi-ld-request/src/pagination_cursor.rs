//! The request's opaque pagination cursor parameter (clause 5.5.9.1).
//!
//! Every collection endpoint that offers keyset-backed pagination reads its
//! `cursor` parameter the same way: a present token is decoded into a
//! [`PageCursor`], and it may not be combined with `offset`. Both rules are
//! written once here so a new endpoint inherits them rather than restating them.

use crate::error::ParameterError;
use andromeda_ngsi_ld_query::cursor::PageCursor;

/// Decode the raw `cursor` query parameter into a [`PageCursor`], if one was
/// present.
///
/// # Errors
///
/// Returns [`ParameterError::CursorInvalid`] when the token is present but
/// cannot be decoded -- corrupt base64, an unknown direction, or a missing or
/// malformed boundary identifier (clause 5.5.9.1).
pub fn decode_cursor(cursor: Option<&str>) -> Result<Option<PageCursor>, ParameterError> {
    match cursor {
        None => Ok(None),
        Some(token) => PageCursor::try_from(token).map(Some).map_err(ParameterError::CursorInvalid),
    }
}

/// Reject a request that supplied both `cursor` and `offset` (clause 5.5.9).
///
/// # Errors
///
/// Returns [`ParameterError::CursorWithOffset`] when both are present: a cursor
/// is a keyset continuation and an offset its offset-based alternative, so the
/// two cannot both govern the same page.
pub const fn validate_cursor_offset_exclusion(cursor: Option<&str>, offset: Option<u32>) -> Result<(), ParameterError> {
    if cursor.is_some() && offset.is_some() {
        return Err(ParameterError::CursorWithOffset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absent cursor decodes to no position.
    #[test]
    fn an_absent_cursor_decodes_to_none() {
        pretty_assertions::assert_eq!(decode_cursor(None).expect("no cursor"), None);
    }

    /// A present, well-formed cursor decodes to its position.
    #[test]
    fn a_valid_cursor_decodes_to_its_position() {
        let token = PageCursor::After("urn:ngsi-ld:Building:001".parse().expect("valid URI")).encode();
        pretty_assertions::assert_eq!(
            decode_cursor(Some(token.as_str())).expect("valid cursor"),
            Some(PageCursor::After("urn:ngsi-ld:Building:001".parse().expect("valid URI")))
        );
    }

    /// A malformed cursor is a `BadRequestData` parameter error.
    #[test]
    fn a_malformed_cursor_is_a_parameter_error() {
        let error = decode_cursor(Some("not base64!!!")).expect_err("malformed cursor");
        assert!(matches!(error, ParameterError::CursorInvalid(_)), "{error}");
    }

    /// `cursor` and `offset` together are rejected.
    #[test]
    fn cursor_with_offset_is_rejected() {
        let token = PageCursor::End.encode();
        let error = validate_cursor_offset_exclusion(Some(token.as_str()), Some(20)).expect_err("mutually exclusive");
        assert!(matches!(error, ParameterError::CursorWithOffset), "{error}");
    }

    /// A cursor without an offset, or an offset without a cursor, is accepted.
    #[test]
    fn cursor_or_offset_alone_is_accepted() {
        let token = PageCursor::End.encode();
        assert!(validate_cursor_offset_exclusion(Some(token.as_str()), None).is_ok());
        assert!(validate_cursor_offset_exclusion(None, Some(20)).is_ok());
        assert!(validate_cursor_offset_exclusion(None, None).is_ok());
    }
}
