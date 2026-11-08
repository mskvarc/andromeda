//! Where one accepted connection stands between its requests.
//!
//! Three parties move the connection from phase to phase, and each sees only
//! part of it: the connection's I/O sees octets arrive, each request sees
//! itself begin and its response end, and the task serving the connection
//! closes it once it has been idle too long. The clock is the record all three
//! share, and the task serving the connection is woken whenever it changes.

use std::{num::NonZeroUsize, time::Duration};
use tokio::{sync::watch, time::Instant};

/// Where a connection stands between its requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No request has begun since the connection opened at `since`.
    Opening {
        /// When the connection opened.
        since: Instant,
    },
    /// Requests are in flight, from the start of each until its response has
    /// been sent or abandoned.
    Busy {
        /// How many requests are in flight.
        requests: NonZeroUsize,
    },
    /// No request has been in flight, and no octet of an HTTP/1.1 request
    /// head has arrived, since `since`.
    Idle {
        /// When the last request in flight ended.
        since: Instant,
    },
    /// No request is in flight, and the first octet of an HTTP/1.1 request
    /// head arrived at `since`.
    Receiving {
        /// When the first octet of the head arrived.
        since: Instant,
    },
}

impl Phase {
    /// When the request head the connection waits for must have arrived by,
    /// `head` after the wait began; `None` while it waits for no head.
    pub(crate) fn head_deadline(self, head: Duration) -> Option<Instant> {
        match self {
            Phase::Opening { since } | Phase::Receiving { since } => Some(since + head),
            Phase::Busy { .. } | Phase::Idle { .. } => None,
        }
    }

    /// When the connection is closed for being idle, `idle` after it went
    /// idle; `None` while it is not idle.
    pub(crate) fn idle_deadline(self, idle: Duration) -> Option<Instant> {
        match self {
            Phase::Idle { since } => Some(since + idle),
            Phase::Opening { .. } | Phase::Busy { .. } | Phase::Receiving { .. } => None,
        }
    }
}

/// The phase of one connection, shared by its I/O, its requests and the task
/// serving it. Every clone records into the same phase.
#[derive(Clone, Debug)]
pub(crate) struct ConnectionClock(watch::Sender<Phase>);

impl ConnectionClock {
    /// The clock of a connection that opened just now.
    pub(crate) fn opened() -> ConnectionClock {
        ConnectionClock(watch::Sender::new(Phase::Opening { since: Instant::now() }))
    }

    /// The connection's phase now.
    pub(crate) fn phase(&self) -> Phase {
        *self.0.borrow()
    }

    /// A receiver of the connection's phase, woken at every change.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Phase> {
        self.0.subscribe()
    }

    /// Record that the first octet of an HTTP/1.1 request head arrived. Only an
    /// idle connection starts receiving a head: one still opening already
    /// waits for its first, and the octets a busy connection receives belong
    /// to the requests in flight or to a head pipelined behind them.
    pub(crate) fn head_began(&self) {
        self.0.send_if_modified(|phase| match phase {
            Phase::Idle { .. } => {
                *phase = Phase::Receiving { since: Instant::now() };
                true
            }
            Phase::Opening { .. } | Phase::Busy { .. } | Phase::Receiving { .. } => false,
        });
    }

    /// Record that a request began; the connection stays busy until the
    /// returned record is dropped.
    pub(crate) fn request_began(&self) -> ActiveRequest {
        self.0.send_modify(|phase| {
            *phase = Phase::Busy {
                requests: match phase {
                    Phase::Busy { requests } => requests.saturating_add(1),
                    Phase::Opening { .. } | Phase::Idle { .. } | Phase::Receiving { .. } => NonZeroUsize::MIN,
                },
            };
        });
        ActiveRequest(self.clone())
    }
}

/// A request in flight on a connection, from its start until its response has
/// been sent or abandoned.
#[derive(Debug)]
pub(crate) struct ActiveRequest(ConnectionClock);

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.0.0.send_modify(|phase| {
            if let Phase::Busy { requests } = phase {
                *phase = match NonZeroUsize::new(requests.get().saturating_sub(1)) {
                    Some(requests) => Phase::Busy { requests },
                    None => Phase::Idle { since: Instant::now() },
                };
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::connection_clock::{ConnectionClock, Phase};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroUsize, time::Duration};
    use tokio::time::{Instant, advance};

    const HEAD: Duration = Duration::from_secs(30);
    const IDLE: Duration = Duration::from_secs(180);
    const STEP: Duration = Duration::from_secs(5);

    #[tokio::test(start_paused = true)]
    async fn a_new_connection_waits_for_its_first_head_from_its_opening() {
        let opened = Instant::now();
        let clock = ConnectionClock::opened();
        advance(STEP).await;
        clock.head_began();
        assert_eq!(clock.phase(), Phase::Opening { since: opened });
        assert_eq!(clock.phase().head_deadline(HEAD), Some(opened + HEAD));
        assert_eq!(clock.phase().idle_deadline(IDLE), None);
    }

    #[tokio::test(start_paused = true)]
    async fn the_connection_goes_idle_when_its_last_request_ends() {
        let clock = ConnectionClock::opened();
        let first = clock.request_began();
        let second = clock.request_began();
        assert_eq!(
            clock.phase(),
            Phase::Busy {
                requests: NonZeroUsize::MIN.saturating_add(1)
            }
        );
        drop(first);
        assert_eq!(clock.phase(), Phase::Busy { requests: NonZeroUsize::MIN });
        assert_eq!(clock.phase().head_deadline(HEAD), None);
        assert_eq!(clock.phase().idle_deadline(IDLE), None);
        advance(STEP).await;
        let ended = Instant::now();
        drop(second);
        assert_eq!(clock.phase(), Phase::Idle { since: ended });
        assert_eq!(clock.phase().idle_deadline(IDLE), Some(ended + IDLE));
        assert_eq!(clock.phase().head_deadline(HEAD), None);
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_connection_receiving_a_head_waits_for_it_from_its_first_octet() {
        let clock = ConnectionClock::opened();
        drop(clock.request_began());
        advance(STEP).await;
        let began = Instant::now();
        clock.head_began();
        advance(STEP).await;
        clock.head_began();
        assert_eq!(clock.phase(), Phase::Receiving { since: began });
        assert_eq!(clock.phase().head_deadline(HEAD), Some(began + HEAD));
        assert_eq!(clock.phase().idle_deadline(IDLE), None);
    }

    #[tokio::test(start_paused = true)]
    async fn octets_reaching_a_busy_connection_start_no_head() {
        let clock = ConnectionClock::opened();
        let request = clock.request_began();
        clock.head_began();
        assert_eq!(clock.phase(), Phase::Busy { requests: NonZeroUsize::MIN });
        drop(request);
    }

    #[tokio::test(start_paused = true)]
    async fn every_change_wakes_a_subscriber() {
        let clock = ConnectionClock::opened();
        let mut phases = clock.subscribe();
        drop(clock.request_began());
        assert!(phases.has_changed().is_ok_and(|changed| changed));
        phases.borrow_and_update();
        clock.head_began();
        assert!(phases.has_changed().is_ok_and(|changed| changed));
    }
}
