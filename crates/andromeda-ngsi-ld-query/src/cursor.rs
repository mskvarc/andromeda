//! The opaque, keyset-backed pagination cursor (clause 5.5.9.1).
//!
//! Clause 5.5.9.1 permits the pagination pointers a collection response carries
//! to be "implemented in a completely opaque way": the broker emits the pointer
//! inside the clause 6.3.10 `Link` header and the client only follows it, never
//! constructing or reading its contents. A [`PageCursor`] is that opaque
//! contents. It records a position in a collection ordered by its identifier,
//! and the direction to walk from there, and nothing else: no server-side state
//! and no expiry, so it costs the broker no storage. The query the position
//! belongs to is not part of the cursor: the request layer binds it beside the
//! token, in a digest every cursor pointer carries.
//!
//! The wire form is a `base64url` (no padding) token of a one-byte direction tag
//! followed by the boundary identifier's own spelling. Because it is opaque, the
//! only thing a client can do wrong with it is corrupt it; every corruption --
//! invalid base64, an unknown tag, a missing or malformed identifier -- is a
//! [`CursorDecodeError`], which the request boundary reports as `BadRequestData`.

use crate::page_reach::PageReach;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use thiserror::Error;

/// The direction tag of a forward cursor: `id > boundary`, ascending.
const TAG_AFTER: u8 = b'a';
/// The direction tag of a backward cursor: `id < boundary`, descending.
const TAG_BEFORE: u8 = b'b';
/// The direction tag of the from-end cursor: the final page, descending.
const TAG_END: u8 = b'e';

/// A position in an identifier-ordered collection, carried opaquely inside a
/// clause 6.3.10 `Link` pointer.
///
/// The three variants are the three directions a page can be walked from: past
/// the boundary ([`PageCursor::After`]), before it ([`PageCursor::Before`]), or
/// from the end of the collection ([`PageCursor::End`]). Only the first two name
/// a boundary, so the boundary lives inside the variant that has one rather than
/// in an `Option` every reader would have to defend against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageCursor {
    /// Forward from the boundary identifier: the store selects `id > boundary`
    /// ascending, so the page follows the one that ended at `boundary`.
    After(Identifier),
    /// Backward from the boundary identifier: the store selects `id < boundary`
    /// descending and the page is reversed to ascending for display, so it
    /// precedes the one that began at `boundary`.
    Before(Identifier),
    /// The final page: the store selects the last identifiers descending and the
    /// page is reversed to ascending for display. This is the from-end anchor
    /// Andromeda offers beyond the clause 5.5.9.1 `prev`/`next` pair.
    End,
}

impl PageCursor {
    /// The opaque `base64url` token that carries this cursor inside a `Link`.
    ///
    /// The token is the direction tag byte followed, for a bounded cursor, by
    /// the boundary identifier's own spelling. [`PageCursor::try_from`] is its
    /// exact inverse.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut bytes = Vec::new();
        match self {
            PageCursor::After(id) => {
                bytes.push(TAG_AFTER);
                bytes.extend_from_slice(id.as_str().as_bytes());
            }
            PageCursor::Before(id) => {
                bytes.push(TAG_BEFORE);
                bytes.extend_from_slice(id.as_str().as_bytes());
            }
            PageCursor::End => bytes.push(TAG_END),
        }
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// The neighbouring-page cursors a returned page offers.
    ///
    /// `incoming` is the cursor the page was fetched under, or `None` when the
    /// page carried neither cursor nor offset -- the first page of a cursor
    /// walk. `reach` is whether the collection continues past the page in the
    /// direction it was read. The boundary identifiers are the first and last
    /// of the page as displayed, ascending.
    ///
    /// Clause 5.5.9.1: `prev` is absent on the page holding the first elements
    /// and `next` on the page holding the last, however the page was reached.
    /// The side a page was read towards is known from `reach`; the side it was
    /// read from is known from `incoming`: a forward fetch has elements before
    /// its boundary, a backward one after it, a fresh walk nothing before it and
    /// the from-end page nothing after it. `last` is the from-end anchor and is
    /// always offered.
    #[must_use]
    pub fn neighbours(incoming: Option<&PageCursor>, first_id: Identifier, last_id: Identifier, reach: PageReach) -> NeighbourCursors {
        let continues = matches!(reach, PageReach::Continues);
        let (has_prev, has_next) = match incoming {
            None => (false, continues),
            Some(PageCursor::After(_)) => (true, continues),
            Some(PageCursor::Before(_)) => (continues, true),
            Some(PageCursor::End) => (continues, false),
        };
        NeighbourCursors {
            prev: has_prev.then_some(PageCursor::Before(first_id)),
            next: has_next.then_some(PageCursor::After(last_id)),
            last: PageCursor::End,
        }
    }
}

impl TryFrom<&str> for PageCursor {
    type Error = CursorDecodeError;

    /// Decode an opaque token back into a cursor, the exact inverse of
    /// [`PageCursor::encode`].
    ///
    /// # Errors
    ///
    /// Returns [`CursorDecodeError`] for any corruption of the token: invalid
    /// `base64url`, an empty token, an unrecognised direction tag, a bounded
    /// cursor with no boundary identifier or a non-UTF-8 or non-URI one, or a
    /// from-end cursor that carries a boundary it should not.
    fn try_from(token: &str) -> Result<PageCursor, CursorDecodeError> {
        let bytes = URL_SAFE_NO_PAD.decode(token).map_err(|_| CursorDecodeError::Base64)?;
        let (tag, rest) = bytes.split_first().ok_or(CursorDecodeError::Empty)?;
        match *tag {
            TAG_AFTER => Ok(PageCursor::After(decode_boundary(rest)?)),
            TAG_BEFORE => Ok(PageCursor::Before(decode_boundary(rest)?)),
            TAG_END if rest.is_empty() => Ok(PageCursor::End),
            TAG_END => Err(CursorDecodeError::EndCarriesBoundary),
            other => Err(CursorDecodeError::UnknownTag(other)),
        }
    }
}

/// Parse the boundary identifier of a bounded cursor from the token's remainder.
fn decode_boundary(bytes: &[u8]) -> Result<Identifier, CursorDecodeError> {
    if bytes.is_empty() {
        return Err(CursorDecodeError::MissingBoundary);
    }
    let text = str::from_utf8(bytes).map_err(|_| CursorDecodeError::BoundaryNotUtf8)?;
    text.parse().map_err(|_| CursorDecodeError::BoundaryNotUri)
}

/// The cursors a returned page points its client at (clause 5.5.9.1).
///
/// `prev` and `next` are the clause 5.5.9.1 pair; `last` is the from-end anchor
/// Andromeda adds. Each present cursor is encoded and placed in a `Link` pointer;
/// an absent `prev`/`next` marks a boundary of the collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighbourCursors {
    /// The previous page, absent on the first page of the walk.
    pub prev: Option<PageCursor>,
    /// The following page, absent on the last page of the walk.
    pub next: Option<PageCursor>,
    /// The final page; always [`PageCursor::End`].
    pub last: PageCursor,
}

/// A pagination cursor that could not be decoded (clause 5.5.9.1).
///
/// Because the cursor is opaque and broker-emitted, every one of these means the
/// token reaching the broker was corrupted or forged rather than followed as
/// given. The request boundary reports all of them as `BadRequestData`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CursorDecodeError {
    /// The token was not valid `base64url`.
    #[error("pagination cursor is not valid base64url")]
    Base64,
    /// The token decoded to no bytes, so it carried no direction tag.
    #[error("pagination cursor is empty")]
    Empty,
    /// The direction tag was none of the three defined values.
    #[error("pagination cursor carries an unrecognised direction tag")]
    UnknownTag(u8),
    /// A forward or backward cursor carried no boundary identifier.
    #[error("a forward or backward pagination cursor carries no boundary identifier")]
    MissingBoundary,
    /// A from-end cursor carried a boundary identifier, which it must not.
    #[error("a from-end pagination cursor must not carry a boundary identifier")]
    EndCarriesBoundary,
    /// The boundary identifier's bytes were not valid UTF-8.
    #[error("a pagination cursor's boundary identifier is not valid UTF-8")]
    BoundaryNotUtf8,
    /// The boundary identifier was not a valid NGSI-LD URI.
    #[error("a pagination cursor's boundary identifier is not a valid URI")]
    BoundaryNotUri,
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor::{CursorDecodeError, PageCursor, TAG_AFTER, TAG_END},
        page_reach::PageReach,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    /// A forward cursor round-trips through its opaque token unchanged.
    #[test]
    fn after_cursor_round_trips() {
        let cursor = PageCursor::After(uri("urn:ngsi-ld:Building:001"));
        let token = cursor.encode();
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()).expect("decodes"), cursor);
    }

    /// A backward cursor round-trips through its opaque token unchanged.
    #[test]
    fn before_cursor_round_trips() {
        let cursor = PageCursor::Before(uri("urn:ngsi-ld:Building:042"));
        let token = cursor.encode();
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()).expect("decodes"), cursor);
    }

    /// The from-end cursor round-trips through its opaque token unchanged.
    #[test]
    fn end_cursor_round_trips() {
        let token = PageCursor::End.encode();
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()).expect("decodes"), PageCursor::End);
    }

    /// An identifier carrying non-ASCII characters (clause 4.6.2 NOTE 2) survives
    /// the round trip character for character.
    #[test]
    fn a_non_ascii_boundary_round_trips_verbatim() {
        let cursor = PageCursor::After(uri("urn:ngsi-ld:Ciudad:PARÍS"));
        let token = cursor.encode();
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()).expect("decodes"), cursor);
    }

    /// The token uses the `base64url` alphabet with no padding, so it never
    /// carries a character a URL query value would have to escape.
    #[test]
    fn the_token_is_url_safe_and_unpadded() {
        let token = PageCursor::After(uri("urn:ngsi-ld:Building:001")).encode();
        assert!(!token.contains('='), "no padding: {token}");
        assert!(!token.contains('+') && !token.contains('/'), "url-safe alphabet: {token}");
    }

    /// A token that is not valid base64url is rejected rather than mistaken for a
    /// position.
    #[test]
    fn invalid_base64_is_rejected() {
        pretty_assertions::assert_eq!(PageCursor::try_from("not base64!!!"), Err(CursorDecodeError::Base64));
    }

    /// An empty token carries no direction tag and is rejected.
    #[test]
    fn an_empty_token_is_rejected() {
        pretty_assertions::assert_eq!(PageCursor::try_from(""), Err(CursorDecodeError::Empty));
    }

    /// A token whose first byte is none of the three tags is rejected.
    #[test]
    fn an_unknown_direction_tag_is_rejected() {
        let token = URL_SAFE_NO_PAD.encode(b"z");
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()), Err(CursorDecodeError::UnknownTag(b'z')));
    }

    /// A forward tag with no following bytes names no boundary and is rejected.
    #[test]
    fn a_bounded_cursor_with_no_boundary_is_rejected() {
        let token = URL_SAFE_NO_PAD.encode([TAG_AFTER]);
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()), Err(CursorDecodeError::MissingBoundary));
    }

    /// A from-end tag followed by boundary bytes is contradictory and is rejected.
    #[test]
    fn a_from_end_cursor_carrying_a_boundary_is_rejected() {
        let mut bytes = vec![TAG_END];
        bytes.extend_from_slice(b"urn:ngsi-ld:Building:001");
        let token = URL_SAFE_NO_PAD.encode(bytes);
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()), Err(CursorDecodeError::EndCarriesBoundary));
    }

    /// A boundary whose bytes are not valid UTF-8 is rejected.
    #[test]
    fn a_non_utf8_boundary_is_rejected() {
        let token = URL_SAFE_NO_PAD.encode([TAG_AFTER, 0xFF, 0xFE]);
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()), Err(CursorDecodeError::BoundaryNotUtf8));
    }

    /// A boundary that is valid UTF-8 but not a URI is rejected.
    #[test]
    fn a_non_uri_boundary_is_rejected() {
        let mut bytes = vec![TAG_AFTER];
        bytes.extend_from_slice(b"not a uri");
        let token = URL_SAFE_NO_PAD.encode(bytes);
        pretty_assertions::assert_eq!(PageCursor::try_from(token.as_str()), Err(CursorDecodeError::BoundaryNotUri));
    }

    /// The first page of a walk offers no `prev`, a `next` while the
    /// collection continues past it, and the from-end anchor.
    #[test]
    fn first_page_neighbours_offer_next_and_last_only() {
        let neighbours = PageCursor::neighbours(None, uri("urn:ngsi-ld:E:001"), uri("urn:ngsi-ld:E:020"), PageReach::Continues);
        assert!(neighbours.prev.is_none());
        pretty_assertions::assert_eq!(neighbours.next, Some(PageCursor::After(uri("urn:ngsi-ld:E:020"))));
        pretty_assertions::assert_eq!(neighbours.last, PageCursor::End);
    }

    /// A first page that exhausted the collection is the only page: it offers no `next`.
    #[test]
    fn an_exhausted_first_page_offers_no_next() {
        let neighbours = PageCursor::neighbours(None, uri("urn:ngsi-ld:E:001"), uri("urn:ngsi-ld:E:005"), PageReach::Exhausted);
        assert!(neighbours.prev.is_none());
        assert!(neighbours.next.is_none());
    }

    /// A forward page offers a `prev` back to before its first identifier and a
    /// `next` while the collection continues past it.
    #[test]
    fn a_forward_page_offers_prev_and_next() {
        let incoming = PageCursor::After(uri("urn:ngsi-ld:E:020"));
        let neighbours = PageCursor::neighbours(Some(&incoming), uri("urn:ngsi-ld:E:021"), uri("urn:ngsi-ld:E:040"), PageReach::Continues);
        pretty_assertions::assert_eq!(neighbours.prev, Some(PageCursor::Before(uri("urn:ngsi-ld:E:021"))));
        pretty_assertions::assert_eq!(neighbours.next, Some(PageCursor::After(uri("urn:ngsi-ld:E:040"))));
    }

    /// Clause 5.5.9.1: a forward page holding the last elements offers no `next`, however full it is.
    #[test]
    fn a_forward_page_that_exhausted_the_collection_offers_no_next() {
        let incoming = PageCursor::After(uri("urn:ngsi-ld:E:020"));
        let neighbours = PageCursor::neighbours(Some(&incoming), uri("urn:ngsi-ld:E:021"), uri("urn:ngsi-ld:E:040"), PageReach::Exhausted);
        pretty_assertions::assert_eq!(neighbours.prev, Some(PageCursor::Before(uri("urn:ngsi-ld:E:021"))));
        assert!(neighbours.next.is_none());
    }

    /// A backward page always offers a `next` (the boundary it came from is
    /// ahead) and a `prev` only while elements precede it.
    #[test]
    fn a_backward_page_offers_next_always_and_prev_while_elements_precede_it() {
        let incoming = PageCursor::Before(uri("urn:ngsi-ld:E:041"));
        let neighbours = PageCursor::neighbours(Some(&incoming), uri("urn:ngsi-ld:E:021"), uri("urn:ngsi-ld:E:040"), PageReach::Continues);
        pretty_assertions::assert_eq!(neighbours.prev, Some(PageCursor::Before(uri("urn:ngsi-ld:E:021"))));
        pretty_assertions::assert_eq!(neighbours.next, Some(PageCursor::After(uri("urn:ngsi-ld:E:040"))));
    }

    /// Clause 5.5.9.1: "there will be no previous NGSI-LD Elements" on the page
    /// holding the first elements, so a backward page that holds them offers
    /// no `prev`, however full it is.
    #[test]
    fn a_backward_page_holding_the_first_elements_offers_no_prev() {
        let incoming = PageCursor::Before(uri("urn:ngsi-ld:E:021"));
        let neighbours = PageCursor::neighbours(Some(&incoming), uri("urn:ngsi-ld:E:001"), uri("urn:ngsi-ld:E:020"), PageReach::Exhausted);
        assert!(neighbours.prev.is_none());
        pretty_assertions::assert_eq!(neighbours.next, Some(PageCursor::After(uri("urn:ngsi-ld:E:020"))));
    }

    /// The from-end page offers no `next` and a `prev` only while elements
    /// precede it.
    #[test]
    fn the_from_end_page_offers_no_next() {
        let preceded = PageCursor::neighbours(Some(&PageCursor::End), uri("urn:ngsi-ld:E:081"), uri("urn:ngsi-ld:E:100"), PageReach::Continues);
        pretty_assertions::assert_eq!(preceded.prev, Some(PageCursor::Before(uri("urn:ngsi-ld:E:081"))));
        assert!(preceded.next.is_none());
        let only = PageCursor::neighbours(Some(&PageCursor::End), uri("urn:ngsi-ld:E:001"), uri("urn:ngsi-ld:E:020"), PageReach::Exhausted);
        assert!(only.prev.is_none(), "a from-end page holding the first elements is the only page");
        assert!(only.next.is_none());
    }
}
