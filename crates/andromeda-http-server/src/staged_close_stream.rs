//! A connection's I/O that closes in stages once a response announced the close.
//!
//! RFC 9112 clause 9.6: "If a server performs an immediate close of a TCP
//! connection, there is a significant risk that the client will not be able to
//! read the last HTTP response." That is what happens when the broker refuses
//! an upload without reading it: the client is still sending, the unread data
//! makes the server's TCP stack answer with a reset, and an HTTP/1.1 client
//! busy writing reports the reset instead of the refusal it already received.
//!
//! Once a response on the connection announced `Connection: close`, shutting
//! the stream down closes only its write side, which delivers the response and
//! a FIN, then waits the configured delay before the connection is closed. The
//! client reads the response in that time and stops sending. The delay reads
//! nothing, so the refused content is never received into the broker.

use crate::announced_close::AnnouncedClose;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::{Sleep, sleep},
};

/// How far a stream's shutdown has progressed.
enum ShutdownStage {
    /// The write side is still open.
    Open,
    /// The write side is closed and the announced close is being delayed.
    Delaying(Pin<Box<Sleep>>),
    /// The shutdown is complete; the connection closes when the stream is dropped.
    Complete,
}

/// A connection's I/O that closes in stages once a response announced the close.
pub(crate) struct StagedCloseStream<Io> {
    /// The accepted connection's own I/O.
    io: Io,
    /// Whether a response on this connection announced `Connection: close`.
    announcement: AnnouncedClose,
    /// How long an announced close waits between the half-close and the close.
    delay: Duration,
    /// How far the shutdown has progressed.
    stage: ShutdownStage,
}

impl<Io> StagedCloseStream<Io> {
    /// Wrap an accepted connection's I/O, delaying an announced close by `delay`.
    pub(crate) fn new(io: Io, delay: Duration) -> StagedCloseStream<Io> {
        StagedCloseStream {
            io,
            announcement: AnnouncedClose::default(),
            delay,
            stage: ShutdownStage::Open,
        }
    }

    /// The record of whether a response on this connection announced the close.
    pub(crate) const fn announcement(&self) -> &AnnouncedClose {
        &self.announcement
    }
}

impl<Io> AsyncRead for StagedCloseStream<Io>
where
    Io: AsyncRead + Unpin,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}

impl<Io> AsyncWrite for StagedCloseStream<Io>
where
    Io: AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write(cx, buf)
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[io::IoSlice<'_>]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        loop {
            match &mut stream.stage {
                ShutdownStage::Open => {
                    ready!(Pin::new(&mut stream.io).poll_shutdown(cx))?;
                    stream.stage = if stream.announcement.is_announced() {
                        ShutdownStage::Delaying(Box::pin(sleep(stream.delay)))
                    } else {
                        ShutdownStage::Complete
                    };
                }
                ShutdownStage::Delaying(delay) => {
                    ready!(delay.as_mut().poll(cx));
                    stream.stage = ShutdownStage::Complete;
                }
                ShutdownStage::Complete => return Poll::Ready(Ok(())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::staged_close_stream::StagedCloseStream;
    use std::{io, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt, duplex},
        time::Instant,
    };

    /// The delay every fixture stream applies to an announced close.
    const DELAY: Duration = Duration::from_millis(500);

    #[tokio::test(start_paused = true)]
    async fn an_unannounced_shutdown_completes_at_once() -> io::Result<()> {
        let (mut peer, io) = duplex(64);
        let mut stream = StagedCloseStream::new(io, DELAY);
        let started = Instant::now();

        stream.shutdown().await?;

        pretty_assertions::assert_eq!(started.elapsed(), Duration::ZERO);
        pretty_assertions::assert_eq!(peer.read(&mut [0; 8]).await?, 0);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn an_announced_shutdown_completes_after_the_delay() -> io::Result<()> {
        let (_peer, io) = duplex(64);
        let mut stream = StagedCloseStream::new(io, DELAY);
        stream.announcement().announce();
        let started = Instant::now();

        stream.shutdown().await?;

        pretty_assertions::assert_eq!(started.elapsed(), DELAY);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn the_peer_reads_the_response_and_the_end_before_the_delay_is_over() -> io::Result<()> {
        let (mut peer, io) = duplex(64);
        let mut stream = StagedCloseStream::new(io, DELAY);
        stream.announcement().announce();
        let started = Instant::now();
        let shutdown = tokio::spawn(async move {
            stream.write_all(b"HTTP/1.1 413").await?;
            stream.shutdown().await
        });

        let mut received = Vec::new();
        peer.read_to_end(&mut received).await?;

        pretty_assertions::assert_eq!(received, b"HTTP/1.1 413".to_vec());
        assert!(started.elapsed() < DELAY, "the end of the response waited {:?}", started.elapsed());
        shutdown.await??;
        pretty_assertions::assert_eq!(started.elapsed(), DELAY);
        Ok(())
    }

    #[tokio::test]
    async fn reads_and_writes_pass_through_unchanged() -> io::Result<()> {
        let (mut peer, io) = duplex(64);
        let mut stream = StagedCloseStream::new(io, DELAY);

        stream.write_all(b"response").await?;
        peer.write_all(b"request").await?;
        let mut from_stream = [0; 8];
        peer.read_exact(&mut from_stream).await?;
        let mut from_peer = [0; 7];
        stream.read_exact(&mut from_peer).await?;

        pretty_assertions::assert_eq!(&from_stream, b"response");
        pretty_assertions::assert_eq!(&from_peer, b"request");
        Ok(())
    }
}
