//! A listener whose connections close in stages once a response announced the close.

use crate::staged_close_stream::StagedCloseStream;
use axum::serve::Listener;
use std::{io, time::Duration};

/// A listener whose connections close in stages once a response announced the
/// close (see [`crate::staged_close_stream`]).
pub(crate) struct StagedCloseListener<L> {
    /// The listener accepting the connections.
    inner: L,
    /// How long an announced close waits between the half-close and the close.
    delay: Duration,
}

impl<L> StagedCloseListener<L> {
    /// Wrap `inner`, delaying every announced close on its connections by `delay`.
    pub(crate) const fn new(inner: L, delay: Duration) -> StagedCloseListener<L> {
        StagedCloseListener { inner, delay }
    }
}

impl<L> Listener for StagedCloseListener<L>
where
    L: Listener,
{
    type Io = StagedCloseStream<L::Io>;
    type Addr = L::Addr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (io, address) = self.inner.accept().await;
        (StagedCloseStream::new(io, self.delay), address)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}
