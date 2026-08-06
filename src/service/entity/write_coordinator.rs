//! Committing atomic writes, and the one generic implementation.
//!
//! There is one coordinator rather than one per combination of stores. Naming
//! a coordinator per profile would be combinatorial — every live store times
//! every history store times every queue — so the coordinator is generic over
//! the anchor and the profile is data.

use crate::{
    core::error::AndromedaError,
    service::entity::{anchor_store::AnchorStore, atomic_write::AtomicWrite, write_outcome::WriteOutcome},
};
use async_trait::async_trait;
use std::collections::HashSet;

/// Committing a set of independent single-entity writes.
#[async_trait]
pub trait WriteCoordinator: Send + Sync + 'static {
    /// Commit every write, returning one outcome per write in the order given.
    ///
    /// The writes are independent: clauses 5.6.7 through 5.6.10 permit a batch
    /// to succeed partially, so a write whose precondition does not hold
    /// reports that and leaves the rest alone.
    async fn commit(&self, writes: Vec<AtomicWrite>) -> Result<Vec<WriteOutcome>, AndromedaError>;
}

/// The generic coordinator: apply what the anchor owns, in one of its
/// transactions, and let it publish whatever the commit made visible.
#[derive(Debug)]
pub struct AnchoredWriteCoordinator<A> {
    anchor: A,
}

impl<A: AnchorStore> AnchoredWriteCoordinator<A> {
    /// Drive atomic writes through `anchor`.
    pub const fn new(anchor: A) -> AnchoredWriteCoordinator<A> {
        AnchoredWriteCoordinator { anchor }
    }
}

/// Reject a set of writes the anchor cannot serve as one commit.
///
/// Two writes for one identifier would race each other inside a single fused
/// statement, and a part the anchor does not own would silently vanish; both
/// are caller errors rather than conditions to report per entity.
fn validate<A: AnchorStore>(anchor: &A, writes: &[AtomicWrite]) -> Result<(), AndromedaError> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(writes.len());
    for write in writes {
        if !seen.insert(&write.entity_id) {
            return Err(AndromedaError::internal(format!(
                "atomic write batch names {} twice; identifiers must be distinct within one commit",
                write.entity_id
            )));
        }
        for part in write.parts() {
            if !anchor.native_parts().contains(&part) {
                return Err(AndromedaError::internal(format!(
                    "the anchor store does not persist {part:?}, and no deferred sink is configured for it"
                )));
            }
        }
    }
    Ok(())
}

/// The writes whose entity mutation actually happened.
///
/// A write whose precondition did not hold contributes no history, no outbox
/// entries and no side effects: the batch paths have always followed the
/// identifiers that landed rather than the ones they submitted, and this is
/// that rule in one place.
fn landed<'a>(writes: &'a [AtomicWrite], outcomes: &[WriteOutcome]) -> Vec<&'a AtomicWrite> {
    writes
        .iter()
        .zip(outcomes)
        .filter(|(_, outcome)| match outcome {
            WriteOutcome::Created | WriteOutcome::Updated | WriteOutcome::Deleted(_) | WriteOutcome::NoEntityChange => true,
            WriteOutcome::AlreadyExists | WriteOutcome::NotFound | WriteOutcome::Superseded | WriteOutcome::Skipped => false,
        })
        .map(|(write, _)| write)
        .collect()
}

#[async_trait]
impl<A: AnchorStore> WriteCoordinator for AnchoredWriteCoordinator<A> {
    async fn commit(&self, writes: Vec<AtomicWrite>) -> Result<Vec<WriteOutcome>, AndromedaError> {
        if writes.is_empty() {
            return Ok(Vec::new());
        }
        validate(&self.anchor, &writes)?;

        let mut tx = self.anchor.begin().await?;
        let outcomes = self.anchor.apply_entities(&mut tx, &writes).await?;
        if outcomes.len() != writes.len() {
            return Err(AndromedaError::internal(format!(
                "anchor store reported {} outcomes for {} writes",
                outcomes.len(),
                writes.len()
            )));
        }

        let landed = landed(&writes, &outcomes);
        self.anchor.apply_history(&mut tx, &landed).await?;
        self.anchor.apply_outbox(&mut tx, &landed).await?;
        self.anchor.apply_side_effects(&mut tx, &landed).await?;

        self.anchor.commit(tx).await?;
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::{AnchoredWriteCoordinator, WriteCoordinator};
    use crate::{
        core::error::AndromedaError,
        service::entity::{
            anchor_store::AnchorStore,
            atomic_write::{ALL_WRITE_PARTS, AtomicWrite, WritePart},
            entity_mutation::EntityMutation,
            history_record::HistoryRecord,
            write_outcome::WriteOutcome,
        },
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// Records what it was asked to do and returns scripted entity outcomes.
    struct RecordingAnchor {
        native: Vec<WritePart>,
        scripted: Mutex<Vec<WriteOutcome>>,
        calls: Mutex<Vec<String>>,
        landed_ids: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl RecordingAnchor {
        fn new(scripted: Vec<WriteOutcome>) -> RecordingAnchor {
            RecordingAnchor {
                native: ALL_WRITE_PARTS.to_vec(),
                scripted: Mutex::new(scripted),
                calls: Mutex::new(Vec::new()),
                landed_ids: Mutex::new(Vec::new()),
            }
        }

        fn without(part: WritePart) -> RecordingAnchor {
            RecordingAnchor {
                native: ALL_WRITE_PARTS.iter().copied().filter(|candidate| *candidate != part).collect(),
                scripted: Mutex::new(Vec::new()),
                calls: Mutex::new(Vec::new()),
                landed_ids: Mutex::new(Vec::new()),
            }
        }

        fn record(&self, call: &str) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(call.to_string());
            }
        }

        fn record_landed(&self, call: &str, landed: &[&AtomicWrite]) {
            self.record(call);
            if let Ok(mut ids) = self.landed_ids.lock() {
                ids.push((call.to_string(), landed.iter().map(|write| write.entity_id.to_string()).collect()));
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().map(|calls| calls.clone()).unwrap_or_default() // clone: the guard cannot leave the lock
        }

        fn landed_for(&self, call: &str) -> Vec<String> {
            self.landed_ids
                .lock()
                .ok()
                .and_then(|ids| ids.iter().find(|(name, _)| name == call).map(|(_, entries)| entries.clone())) // clone: the guard cannot leave the lock
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl AnchorStore for Arc<RecordingAnchor> {
        type Transaction = ();

        fn native_parts(&self) -> &[WritePart] {
            &self.native
        }

        async fn begin(&self) -> Result<(), AndromedaError> {
            self.record("begin");
            Ok(())
        }

        async fn apply_entities(&self, _tx: &mut (), writes: &[AtomicWrite]) -> Result<Vec<WriteOutcome>, AndromedaError> {
            self.record("entities");
            let mut scripted = self
                .scripted
                .lock()
                .map_err(|_poisoned| AndromedaError::internal("scripted outcomes poisoned"))?;
            let taken: Vec<WriteOutcome> = scripted.drain(..).collect();
            if taken.len() == writes.len() {
                Ok(taken)
            } else {
                Ok(writes.iter().map(|_| WriteOutcome::Created).collect())
            }
        }

        async fn apply_history(&self, _tx: &mut (), landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
            self.record_landed("history", landed);
            Ok(())
        }

        async fn apply_outbox(&self, _tx: &mut (), landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
            self.record_landed("outbox", landed);
            Ok(())
        }

        async fn apply_side_effects(&self, _tx: &mut (), landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
            self.record_landed("side_effects", landed);
            Ok(())
        }

        async fn commit(&self, _tx: ()) -> Result<(), AndromedaError> {
            self.record("commit");
            Ok(())
        }
    }

    fn write(id: &str) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::from(id))
            .entity(EntityMutation::Delete)
            .history(vec![HistoryRecord::Observations(Arc::new(json!([{}])))])
            .build()
    }

    fn coordinator(anchor: &Arc<RecordingAnchor>) -> AnchoredWriteCoordinator<Arc<RecordingAnchor>> {
        AnchoredWriteCoordinator::new(Arc::clone(anchor)) // clone: Arc ref-count bump so the test keeps its handle
    }

    #[tokio::test]
    async fn duplicate_identifiers_in_one_commit_are_rejected() {
        let anchor = Arc::new(RecordingAnchor::new(Vec::new()));
        let error = coordinator(&anchor)
            .commit(vec![write("urn:ngsi-ld:A"), write("urn:ngsi-ld:A")])
            .await
            .expect_err("duplicate identifiers must be refused");
        assert!(error.to_string().contains("twice"), "{error}");
        assert!(anchor.calls().is_empty(), "the transaction must not be opened");
    }

    #[tokio::test]
    async fn a_part_the_anchor_does_not_own_is_rejected() {
        let anchor = Arc::new(RecordingAnchor::without(WritePart::History));
        let error = coordinator(&anchor)
            .commit(vec![write("urn:ngsi-ld:A")])
            .await
            .expect_err("a non-native part must be refused");
        assert!(error.to_string().contains("History"), "{error}");
        assert!(anchor.calls().is_empty(), "the transaction must not be opened");
    }

    #[tokio::test]
    async fn only_landed_writes_reach_the_effect_parts() {
        let anchor = Arc::new(RecordingAnchor::new(vec![
            WriteOutcome::Created,
            WriteOutcome::AlreadyExists,
            WriteOutcome::Created,
        ]));
        let outcomes = coordinator(&anchor)
            .commit(vec![write("urn:ngsi-ld:A"), write("urn:ngsi-ld:B"), write("urn:ngsi-ld:C")])
            .await
            .expect("commit");

        pretty_assertions::assert_eq!(outcomes.len(), 3);
        for part in ["history", "outbox", "side_effects"] {
            pretty_assertions::assert_eq!(
                anchor.landed_for(part),
                vec!["urn:ngsi-ld:A".to_string(), "urn:ngsi-ld:C".to_string()],
                "{part} must only see the writes whose entity mutation landed"
            );
        }
    }

    #[tokio::test]
    async fn a_write_with_no_effects_still_reports_its_outcome() {
        let anchor = Arc::new(RecordingAnchor::new(vec![WriteOutcome::Updated]));
        let bare = AtomicWrite::builder()
            .entity_id(Arc::from("urn:ngsi-ld:A"))
            .entity(EntityMutation::Delete)
            .build();

        let outcomes = coordinator(&anchor).commit(vec![bare]).await.expect("commit");

        assert!(outcomes[0].is_updated());
        pretty_assertions::assert_eq!(anchor.landed_for("history"), vec!["urn:ngsi-ld:A".to_string()]);
    }

    #[tokio::test]
    async fn the_effect_parts_run_before_the_commit() {
        let anchor = Arc::new(RecordingAnchor::new(vec![WriteOutcome::Created]));
        coordinator(&anchor).commit(vec![write("urn:ngsi-ld:A")]).await.expect("commit");
        pretty_assertions::assert_eq!(
            anchor.calls(),
            vec![
                "begin".to_string(),
                "entities".to_string(),
                "history".to_string(),
                "outbox".to_string(),
                "side_effects".to_string(),
                "commit".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn an_empty_write_list_commits_nothing() {
        let anchor = Arc::new(RecordingAnchor::new(Vec::new()));
        let outcomes = coordinator(&anchor).commit(Vec::new()).await.expect("commit");
        assert!(outcomes.is_empty());
        assert!(anchor.calls().is_empty());
    }
}
