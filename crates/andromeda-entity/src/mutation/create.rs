//! Create Entity (clause 5.6.1).

use crate::{EntityService, admitted_write::AdmittedEntity, error::EntityError, notification_plan::creation, write_kind::CreateEntity};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::Precondition,
    write_outcome::single,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Create the new NGSI-LD entity `id` from its admitted payload, returning
    /// its URI on success (clause 5.6.1).
    ///
    /// Clause 5.6.1.4 opens with the clause 5.5.4 validation, which produced
    /// `admitted` before the operation forwarded anything.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the written document is no
    /// entity a creation can store (clause 5.6.1.4), [`EntityError::AlreadyExists`] when another entity already holds
    /// the identifier (clause 6.4.3.1), and [`EntityError::Persistence`] when
    /// the store could not commit the insert.
    pub async fn create(&self, target: &ContextTarget, id: &Identifier, admitted: AdmittedEntity<CreateEntity>) -> Result<Identifier, EntityError> {
        let timer = entity::start(EntityOperation::Create);
        let result: Result<Identifier, EntityError> = async {
            // Clause 4.8: a created entity's attribute instances carry the write
            // instant as both createdAt and modifiedAt.
            let mut created = EntityState::try_from(admitted.into_document())?;
            created.settle(None, SystemInstant::new(Utc::now()));
            let created = Arc::new(created);
            let plan = self
                .notifier
                .plan(target, id, created.types().as_slice(), creation(&created), Some(&created), None);

            let write = AtomicWrite::builder()
                .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
                .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::clone(&created))?)) // clone: Arc ref-count bump; the state is also the change event's
                .history(vec![HistoryRecord::Observations(created)])
                .subscription_matches(plan.matches)
                .precondition(Precondition::MustNotExist)
                .build();

            // A conflicting insert is 409, per NGSI-LD 6.4.3.1. It reaches here as a
            // precondition failure rather than a duplicate-key error, which is what
            // lets the same statement serve a partially-succeeding batch.
            if single(self.writes.commit(target, vec![write]).await?)?.is_already_exists() {
                return Err(EntityError::already_exists(id));
            }

            plan.events.into_iter().for_each(|event| self.notifier.publish(target, event));

            Ok(id.clone()) // clone: the caller owns the created identifier while the request entity remains borrowed
        }
        .await;
        timer.finish_result(&result);
        result
    }
}
