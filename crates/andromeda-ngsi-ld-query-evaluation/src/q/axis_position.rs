//! Where a walk down the `.` axis of an attribute reference stands.

use crate::{
    q::{
        family_target::{family_target, first_family_value},
        target_value::{ResolvedTarget, TargetValue},
    },
    queried_entity::{attribute_reading::QueriedInstance, value_item::ValueItem},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{
    instance_metadata::InstanceMetadata,
    path::AttrPath,
    reading::family_reading::{MultiValueReading, TargetOrigin, ValueKeys, family_reading},
    target_language::LanguageSelector,
};

/// Where a walk down the `.` axis stands.
///
/// A `.` segment names a sub-attribute, and the walk stands on each of its
/// instances. It may instead name an [`InstanceMetadata`] member the instance
/// carries as data of its own, which ends the walk on the target value it
/// holds. Clause 4.9 makes each name after the first "a (sub)Property or
/// (sub)Relationship", and a family's value member is neither, so a segment
/// naming it (`speed.value`) is a sub-attribute name no instance holds: a
/// compound value is entered through the trailing path alone.
pub(crate) enum AxisPosition<'a, I: QueriedInstance<'a>> {
    /// An attribute or sub-attribute instance.
    Instance(I),
    /// The target value an instance member that is not a sub-attribute holds.
    Metadata(TargetValue<'a>),
}

impl<'a, I: QueriedInstance<'a>> AxisPosition<'a, I> {
    /// The positions one `.` segment further down: one per instance of a
    /// sub-attribute, at most one for an instance member, and none when the
    /// segment names nothing there.
    pub(crate) fn step(self, name: &AttributeName) -> impl Iterator<Item = AxisPosition<'a, I>> {
        let (member, sub_attribute) = match self {
            AxisPosition::Instance(instance) => match InstanceMetadata::named(name) {
                Some(member) => (TargetValue::of_metadata(member, instance.metadata(member)).map(AxisPosition::Metadata), None),
                None => (None, Some(instance.sub_attribute_instances(name).map(AxisPosition::Instance))),
            },
            AxisPosition::Metadata(..) => (None, None),
        };
        member.into_iter().chain(sub_attribute.into_iter().flatten())
    }

    /// The `LanguageProperty` instance at this position with the target
    /// language `path`'s trailing group names, or `None` when the trailing
    /// group addresses a compound member instead (see
    /// [`AttrPath::target_language`]).
    pub(crate) fn target_language<'p>(&self, path: &'p AttrPath) -> Option<(I, LanguageSelector<'p>)> {
        match self {
            AxisPosition::Instance(instance) if family_reading(instance.family()).keys == ValueKeys::Languages => {
                path.target_language().map(|language| (*instance, language))
            }
            AxisPosition::Instance(_) | AxisPosition::Metadata(..) => None,
        }
    }

    /// The first value of the family member at this position, the node a
    /// compound value's members hang off.
    pub(crate) fn first_family_value(&self) -> Option<ValueItem<'a, I::Node>> {
        match self {
            AxisPosition::Instance(instance) => first_family_value(*instance),
            AxisPosition::Metadata(..) => None,
        }
    }

    /// The target value this position holds, or `None` when it holds none.
    pub(crate) fn terminal(self) -> Option<ResolvedTarget<'a>> {
        match self {
            AxisPosition::Instance(instance) => family_target(instance),
            AxisPosition::Metadata(value) => Some(ResolvedTarget {
                value,
                origin: TargetOrigin::AttributeValue,
                reading: MultiValueReading::WholeArray,
            }),
        }
    }
}

/// Every position the sub-attribute chain `sub_attrs` reaches from one
/// attribute instance, one per instance of each sub-attribute on the way.
///
/// Clause 4.9: "If a Property has multiple instances (identified by its
/// respective datasetId), and no datasetId is explicitly addressed, the target
/// value shall be any Value of such instances". The `q` grammar addresses no
/// datasetId, so a sub-attribute's instances fan out exactly as an attribute's
/// do.
pub(crate) fn axis_positions<'a, I: QueriedInstance<'a>>(instance: I, sub_attrs: &[AttributeName]) -> Vec<AxisPosition<'a, I>> {
    sub_attrs.iter().fold(vec![AxisPosition::Instance(instance)], |positions, name| {
        positions.into_iter().flat_map(|position| position.step(name)).collect()
    })
}
