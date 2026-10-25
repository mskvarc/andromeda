//! The clause 4.6.4 key rule, read off the typed compact entity.
//!
//! The rule is stated over the compact JSON of an entity: the keys JSON-LD
//! expansion would drop or rewrite past recovery
//! ([`andromeda_ngsi_ld_representation::compact_key_admission`]).
//! Clause 5.5.7's `@context` below the top level is not among them: it is part
//! of the request's user `@context`, refused by the user `@context` admission
//! (`andromeda_jsonld::user_context_admission`) before this screening runs. The
//! attribute types of clause 5.2.4, the seals of clause 4.5.2.2 and the
//! prohibited members of the family clauses are not among them either: the
//! decode of the request refuses a node any of those rules refuses, so a
//! decoded payload holds none. The extractor decodes the request straight into
//! the typed [`Entity`] or [`EntityPatch`], so the rule is applied here to what
//! that decode holds, member by member, rather than to a JSON copy of the whole
//! entity rendered back for it:
//!
//! - every client-written JSON value inside a typed instance is read by the
//!   representation crate's own walk, its location reported from the top of
//!   the entity;
//! - a typed instance is keyed by its family's members and by attribute names,
//!   so it can carry a refused key only inside a JSON value or a sub-attribute,
//!   and only those are read;
//! - a typed geometry is RFC 7946 positions and nothing else, so the rule has
//!   nothing to read in it, which is what keeps a large geometry out of the
//!   screening.
//!
//! The rule reports the first fault in the member order of the compact JSON it
//! is stated over.

use andromeda_ngsi_ld_expansion::written_member::{WrittenMember, WrittenMemberRef};
use andromeda_ngsi_ld_model::{
    attribute::key_screening::{member_segment, screen_keys_in_member, screen_keys_in_merge_member},
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::json_pointer::JsonPointerSegment;
use andromeda_ngsi_ld_representation::{compact_key_admission::CompactKeyError, embedded_context_admission::is_native_json_member};

/// Screen the attribute members of a decoded Entity.
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses.
pub(crate) fn screen_compact_entity<M: WrittenMember>(entity: &Entity<M>) -> Result<(), CompactKeyError> {
    screen_compact_members(entity.attribute_members())
}

/// Screen the attribute members of a decoded entity Fragment.
///
/// # Errors
///
/// As [`screen_compact_entity`].
pub(crate) fn screen_compact_patch<M: WrittenMember>(patch: &EntityPatch<M>) -> Result<(), CompactKeyError> {
    screen_compact_members(patch.attribute_members())
}

/// Screen the one attribute member a bare Attribute Fragment of clauses 5.6.4
/// and 5.6.19 writes to the attribute `name`.
///
/// # Errors
///
/// As [`screen_compact_entity`], located from the attribute's name.
pub(crate) fn screen_compact_attribute<M: WrittenMember>(name: &str, member: &M) -> Result<(), CompactKeyError> {
    if is_native_json_member(name) {
        return Ok(());
    }
    screen_keys_in_written_member(member, &[member_segment(name)])
}

/// Screen the attribute members of a decoded payload.
fn screen_compact_members<'a, M: WrittenMember + 'a>(members: impl Iterator<Item = (&'a str, &'a M)>) -> Result<(), CompactKeyError> {
    let mut members: Vec<_> = members.filter(|(name, _)| !is_native_json_member(name)).collect();
    // The rule walks a JSON object, whose members come in name order.
    members.sort_unstable_by_key(|(name, _)| *name);
    members
        .into_iter()
        .try_for_each(|(name, member)| screen_keys_in_written_member(member, &[member_segment(name)]))
}

/// The key rule over one member of an entity or of a merge fragment, standing
/// at `at`.
fn screen_keys_in_written_member<M: WrittenMember>(member: &M, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member.written() {
        WrittenMemberRef::Attribute(member) => screen_keys_in_member(member, at),
        WrittenMemberRef::Merge(member) => screen_keys_in_merge_member(member, at),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
