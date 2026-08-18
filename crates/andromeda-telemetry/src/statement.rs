//! Latency and failure accounting for individual database statements.

use crate::names::{DB_STATEMENT_FAILURES_TOTAL, DB_STATEMENT_SECONDS, LABEL_STATEMENT};
use metrics::{counter, histogram};
use std::time::Instant;

/// The stable name one hot statement is reported under.
///
/// The name is the `statement` label value; it identifies the statement
/// family (`entity-insert`, `cav-upsert`), never one rendered SQL text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatementName(pub &'static str);

impl StatementName {
    /// The label value this name renders as.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Await `statement`, recording its wall-clock duration under `name` and
/// counting a failure when it resolves to `Err`.
pub async fn observe_statement<T, E, F>(name: StatementName, statement: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
{
    let started = Instant::now();
    let outcome = statement.await;
    histogram!(DB_STATEMENT_SECONDS, LABEL_STATEMENT => name.as_str()).record(started.elapsed());
    if outcome.is_err() {
        counter!(DB_STATEMENT_FAILURES_TOTAL, LABEL_STATEMENT => name.as_str()).increment(1);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use crate::{
        names::{DB_STATEMENT_FAILURES_TOTAL, DB_STATEMENT_SECONDS},
        recorder::TelemetryRecorder,
        statement::{StatementName, observe_statement},
    };
    use std::error::Error;
    use tokio::runtime::Builder;

    /// Drive `statement` to completion inside the recorder's local scope.
    ///
    /// The observation is recorded after the awaited statement resolves, so the
    /// whole future must be polled while the local recorder is active.
    fn observe_locally<T: 'static, E: 'static>(
        recorder: &TelemetryRecorder,
        name: StatementName,
        statement: impl Future<Output = Result<T, E>>,
    ) -> Result<Result<T, E>, Box<dyn Error>> {
        let runtime = Builder::new_current_thread().build()?;
        Ok(recorder.record_locally(|| runtime.block_on(observe_statement(name, statement))))
    }

    #[test]
    fn a_failing_statement_records_duration_and_one_failure() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        let outcome = observe_locally(&recorder, StatementName("unit-probe"), async { Err::<(), &str>("boom") })?;
        assert!(outcome.is_err());
        let rendered = recorder.render();
        assert!(rendered.contains(DB_STATEMENT_SECONDS), "{rendered}");
        assert!(
            rendered.contains(&format!("{DB_STATEMENT_FAILURES_TOTAL}{{statement=\"unit-probe\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_successful_statement_records_no_failure() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        let outcome = observe_locally(&recorder, StatementName("unit-ok"), async { Ok::<u8, &str>(7) })?;
        pretty_assertions::assert_eq!(outcome, Ok(7));
        let rendered = recorder.render();
        assert!(rendered.contains("statement=\"unit-ok\""), "{rendered}");
        assert!(!rendered.contains(DB_STATEMENT_FAILURES_TOTAL), "{rendered}");
        Ok(())
    }
}
