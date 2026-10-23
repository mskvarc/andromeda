//! The member names `serde_json`'s reader reserves for its own tokens.
//!
//! With `arbitrary_precision`, the reader hands a number that is not a 64-bit
//! integer to a visitor as a one-member map, and a raw value the same way. An
//! object whose first member carries one of these names is read as that token,
//! so a text naming such a member is told apart from a token by where the name
//! stands.

/// The prefix of the member names the reader reserves for its
/// arbitrary-precision number and raw-value tokens.
pub const SERDE_JSON_PRIVATE_MEMBER_PREFIX: &str = "$serde_json::private::";

/// The member name the reader spells a number token with when it hands a
/// number that is not a 64-bit integer to a visitor as a one-member map.
pub const SERDE_JSON_PRIVATE_NUMBER_MEMBER: &str = "$serde_json::private::Number";
