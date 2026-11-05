//! Decoding the value of one member of a compact attribute instance, and
//! locating a member's refusal at it.
//!
//! A JSON `null` is no value of any member (ETSI GS CIM 009 clause 5.2.1), an
//! absent optional member is no value, and an instant member reads by the
//! clause 4.6.3 `DateTime` rules.

use crate::{
    attribute::instance_node::required,
    compact_member::decoded,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::{compact_node::CompactNode, value_budget::ValueBudget};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use serde::de::{DeserializeOwned, DeserializeSeed};
use std::{marker::PhantomData, str::FromStr};

/// The failure of reading a member of a node of type `N`.
pub(crate) type ReadError<'n, N> = NodeReadError<<N as CompactNode<'n>>::Unreadable>;

/// The attribute name an instance is written under.
///
/// # Errors
///
/// Returns [`NodeRefusal::AttributeName`] for a name that is no attribute name.
pub(crate) fn attribute_name(attribute: &str) -> Result<AttributeName, NodeRefusal> {
    AttributeName::from_str(attribute).map_err(NodeRefusal::AttributeName)
}

/// Decode the `unitCode` of an instance of the attribute named `attribute`:
/// absent is no unit, a JSON `null` is no value of any member (clause 5.2.1),
/// and one that is no UN/CEFACT common code refuses the instance (Table
/// 5.2.5-1).
///
/// # Errors
///
/// Returns [`NodeRefusal::Null`] for a `null` member,
/// [`NodeRefusal::UnitCodeUnusable`] for a member that does not decode as a
/// common code, and the node's [`CompactNode::Unreadable`].
pub(crate) fn unit_code_member<'n, N: CompactNode<'n>>(attribute: &str, member: Option<N>) -> Result<Option<UnitCode>, ReadError<'n, N>> {
    let Some(member) = member else {
        return Ok(None);
    };
    let member = required(member, RefusedMember::UnitCode)?;
    match decoded::<UnitCode, N>(member).map_err(NodeReadError::Unreadable)? {
        Ok(unit_code) => Ok(Some(unit_code)),
        Err(source) => Err(refused_member::<N>(
            RefusedMember::UnitCode,
            NodeRefusal::UnitCodeUnusable {
                attribute: attribute_name(attribute)?,
                source,
            },
        )),
    }
}

/// The refusal of the member `member`, located at it.
pub(crate) fn refused_member<'n, N: CompactNode<'n>>(member: RefusedMember, refusal: NodeRefusal) -> ReadError<'n, N> {
    ReadError::<N>::from(refusal).within_member(&member.to_string())
}

/// Decode a member that is present as a `T`, the refusal left for the caller
/// to locate: a JSON `null` is no value of any member (clause 5.2.1).
///
/// # Errors
///
/// Returns [`NodeRefusal::Null`] for a `null` member, [`NodeRefusal::Member`]
/// when the member does not decode as a `T`, and the node's
/// [`CompactNode::Unreadable`].
pub(crate) fn present_member<'n, T: DeserializeOwned, N: CompactNode<'n>>(member: N, name: RefusedMember) -> Result<T, ReadError<'n, N>> {
    present_member_within(member, name, |_| PhantomData::<T>)
}

/// Decode a member that is present by the seed `seed_for` builds from the
/// budget of values the member's entity may still hold, the refusal left for
/// the caller to locate: a JSON `null` is no value of any member (clause
/// 5.2.1).
///
/// # Errors
///
/// As [`present_member`].
pub(crate) fn present_member_within<'n, T, S, N>(
    member: N,
    name: RefusedMember,
    seed_for: impl FnOnce(Option<&'n ValueBudget>) -> S,
) -> Result<T, ReadError<'n, N>>
where
    S: for<'de> DeserializeSeed<'de, Value = T>,
    N: CompactNode<'n>,
{
    if member.is_null() {
        return Err(NodeRefusal::Null.into());
    }
    member
        .decoded_by(seed_for(member.value_budget()))
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Decode an optional member by the seed `seed_for` builds from the budget of
/// values the member's entity may still hold, located at it: absent is no
/// value.
///
/// # Errors
///
/// As [`present_member`].
pub(crate) fn optional_member_within<'n, T, S, N>(
    member: Option<N>,
    name: RefusedMember,
    seed_for: impl FnOnce(Option<&'n ValueBudget>) -> S,
) -> Result<Option<T>, ReadError<'n, N>>
where
    S: for<'de> DeserializeSeed<'de, Value = T>,
    N: CompactNode<'n>,
{
    member
        .map(|member| present_member_within(member, name, seed_for).map_err(|error| error.within_member(&name.to_string())))
        .transpose()
}

/// Decode an optional member, located at it: absent is no value.
///
/// # Errors
///
/// As [`present_member`].
pub(crate) fn optional_member<'n, T: DeserializeOwned, N: CompactNode<'n>>(member: Option<N>, name: RefusedMember) -> Result<Option<T>, ReadError<'n, N>> {
    member
        .map(|member| present_member(member, name).map_err(|error| error.within_member(&name.to_string())))
        .transpose()
}

/// Read an optional instant member (`observedAt`, `expiresAt`, a system
/// time) by the clause 4.6.3 `DateTime` rules.
///
/// # Errors
///
/// Returns [`NodeRefusal::Member`] when the member is not a clause 4.6.3
/// `DateTime`, and the node's [`CompactNode::Unreadable`].
pub(crate) fn optional_instant_member<'n, N: CompactNode<'n>>(member: Option<N>, name: RefusedMember) -> Result<Option<DateTime<Utc>>, ReadError<'n, N>> {
    Ok(optional_member::<NgsiLdDateTime, N>(member, name)?.map(NgsiLdDateTime::get))
}

/// Read the optional `datasetId` of an instance (clause 4.5.5).
///
/// "If no datasetId is provided, or "datasetId": "@none" is supplied, it is
/// considered as the default Attribute instance", so the keyword reads as no
/// `datasetId` at all and the instance is the one an absent member makes.
///
/// # Errors
///
/// Returns [`NodeRefusal::Member`] when the member is neither the `@none`
/// keyword nor a URI, and the node's [`CompactNode::Unreadable`].
pub(crate) fn optional_dataset_member<'n, N: CompactNode<'n>>(member: Option<N>) -> Result<Option<Identifier>, ReadError<'n, N>> {
    Ok(optional_member::<DatasetSelector, N>(member, RefusedMember::DatasetId)?.and_then(DatasetSelector::into_named))
}
