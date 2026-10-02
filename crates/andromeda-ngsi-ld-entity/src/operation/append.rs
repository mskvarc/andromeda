//! Append Attributes (clause 5.6.3).

use crate::{
    attribute_instances::AttributeInstances,
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot, written_value::ValueSlot},
    entity_state::EntityState,
    operation::{
        attribute_write::apply_instances,
        instance_patch::{InstancePatchDepth, new_instance},
        mutation_error::MutationError,
        scope_write::{append_missing_scopes, replace_scope},
        selection::OverwritePolicy,
    },
    reads::member_name::MemberName,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Whether appending one attribute under `noOverwrite` skipped an instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Appended {
    /// Every written instance was appended.
    Whole,
    /// At least one written instance named an instance the entity holds.
    Partly,
}

impl EntityState {
    /// Append `fragment`'s attributes to the entity (clause 5.6.3).
    ///
    /// Under [`OverwritePolicy::Overwrite`] each written instance replaces the
    /// whole instance with its `datasetId` (the semantics of Update
    /// Attributes), and the scope "will become the one included in the
    /// Fragment" (clause 5.6.3.4). Under [`OverwritePolicy::Preserve`] the
    /// instances the entity holds are kept and only the others are appended,
    /// and only "the Scopes in the Fragment that are not part of the value of
    /// scope of the target Entity" are appended. The fragment's types are not
    /// merged: clause 5.6.3 appends attributes only.
    ///
    /// Returns the members with at least one instance not appended because
    /// [`OverwritePolicy::Preserve`] kept the held one, which decides between
    /// 204 and 207.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::Refused`] when a written instance cannot
    /// become state.
    pub fn append_attributes<'a>(&mut self, fragment: &'a EntityDocument, overwrite: OverwritePolicy) -> Result<Vec<MemberName<'a>>, MutationError> {
        let mut not_appended = Vec::new();
        if let Some(scope) = fragment.scope() {
            match overwrite {
                OverwritePolicy::Overwrite => replace_scope(self, scope),
                OverwritePolicy::Preserve => append_missing_scopes(self, scope),
            }
        }
        match (fragment.expires_at(), overwrite) {
            (Some(Slot::Set(_)), OverwritePolicy::Preserve) if self.expires_at().is_some() => not_appended.push(MemberName::ExpiresAt),
            // clone: the entity owns its copy of the borrowed fragment's expiry.
            (Some(Slot::Set(expires_at)), OverwritePolicy::Overwrite | OverwritePolicy::Preserve) => self.set_expires_at(Some(expires_at.clone())),
            (Some(Slot::Remove), OverwritePolicy::Overwrite) => self.set_expires_at(None),
            (Some(Slot::Remove), OverwritePolicy::Preserve) | (None, OverwritePolicy::Overwrite | OverwritePolicy::Preserve) => {}
        }
        for (name, member) in fragment.attributes() {
            let appended = match (member, overwrite) {
                (AttributeMember::Remove, OverwritePolicy::Overwrite) => {
                    self.attributes_mut().remove(name);
                    Appended::Whole
                }
                (AttributeMember::Remove, OverwritePolicy::Preserve) if self.attributes().contains(name) => Appended::Partly,
                (AttributeMember::Remove, OverwritePolicy::Preserve) => Appended::Whole,
                (AttributeMember::Instances(documents), OverwritePolicy::Overwrite) => {
                    apply_instances(self.attributes_mut(), name, documents, InstancePatchDepth::ReplaceInstance, None)?;
                    Appended::Whole
                }
                (AttributeMember::Instances(documents), OverwritePolicy::Preserve) => self.append_missing_instances(name, documents)?,
            };
            if appended == Appended::Partly {
                not_appended.push(MemberName::Attribute(name));
            }
        }
        Ok(not_appended)
    }

    /// Append the written instances of `name` whose `datasetId` the entity
    /// does not hold yet (clause 5.6.3.4 `noOverwrite`). A deletion has
    /// nothing to delete among instances that are not there, and is not
    /// stored.
    fn append_missing_instances(&mut self, name: &AttributeName, documents: &[InstanceDocument]) -> Result<Appended, MutationError> {
        let mut appended = Appended::Whole;
        for document in documents {
            let dataset_id = document.dataset_id();
            if self.attributes().get(name).is_some_and(|held| held.contains(dataset_id)) {
                appended = Appended::Partly;
                continue;
            }
            if let ValueSlot::Remove(_) = document.value() {
                continue;
            }
            let instance = new_instance(name, document, None)?;
            match self.attributes_mut().get_mut(name) {
                // clone: the instance is keyed by its own copy of the written identity.
                Some(held) => {
                    held.insert(dataset_id.cloned(), instance);
                }
                // clone: the entity owns the attribute name and the instance identity it gains.
                None => {
                    self.attributes_mut()
                        .insert(name.clone(), AttributeInstances::one(dataset_id.cloned(), instance));
                }
            }
        }
        Ok(appended)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{EntityDocument, attribute_member::AttributeMember, slot::Slot},
        operation::{
            selection::OverwritePolicy,
            speed_entity::{SPEED, date_time, entity, fragment, instances, number, property, property_document, values},
        },
        reads::member_name::MemberName,
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use indexmap::IndexMap;

    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    fn scope_fragment(paths: &[&str]) -> EntityDocument {
        EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(paths))), None, IndexMap::new())
    }

    #[test]
    fn append_no_overwrite_skips_existing() {
        let mut target = entity(vec![("https://example.com/a", instances(vec![(None, property(number(1)))]))]);
        let attrs = fragment(vec![
            ("https://example.com/a", AttributeMember::Instances(vec![property_document(number(99), None)])),
            ("https://example.com/b", AttributeMember::Instances(vec![property_document(number(2), None)])),
        ]);

        target.append_attributes(&attrs, OverwritePolicy::Preserve).expect("append succeeds");

        pretty_assertions::assert_eq!(values(&target, "https://example.com/a"), vec![1]);
        pretty_assertions::assert_eq!(values(&target, "https://example.com/b"), vec![2]);
    }

    /// NGSI-LD append semantics with the `noOverwrite` option, clause 5.6.3.
    #[test]
    fn appending_an_existing_attribute_with_preserve_leaves_it_untouched() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        let attrs = fragment(vec![(SPEED, AttributeMember::Instances(vec![property_document(number(20), None)]))]);

        let not_appended = target.append_attributes(&attrs, OverwritePolicy::Preserve).expect("append succeeds");

        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10]);
        pretty_assertions::assert_eq!(not_appended.iter().map(MemberName::as_str).collect::<Vec<_>>(), vec![SPEED]);
    }

    #[test]
    fn appending_an_existing_attribute_with_overwrite_replaces_it() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        let attrs = fragment(vec![(SPEED, AttributeMember::Instances(vec![property_document(number(20), None)]))]);

        let not_appended = target.append_attributes(&attrs, OverwritePolicy::Overwrite).expect("append succeeds");

        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
        assert!(not_appended.is_empty(), "{not_appended:?}");
    }

    /// Clause 5.6.3.4 under `noOverwrite`: an instance with a new `datasetId`
    /// is appended beside the held one.
    #[test]
    fn appending_a_new_dataset_with_preserve_adds_it_beside_the_held_one() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        let attrs = fragment(vec![(
            SPEED,
            AttributeMember::Instances(vec![property_document(number(20), Some("urn:ngsi-ld:Dataset:B"))]),
        )]);

        let not_appended = target.append_attributes(&attrs, OverwritePolicy::Preserve).expect("append succeeds");

        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 20]);
        assert!(not_appended.is_empty(), "{not_appended:?}");
    }

    /// Clause 5.6.3.4 under `noOverwrite` appends only the Scopes the target
    /// lacks.
    #[test]
    fn append_with_preserve_adds_only_the_missing_scopes() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/A/B", "/C/D"])));
        target
            .append_attributes(&scope_fragment(&["/A/B", "/E/F"]), OverwritePolicy::Preserve)
            .expect("append succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D", "/E/F"])));
    }

    /// Clause 5.6.3.4 with overwrite allowed: the scope "will become the one
    /// included in the Fragment".
    #[test]
    fn append_with_overwrite_replaces_the_scope() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/A/B", "/C/D"])));
        target
            .append_attributes(&scope_fragment(&["/E/F"]), OverwritePolicy::Overwrite)
            .expect("append succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/E/F"])));
    }

    /// Under `noOverwrite` an entity expiry the target already holds is kept
    /// and reported as not appended.
    #[test]
    fn append_with_preserve_keeps_and_reports_a_held_entity_expiry() {
        let mut target = entity(Vec::new());
        target.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        let attrs = EntityDocument::new(None, Vec::new(), None, Some(Slot::Set(date_time("2031-01-01T00:00:00Z"))), IndexMap::new());

        let not_appended = target.append_attributes(&attrs, OverwritePolicy::Preserve).expect("append succeeds");

        pretty_assertions::assert_eq!(not_appended, vec![MemberName::ExpiresAt]);
        pretty_assertions::assert_eq!(target.expires_at(), Some(&date_time("2030-01-01T00:00:00Z")));
    }
}
