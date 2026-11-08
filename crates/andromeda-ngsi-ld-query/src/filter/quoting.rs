//! How a `q` token was written in the query text (clause 4.9).
//!
//! Clause 4.9's grammar distinguishes the two spellings itself:
//! `ComparableValue = Number / quotedStr / dateTime / date / time`, where
//! `quotedStr` is the IETF RFC 8259 `String` production *including its quotes*
//! while `dateTime`, `date` and `time` are separate, unquoted alternatives: the
//! clause writes them without quotes in its own EXAMPLE 8,
//! `?q=temperature.observedAt>=2017-12-24T12:00:00Z`. So the quotes are part of
//! the derivation, not decoration on it, and whether a literal carried them
//! decides which productions can derive it.
//!
//! The tokeniser is the only place that knows, so it says so here rather than
//! discarding it: an operand's classification and an attribute name's
//! re-splitting both turn on the same fact.

/// How a name or literal was written in the query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quoting {
    /// Written bare, so the tokeniser may have glued following `:` characters
    /// to it (`:` continues an identifier), and only the unquoted productions
    /// of clause 4.9's grammar can derive it.
    Bare,
    /// Written inside double quotes, so it ends exactly where the client ended
    /// it, any colon it contains belongs to it, and clause 4.9's `quotedStr`
    /// derives it whatever its content.
    Quoted,
}
