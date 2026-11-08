//! Whether a request in flight is held up by its own client.
//!
//! A request past its deadline is either the broker's failure or the client's:
//! a body the client has not finished sending keeps the handler waiting on the
//! client, which RFC 9110 clause 15.5.9 answers with 408, not the broker's 500.
//! The request body is watched as the stack reads it, so the deadline can tell
//! the two apart when it passes.

use axum::{
    body::{Body, Bytes, HttpBody},
    extract::Request,
};
use http_body::{Frame, SizeHint};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

/// Who a request still in flight is waiting on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeldUpBy {
    /// The stack's last read of the body found nothing more from the client.
    Client,
    /// The body was read to its end, or the stack is not reading it.
    Broker,
}

/// The watch on one request's body arrival.
#[derive(Debug)]
pub(crate) struct BodyArrival(Option<Arc<AtomicBool>>);

impl BodyArrival {
    /// `request` with its body watched, and the watch.
    ///
    /// A body already at its end, as a request without one is, is left
    /// unwrapped: nothing of it can keep the stack waiting.
    pub(crate) fn watch(request: Request) -> (Request, BodyArrival) {
        if request.body().is_end_stream() {
            return (request, BodyArrival(None));
        }
        let awaiting_client = Arc::new(AtomicBool::new(false));
        let watched = request.map(|inner| {
            Body::new(WatchedBody {
                inner,
                awaiting_client: Arc::clone(&awaiting_client),
            })
        });
        (watched, BodyArrival(Some(awaiting_client)))
    }

    /// Who the request is waiting on now.
    pub(crate) fn held_up_by(&self) -> HeldUpBy {
        match &self.0 {
            Some(awaiting_client) if awaiting_client.load(Ordering::Acquire) => HeldUpBy::Client,
            Some(_) | None => HeldUpBy::Broker,
        }
    }
}

/// A request body that records whether its last read waited on the client.
struct WatchedBody {
    /// The body as the connection delivers it.
    inner: Body,
    /// Raised while the last read of `inner` is pending.
    awaiting_client: Arc<AtomicBool>,
}

impl HttpBody for WatchedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let body = self.get_mut();
        let polled = Pin::new(&mut body.inner).poll_frame(cx);
        body.awaiting_client.store(polled.is_pending(), Ordering::Release);
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use crate::stack::body_arrival::{BodyArrival, HeldUpBy};
    use axum::{
        body::{Body, Bytes, HttpBody, to_bytes},
        extract::Request,
    };
    use futures::stream::{StreamExt, iter, pending};
    use std::{convert::Infallible, error::Error, future::poll_fn, pin::Pin, task::Poll};

    #[test]
    fn a_request_without_a_body_waits_on_the_broker() -> Result<(), Box<dyn Error>> {
        let (_request, arrival) = BodyArrival::watch(Request::get("/probe").body(Body::empty())?);
        pretty_assertions::assert_eq!(arrival.held_up_by(), HeldUpBy::Broker);
        Ok(())
    }

    #[tokio::test]
    async fn a_body_read_to_its_end_waits_on_the_broker() -> Result<(), Box<dyn Error>> {
        let chunks = iter([Ok::<_, Infallible>(Bytes::from_static(b"{}"))]);
        let (request, arrival) = BodyArrival::watch(Request::post("/probe").body(Body::from_stream(chunks))?);
        pretty_assertions::assert_eq!(to_bytes(request.into_body(), usize::MAX).await?, Bytes::from_static(b"{}"));
        pretty_assertions::assert_eq!(arrival.held_up_by(), HeldUpBy::Broker);
        Ok(())
    }

    #[tokio::test]
    async fn a_body_whose_client_stopped_sending_waits_on_the_client() -> Result<(), Box<dyn Error>> {
        let chunks = iter([Ok::<_, Infallible>(Bytes::from_static(b"{"))]).chain(pending());
        let (request, arrival) = BodyArrival::watch(Request::post("/probe").body(Body::from_stream(chunks))?);
        let mut body = request.into_body();
        let first = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
        assert!(first.is_some(), "the first chunk arrives");
        pretty_assertions::assert_eq!(arrival.held_up_by(), HeldUpBy::Broker);
        let second = poll_fn(|cx| Poll::Ready(Pin::new(&mut body).poll_frame(cx))).await;
        assert!(second.is_pending(), "the client sends nothing more");
        pretty_assertions::assert_eq!(arrival.held_up_by(), HeldUpBy::Client);
        Ok(())
    }

    #[test]
    fn a_body_the_stack_never_reads_waits_on_the_broker() -> Result<(), Box<dyn Error>> {
        let (_request, arrival) = BodyArrival::watch(Request::post("/probe").body(Body::from_stream(pending::<Result<Bytes, Infallible>>()))?);
        pretty_assertions::assert_eq!(arrival.held_up_by(), HeldUpBy::Broker);
        Ok(())
    }
}
