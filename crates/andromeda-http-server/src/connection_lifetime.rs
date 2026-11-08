//! Serving one accepted connection to its end.
//!
//! A connection ends by itself, or is shut down gracefully for one of two
//! reasons: the listener is draining, or the connection has been idle for the
//! idle bound. A graceful shutdown never cuts a request off: HTTP/1.1 closes
//! once the response in flight is sent, and HTTP/2 sends a GOAWAY and closes
//! once its open streams are done (RFC 9113 section 6.8), so a client never
//! sees a connection vanish under a request it already sent.

use crate::connection_clock::ConnectionClock;
use std::{
    future::{Future, pending},
    pin::{Pin, pin},
    time::Duration,
};
use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

/// Serve `connection` until it ends, shutting it down with
/// `graceful_shutdown` once `draining` changes or is dropped, or once the
/// connection has been idle on `clock` for `idle`.
pub(crate) async fn serve_until_closed<C, E>(
    connection: C,
    graceful_shutdown: fn(Pin<&mut C>),
    clock: ConnectionClock,
    idle: Duration,
    mut draining: watch::Receiver<()>,
) -> Result<(), E>
where
    C: Future<Output = Result<(), E>>,
{
    let mut connection = pin!(connection);
    // The clock is held here as well as by the connection, so its phases
    // keep a sender for as long as they are watched.
    let mut phases = clock.subscribe();
    loop {
        let idle_deadline = phases.borrow_and_update().idle_deadline(idle);
        tokio::select! {
            ended = connection.as_mut() => return ended,
            _ = draining.changed() => break,
            () = until(idle_deadline) => break,
            _ = phases.changed() => {}
        }
    }
    graceful_shutdown(connection.as_mut());
    connection.await
}

/// Resolves at `deadline`, or never without one.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => pending().await,
    }
}

#[cfg(test)]
mod tests {
    use crate::{connection_clock::ConnectionClock, connection_lifetime::serve_until_closed};
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll},
        time::Duration,
    };
    use tokio::{
        sync::watch,
        time::{Instant, advance},
    };

    const IDLE: Duration = Duration::from_secs(180);

    /// A connection that ends as soon as it is shut down gracefully, and
    /// records the shutdown.
    struct Connection {
        /// Whether the connection was shut down.
        shut_down: Arc<AtomicBool>,
    }

    impl Future for Connection {
        type Output = Result<(), ()>;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
            if self.shut_down.load(Ordering::Relaxed) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
    }

    fn shut_down(connection: Pin<&mut Connection>) {
        connection.get_mut().shut_down.store(true, Ordering::Relaxed);
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_connection_is_shut_down_at_the_idle_bound() {
        let clock = ConnectionClock::opened();
        drop(clock.request_began());
        let went_idle = Instant::now();
        let shut = Arc::new(AtomicBool::new(false));
        let (_drain, draining) = watch::channel(());
        let served = serve_until_closed(Connection { shut_down: Arc::clone(&shut) }, shut_down, clock, IDLE, draining);

        assert_eq!(served.await, Ok(()));
        assert!(shut.load(Ordering::Relaxed));
        assert_eq!(went_idle.elapsed(), IDLE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_beginning_while_idle_restarts_the_idle_bound_when_it_ends() {
        let clock = ConnectionClock::opened();
        drop(clock.request_began());
        let (_drain, draining) = watch::channel(());
        let served = tokio::spawn(serve_until_closed(
            Connection {
                shut_down: Arc::new(AtomicBool::new(false)),
            },
            shut_down,
            clock.clone(),
            IDLE,
            draining,
        ));
        advance(IDLE / 2).await;
        let request = clock.request_began();
        advance(IDLE * 2).await;
        assert!(!served.is_finished());
        let ended = Instant::now();
        drop(request);

        assert_eq!(served.await.ok(), Some(Ok(())));
        assert_eq!(ended.elapsed(), IDLE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_opening_is_not_shut_down_for_idleness() {
        let (_drain, draining) = watch::channel(());
        let served = tokio::spawn(serve_until_closed(
            Connection {
                shut_down: Arc::new(AtomicBool::new(false)),
            },
            shut_down,
            ConnectionClock::opened(),
            IDLE,
            draining,
        ));
        advance(IDLE * 2).await;
        assert!(!served.is_finished());
        served.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_draining_listener_shuts_a_busy_connection_down() {
        let clock = ConnectionClock::opened();
        let request = clock.request_began();
        let shut = Arc::new(AtomicBool::new(false));
        let (drain, draining) = watch::channel(());
        let served = tokio::spawn(serve_until_closed(
            Connection { shut_down: Arc::clone(&shut) },
            shut_down,
            clock,
            IDLE,
            draining,
        ));
        drain.send_modify(|()| {});

        assert_eq!(served.await.ok(), Some(Ok(())));
        assert!(shut.load(Ordering::Relaxed));
        drop(request);
    }
}
