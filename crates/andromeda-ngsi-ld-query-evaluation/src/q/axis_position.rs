//! Where a walk down the `.` axis of an attribute reference stands.

use crate::{
    q::{
        family_target::{TerminalKeyword, family_target, first_family_value},
        target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue},
    },
    queried_entity::{
        attribute_reading::{QueriedInstance, QueriedValueNode},
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_query::filter::{instance_metadata::InstanceMetadata, path::AttrPath, target_language::LanguageSelector};

/// Where a walk down the `.` axis stands.
///
/// A `.` segment names a sub-attribute, and the walk stands on each of its
/// instances. It may instead name a member the instance carries as data of its
/// own: an [`InstanceMetadata`] member, which ends the walk on the target value
/// it holds, or the family's value member itself (`speed.value`), which lands
/// on the first value. From a structured value the walk continues into its
/// members.
pub(crate) enum AxisPosition<'a, I: QueriedInstance<'a>> {
    /// An attribute or sub-attribute instance.
    Instance(I),
    /// One value of a family member or of a structured value's member.
    Value(ValueItem<'a, I::Node>),
    /// The target value an instance member that is not a sub-attribute holds.
    Metadata(TargetValue<'a>),
}

impl<'a, I: QueriedInstance<'a>> AxisPosition<'a, I> {
    /// The positions one `.` segment further down: one per instance of a
    /// sub-attribute, at most one for any other member, and none when the
    /// segment names nothing there.
    pub(crate) fn step(self, name: &AttributeName) -> impl Iterator<Item = AxisPosition<'a, I>> {
        let (member, sub_attribute) = match self {
            AxisPosition::Instance(instance) => {
                if let Some(member) = InstanceMetadata::named(name) {
                    (TargetValue::of_metadata(member, instance.metadata(member)).map(AxisPosition::Metadata), None)
                } else if instance.family().value_key().as_str() == name.as_str() {
                    (first_family_value(instance).map(AxisPosition::Value), None)
                } else {
                    (None, Some(instance.sub_attribute_instances(name).map(AxisPosition::Instance)))
                }
            }
            AxisPosition::Value(ValueItem::Node(node)) => (node.member_values(name).next().map(AxisPosition::Value), None),
            AxisPosition::Value(ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Opaque) | AxisPosition::Metadata(..) => {
                (None, None)
            }
        };
        member.into_iter().chain(sub_attribute.into_iter().flatten())
    }

    /// The `LanguageProperty` instance at this position with the target
    /// language `path`'s trailing group names, or `None` when the trailing
    /// group addresses a compound member instead (see
    /// [`AttrPath::target_language`]).
    pub(crate) fn target_language<'p>(&self, path: &'p AttrPath) -> Option<(I, LanguageSelector<'p>)> {
        match self {
            AxisPosition::Instance(instance) if instance.family() == AttributeType::LanguageProperty => {
                path.target_language().map(|language| (*instance, language))
            }
            AxisPosition::Instance(_) | AxisPosition::Value(_) | AxisPosition::Metadata(..) => None,
        }
    }

    /// The first value of the family member at this position, the node a
    /// compound value's members hang off.
    pub(crate) fn first_family_value(&self) -> Option<ValueItem<'a, I::Node>> {
        match self {
            AxisPosition::Instance(instance) => first_family_value(*instance),
            AxisPosition::Value(_) | AxisPosition::Metadata(..) => None,
        }
    }

    /// The target value this position holds, or `None` when it holds none.
    pub(crate) fn terminal(self) -> Option<ResolvedTarget<'a>> {
        let value = match self {
            AxisPosition::Instance(instance) => return family_target(instance),
            AxisPosition::Value(item) => TerminalKeyword::Identifier.read(item)?,
            AxisPosition::Metadata(value) => value,
        };
        Some(ResolvedTarget {
            value,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        })
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
