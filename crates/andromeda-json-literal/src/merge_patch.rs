//! Merging one literal into another (clauses 4.5.24 and 5.5.12).
//!
//! A `JsonProperty`'s `json` member "contains data which is not available for
//! JSON-LD interpretation" (clause 4.5.24.2), yet clause 5.5.12 still merges
//! "up to an arbitrary depth, e.g. including going into JSON objects
//! representing a Property value", and clause 5.5.4 excepts merge operations
//! alone from its prohibition of the NGSI-LD Null "as the value of a key value
//! pair within a JSON object". The merge is IETF RFC 7396 with the NGSI-LD Null
//! string of clause 4.5.0 in place of `null`: inside a literal a JSON `null`
//! is ordinary data.
//!
//! The merged literal is written from the pieces of the two admitted ones, so
//! its text is canonical as it stands. Members keep the target's order, and a
//! member only the patch names follows them in the patch's order.

use crate::{
    JsonLiteral,
    admission::JsonLiteralError,
    null_sentinel::is_null_string,
    piece::{JsonKind, JsonPiece},
};
use std::iter::once;

impl JsonLiteral {
    /// The literal `patch` makes of this one, per IETF RFC 7396 with the
    /// NGSI-LD Null in place of `null`.
    ///
    /// A patch that is not an object replaces the literal outright, as RFC
    /// 7396 states; a literal that is not an object is replaced by the object
    /// the patch describes, because there is nothing in it to merge into.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::Syntax`] when a member name cannot be
    /// written, which no admitted literal causes.
    pub fn merged(&self, patch: &JsonLiteral) -> Result<JsonLiteral, JsonLiteralError> {
        merged_text(Some(self.piece()), patch.piece())
    }

    /// The literal merged into nothing: every member set to the NGSI-LD Null
    /// is left out, at any depth of nested objects.
    ///
    /// # Errors
    ///
    /// As [`JsonLiteral::merged`].
    pub fn settled(&self) -> Result<JsonLiteral, JsonLiteralError> {
        merged_text(None, self.piece())
    }
}

/// The literal `patch` makes of `target`.
fn merged_text(target: Option<JsonPiece<'_>>, patch: JsonPiece<'_>) -> Result<JsonLiteral, JsonLiteralError> {
    let mut written = String::with_capacity(patch.text().len() + target.map_or(0, |target| target.text().len()));
    write_merged(target, patch, &mut written)?;
    JsonLiteral::from_admitted_text(written)
}

/// Write the value `patch` makes of `target`.
fn write_merged(target: Option<JsonPiece<'_>>, patch: JsonPiece<'_>, written: &mut String) -> Result<(), JsonLiteralError> {
    let Some(patch_members) = patch.members() else {
        written.push_str(patch.text());
        return Ok(());
    };
    let target_members = target.and_then(JsonPiece::members).unwrap_or_default();
    let patched = |name: &str| patch_members.iter().find_map(|(patched, value)| (patched == name).then_some(*value));
    let mut first = true;
    written.push('{');
    for (name, held) in &target_members {
        match patched(name) {
            None => write_member(name, written, &mut first, |written| {
                written.push_str(held.text());
                Ok(())
            })?,
            Some(value) => write_patched_member(name, Some(*held), value, written, &mut first)?,
        }
    }
    for (name, value) in &patch_members {
        if target_members.iter().all(|(held, _)| held != name) {
            write_patched_member(name, None, *value, written, &mut first)?;
        }
    }
    written.push('}');
    Ok(())
}

/// Write the member `name` the patch sets to `value`: left out when the value
/// is the NGSI-LD Null, merged when it is an object, written as it stands
/// otherwise.
///
/// IETF RFC 7396 section 2 recurses into every object the patch carries,
/// whether or not the target holds the member, so a Null nested under a member
/// the target lacks is dropped rather than written as data.
fn write_patched_member(name: &str, held: Option<JsonPiece<'_>>, value: JsonPiece<'_>, written: &mut String, first: &mut bool) -> Result<(), JsonLiteralError> {
    if is_null_string(value) {
        return Ok(());
    }
    write_member(name, written, first, |written| match value.kind() {
        JsonKind::Object => write_merged(held, value, written),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array => {
            written.push_str(value.text());
            Ok(())
        }
    })
}

/// Write one member: its separator, its quoted name, and the value `value`
/// writes.
fn write_member<F>(name: &str, written: &mut String, first: &mut bool, value: F) -> Result<(), JsonLiteralError>
where
    F: FnOnce(&mut String) -> Result<(), JsonLiteralError>,
{
    if !*first {
        written.push(',');
    }
    *first = false;
    let quoted = serde_json::to_string(name).map_err(|source| JsonLiteralError::Syntax { source })?;
    written.extend(once(quoted.as_str()).chain(once(":")));
    value(written)
}
