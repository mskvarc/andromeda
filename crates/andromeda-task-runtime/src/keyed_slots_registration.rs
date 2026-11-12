//! Registration of keyed slots whose work a supervisor stops at shutdown.

use crate::{
    failure::TaskFailure,
    keyed_slots::KeyedSlots,
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    supervisor::TaskSupervisor,
};
use std::{convert::Infallible, hash::Hash, num::NonZeroUsize};

impl TaskSupervisor {
    /// Create `width` keyed slots, and register under `registration` the
    /// resident task that stops their work and waits for every item to end
    /// once this supervisor's scope is cancelled.
    ///
    /// The slots stop on a child of the scope's cancellation, so finishing them
    /// cancels none of the scope's other tasks.
    ///
    /// # Errors
    ///
    /// Returns [`TaskRegistrationError`] when the task name is already reserved
    /// by this supervisor.
    pub fn register_keyed_slots<K: Eq + Hash + Clone + Send + Sync + 'static>(
        &self,
        registration: TaskRegistration,
        width: NonZeroUsize,
    ) -> Result<KeyedSlots<K>, TaskRegistrationError> {
        let slots = KeyedSlots::new(width, self.inner.cancellation.child_token());
        let owned = slots.clone();
        let task = ResidentTask::new(registration, move |execution| {
            let slots = owned.clone();
            async move {
                execution.mark_running();
                execution.cancellation().cancelled().await;
                slots.finish().await;
                Ok::<(), TaskFailure<Infallible>>(())
            }
        });
        self.register_resident(task)?;
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        criticality::TaskCriticality,
        keyed_slots::{Admission, KeyedSlots},
        registration::TaskRegistration,
        restart_policy::RestartPolicy,
        shutdown_policy::ShutdownPolicy,
        supervisor::TaskSupervisor,
        task_health::TaskHealthState,
        task_name::TaskName,
    };
    use std::{future, num::NonZeroUsize, time::Duration};
    use tokio::{runtime::Handle, sync::oneshot, time::timeout};

    fn registration(name: &str) -> TaskRegistration {
        TaskRegistration::new(
            TaskName::try_from(name).expect("a valid task name"),
            TaskCriticality::Observed,
            RestartPolicy::Never,
            ShutdownPolicy::new(Duration::from_secs(1)),
        )
    }

    fn width() -> NonZeroUsize {
        NonZeroUsize::new(2).expect("a positive width")
    }

    #[tokio::test]
    async fn shutting_the_scope_down_stops_the_work_in_its_slots_and_ends_their_task() {
        let supervisor = TaskSupervisor::process(Handle::current());
        let slots: KeyedSlots<u32> = supervisor.register_keyed_slots(registration("slots"), width()).expect("the slots register");
        let (held, dropped) = oneshot::channel::<()>();
        let admission = slots.offer(
            1,
            async move {
                let _held = held;
                future::pending::<()>().await;
            },
            || {},
        );
        pretty_assertions::assert_eq!(admission, Admission::Started);

        timeout(Duration::from_secs(5), supervisor.shutdown())
            .await
            .expect("the scope stops within its grace");

        let ended = timeout(Duration::from_secs(5), dropped).await.expect("the work ends");
        assert!(ended.is_err(), "the work was stopped rather than run out");
        assert!(!slots.is_in_flight(&1), "stopped work frees its key");
        let name = TaskName::try_from("slots").expect("a valid task name");
        let health = supervisor.health(&name).await.expect("the slots' task is registered");
        pretty_assertions::assert_eq!(health.state, TaskHealthState::Cancelled);
    }

    #[tokio::test]
    async fn finishing_the_slots_cancels_none_of_the_scopes_other_tasks() {
        let supervisor = TaskSupervisor::process(Handle::current());
        let slots: KeyedSlots<u32> = supervisor.register_keyed_slots(registration("slots"), width()).expect("the slots register");

        timeout(Duration::from_secs(5), slots.finish()).await.expect("the slots finish");

        assert!(!supervisor.cancellation_token().is_cancelled(), "the scope keeps running");
    }

    #[tokio::test]
    async fn a_second_registration_under_the_same_name_is_refused() {
        let supervisor = TaskSupervisor::process(Handle::current());
        let first: Result<KeyedSlots<u32>, _> = supervisor.register_keyed_slots(registration("slots"), width());
        assert!(first.is_ok(), "{first:?}");

        let second: Result<KeyedSlots<u32>, _> = supervisor.register_keyed_slots(registration("slots"), width());

        assert!(second.is_err(), "{second:?}");
    }
}
