//! Latency and failure accounting for individual database statements.

use crate::names::{LABEL_STATEMENT, POSTGRESQL_STATEMENT_DURATION_SECONDS, POSTGRESQL_STATEMENT_FAILURES_TOTAL};
use metrics::{counter, histogram};
use std::time::Instant;
use strum::{EnumIter, IntoStaticStr};

/// The statement families observed one by one, each named by the `statement`
/// label value it renders as: what the statement does, never its SQL text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ObservedStatement {
    /// Inserting a created entity.
    EntityInsert,
    /// Updating a stored entity.
    EntityUpdate,
    /// Deleting a stored entity.
    EntityDelete,
    /// Withdrawing the Temporal Evolution an entity creation established.
    EntityEvolutionWithdrawal,
    /// Locking the identity rows of writes that record history and leave
    /// current state as it was.
    EntityHistoryLock,
    /// Claiming queued notification deliveries.
    NotificationDeliveryClaim,
    /// Recording the outcome of claimed notification deliveries: delivered,
    /// failed or exhausted.
    NotificationDeliveryAcknowledge,
    /// The second statement of an acknowledgement: setting deliveries aside,
    /// releasing them or handing their hold over.
    NotificationDeliveryHold,
    /// Claiming queued `respond-async` batch notifications.
    PendingSideEffectClaim,
    /// Releasing a failed `respond-async` job for another attempt, or setting an
    /// unreadable one aside. A replay that succeeds releases nothing: its
    /// commit removes the job.
    PendingSideEffectRelease,
    /// Claiming queued history writes.
    TemporalIntentClaim,
    /// Scheduling a failed history write for another attempt.
    TemporalIntentRetry,
    /// Releasing held-back history writes.
    TemporalIntentRelease,
    /// Asking whether a history write a request waits for has been released.
    TemporalIntentReleaseProbe,
    /// Applying a claimed history write, from its first statement to its
    /// commit.
    TemporalIntentApplication,
    /// Which temporal entities a temporal query selects.
    TemporalCandidateEntities,
    /// How many temporal entities a temporal query selects.
    TemporalCandidateEntityCount,
    /// The history rows of one selected entity.
    TemporalHistory,
    /// The reified Scope events of one selected entity (clause 4.18).
    TemporalScopeHistory,
    /// The aggregated history of one selected entity.
    TemporalAggregation,
    /// Which shapes that rule out aggregation the aggregated instances carry
    /// (clause 5.7.4.4).
    TemporalAggregationShapeCheck,
    /// The truth of every filter leaf of a temporal query over the entities a
    /// Context Source contributed to (clause 5.7.4.4).
    TemporalSourceFilterEvaluation,
    /// The instances a temporal query's filters read in effect when its window
    /// opens, over the entities a Context Source contributed to (clauses 4.11
    /// and 5.7.4.4).
    TemporalSourceWindowStartInstances,
    /// The Scope event in force when a temporal query's window opens, over the
    /// entities a Context Source contributed to (clauses 4.18 and 5.7.4.4).
    TemporalSourceWindowStartScope,
    /// The instances inside a temporal query's window of the Relationships its
    /// hops start from, over the entities a Context Source contributed to
    /// (clauses 4.9 and 5.7.4.4).
    TemporalSourceWindowRelationships,
}

/// Await `statement`, recording its wall-clock duration under `name` and
/// counting a failure when it resolves to `Err`.
///
/// # Errors
///
/// Returns the awaited statement's own error unchanged; observing it never
/// introduces a failure of its own.
pub async fn observe_statement<T, E, F>(name: ObservedStatement, statement: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
{
    let label: &'static str = name.into();
    let started = Instant::now();
    let outcome = statement.await;
    histogram!(POSTGRESQL_STATEMENT_DURATION_SECONDS, LABEL_STATEMENT => label).record(started.elapsed());
    if outcome.is_err() {
        counter!(POSTGRESQL_STATEMENT_FAILURES_TOTAL, LABEL_STATEMENT => label).increment(1);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use crate::{
        names::{POSTGRESQL_STATEMENT_DURATION_SECONDS, POSTGRESQL_STATEMENT_FAILURES_TOTAL},
        recorder::TelemetryRecorder,
        statement::{ObservedStatement, observe_statement},
    };
    use std::error::Error;
    use strum::IntoEnumIterator;
    use tokio::runtime::Builder;

    /// Drive `statement` to completion inside the recorder's local scope.
    ///
    /// The observation is recorded after the awaited statement resolves, so the
    /// whole future must be polled while the local recorder is active.
    fn observe_locally<T: 'static, E: 'static>(
        recorder: &TelemetryRecorder,
        name: ObservedStatement,
        statement: impl Future<Output = Result<T, E>>,
    ) -> Result<Result<T, E>, Box<dyn Error>> {
        let runtime = Builder::new_current_thread().build()?;
        Ok(recorder.record_locally(|| runtime.block_on(observe_statement(name, statement))))
    }

    #[test]
    fn a_failing_statement_records_duration_and_one_failure() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        let outcome = observe_locally(&recorder, ObservedStatement::EntityDelete, async { Err::<(), &str>("boom") })?;
        assert!(outcome.is_err());
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{POSTGRESQL_STATEMENT_DURATION_SECONDS}_count{{statement=\"entity-delete\"}} 1")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{POSTGRESQL_STATEMENT_FAILURES_TOTAL}{{statement=\"entity-delete\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_successful_statement_records_no_failure() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        let outcome = observe_locally(&recorder, ObservedStatement::EntityInsert, async { Ok::<u8, &str>(7) })?;
        pretty_assertions::assert_eq!(outcome, Ok(7));
        let rendered = recorder.render();
        assert!(rendered.contains("statement=\"entity-insert\""), "{rendered}");
        assert!(!rendered.contains(&format!("{POSTGRESQL_STATEMENT_FAILURES_TOTAL}{{")), "{rendered}");
        Ok(())
    }

    #[test]
    fn statement_label_values_are_the_documented_kebab_case_names() {
        let labels: Vec<&'static str> = ObservedStatement::iter().map(<&'static str>::from).collect();
        pretty_assertions::assert_eq!(
            labels,
            [
                "entity-insert",
                "entity-update",
                "entity-delete",
                "entity-evolution-withdrawal",
                "entity-history-lock",
                "notification-delivery-claim",
                "notification-delivery-acknowledge",
                "notification-delivery-hold",
                "pending-side-effect-claim",
                "pending-side-effect-release",
                "temporal-intent-claim",
                "temporal-intent-retry",
                "temporal-intent-release",
                "temporal-intent-release-probe",
                "temporal-intent-application",
                "temporal-candidate-entities",
                "temporal-candidate-entity-count",
                "temporal-history",
                "temporal-scope-history",
                "temporal-aggregation",
                "temporal-aggregation-shape-check",
                "temporal-source-filter-evaluation",
                "temporal-source-window-start-instances",
                "temporal-source-window-start-scope",
                "temporal-source-window-relationships",
            ]
        );
    }
}
