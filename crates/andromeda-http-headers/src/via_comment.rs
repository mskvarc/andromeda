//! The comment that may close a `Via` element (RFC 9110 sections 5.6.5 and
//! 7.6.3, the grammar of RFC 7230 sections 3.2.6 and 5.7.1).
//!
//! A comment is `"(" *( ctext / quoted-pair / comment ) ")"`. It nests, a
//! backslash escapes the octet after it, and `ctext` admits `obs-text`, so a
//! comment can hold a comma, a parenthesis and octets beyond ASCII. Its extent
//! is found by depth, never by searching for a delimiter.

use thiserror::Error;

/// Why an octet run is not one comment.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ViaCommentError {
    /// An octet that neither `ctext` nor a quoted pair admits: a control
    /// character or DEL.
    #[error("via comment holds the octet {offending:#04x}, which a comment does not admit")]
    InvalidOctet {
        /// The first octet a comment does not admit.
        offending: u8,
    },
    /// The run does not open with `(` and close with the `)` that balances it,
    /// or a backslash escapes nothing.
    #[error("via comment is not one balanced parenthesised comment")]
    Unbalanced,
}

/// The comment of a `Via` element, held as written: the enclosing parentheses,
/// nested comments and quoted pairs included.
///
/// RFC 9110 section 7.6.3 lets a comment identify the software of a recipient,
/// and section 5.6.5 admits `obs-text` in it, so the comment is octets with no
/// character encoding. Andromeda never reads it, and sends it on unchanged when
/// it extends the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaComment(Vec<u8>);

impl ViaComment {
    /// Validate `written` as one comment, parentheses included.
    ///
    /// # Errors
    ///
    /// Returns [`ViaCommentError::InvalidOctet`] for an octet outside `ctext`
    /// and the quoted pair, and [`ViaCommentError::Unbalanced`] when `written`
    /// is not one comment from its first octet to its last.
    pub fn new(written: &[u8]) -> Result<ViaComment, ViaCommentError> {
        if let Some(&offending) = written.iter().find(|octet| !is_comment_octet(**octet)) {
            return Err(ViaCommentError::InvalidOctet { offending });
        }
        if comment_len(written) != Some(written.len()) {
            return Err(ViaCommentError::Unbalanced);
        }
        Ok(ViaComment(written.to_vec()))
    }

    /// The comment as written, parentheses included.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Whether a comment admits `octet`, as `ctext`, as one of its delimiters, or
/// as the octet a quoted pair escapes: HTAB, SP, VCHAR or `obs-text`.
const fn is_comment_octet(octet: u8) -> bool {
    matches!(octet, b'\t' | b' '..=b'~' | 0x80..=0xFF)
}

/// The length of the comment that opens `octets`, through the `)` that
/// balances its opening `(`, or `None` when `octets` opens with no `(` or the
/// comment never closes.
///
/// A quoted pair's second octet is data, so `\)` neither closes a comment nor
/// `\(` opens one.
#[must_use]
pub fn comment_len(octets: &[u8]) -> Option<usize> {
    if octets.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0_usize;
    let mut escaped = false;
    for (index, octet) in octets.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match octet {
            b'\\' => escaped = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _other => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::via_comment::{ViaComment, ViaCommentError, comment_len};

    #[test]
    fn plain_nested_escaped_and_obs_text_comments_are_one_comment_each() {
        for written in [
            b"(Apache)".as_slice(),
            b"(proxy, (zone 2))",
            br"(a \) b)",
            br"(a \( b)",
            b"(Z\xc3\xbcrich proxy)",
            b"(\xff)",
            b"()",
            b"(tab\there)",
        ] {
            let comment = ViaComment::new(written).expect("one comment");
            pretty_assertions::assert_eq!(comment.as_bytes(), written);
        }
    }

    #[test]
    fn an_unbalanced_run_or_a_dangling_escape_is_not_a_comment() {
        for written in [b"(open".as_slice(), b"(a) (b)", b"a (b)", b")", br"(a \", b"", b"(a))"] {
            pretty_assertions::assert_eq!(ViaComment::new(written), Err(ViaCommentError::Unbalanced), "{written:?}");
        }
    }

    #[test]
    fn a_control_octet_or_del_is_refused() {
        pretty_assertions::assert_eq!(ViaComment::new(b"(a\nb)"), Err(ViaCommentError::InvalidOctet { offending: b'\n' }));
        pretty_assertions::assert_eq!(ViaComment::new(b"(\x7f)"), Err(ViaCommentError::InvalidOctet { offending: 0x7f }));
    }

    /// The extent of a comment ends at the parenthesis that balances the
    /// first, so what follows it is left to the caller.
    #[test]
    fn the_extent_ends_at_the_balancing_parenthesis() {
        pretty_assertions::assert_eq!(comment_len(b"(a (b), c), 1.1 next"), Some(10));
        pretty_assertions::assert_eq!(comment_len(br"(a \) b), 1.1 next"), Some(8));
        pretty_assertions::assert_eq!(comment_len(b"(never closed, 1.1 next"), None);
        pretty_assertions::assert_eq!(comment_len(b"no comment"), None);
    }
}
