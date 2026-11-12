//! A connection's I/O that reports arriving request heads to the connection's
//! clock, and ends the connection when a request head is late.
//!
//! The connection waits for a head from its opening until its first request,
//! and over HTTP/1.1 from the first octet of each later head. While it waits
//! the HTTP server reads, so the read side is where a late head shows: once
//! the head bound has passed, a read ends the stream instead of waiting on.
//! RFC 9110 section 15.5.9 lets the server answer 408 when it gives up
//! waiting for a request, so a late HTTP/1.1 head that has begun to arrive is
//! answered `408 Request Timeout` with `Connection: close` first, written
//! straight to the connection since no response is in progress on it. The
//! answer announces the close, so the connection closes in stages
//! ([`crate::staged_close_stream`]). A connection that received nothing, or
//! only part of the HTTP/2 preface, is closed without an answer: there is no
//! HTTP/1.1 request to answer.
//!
//! The HTTP server answers an HTTP/1.1 head it cannot read, one over the
//! header section limit among them, without serving a request, and shuts the
//! connection down. The client may still be sending the content behind that
//! head, so a shutdown with a head received and no request served announces
//! the close as well (RFC 9112 section 9.6).

use crate::{
    announced_close::AnnouncedClose,
    connection_clock::{ConnectionClock, Phase},
    http2_preface::PrefaceReading,
};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::{Instant, Sleep, sleep_until},
};

/// How far the refusal of a late request head has progressed.
#[derive(Debug)]
enum Refusal {
    /// The `408` answer is being written, `written` octets of it so far.
    Answering {
        /// The whole answer.
        answer: Vec<u8>,
        /// How many octets of it have been written.
        written: usize,
    },
    /// The answer is written and being flushed.
    Flushing,
    /// The stream is shutting down.
    Closing,
    /// The stream is shut down, and every read ends.
    Closed,
}

/// A connection's I/O that reports arriving request heads to the connection's
/// clock, and ends the connection when a request head is late.
pub(crate) struct ClockedStream<Io> {
    /// The connection's own I/O.
    io: Io,
    /// The phase of the connection.
    clock: ConnectionClock,
    /// How long a request head may take to arrive.
    head: Duration,
    /// Whether a response on the connection announced its close.
    announcement: AnnouncedClose,
    /// The protocol the connection's first octets name.
    preface: PrefaceReading,
    /// Whether octets written to the connection wait for a flush. A `408`
    /// written behind them would land inside the response they belong to.
    unflushed: bool,
    /// The timer of the head deadline, set while the connection waits for a
    /// head.
    timer: Option<Pin<Box<Sleep>>>,
    /// The refusal of a late head, once the head bound has passed.
    refusal: Option<Refusal>,
}

impl<Io> ClockedStream<Io> {
    /// Wrap a connection's I/O, giving each request head `head` to arrive and
    /// announcing a refusal through `announcement`.
    pub(crate) const fn new(io: Io, clock: ConnectionClock, head: Duration, announcement: AnnouncedClose) -> ClockedStream<Io> {
        ClockedStream {
            io,
            clock,
            head,
            announcement,
            preface: PrefaceReading::NOTHING_RECEIVED,
            unflushed: false,
            timer: None,
            refusal: None,
        }
    }

    /// Record `octets` arriving on the connection: they name its protocol, and
    /// over HTTP/1.1 they begin a head when the connection was idle.
    fn received(&mut self, octets: &[u8]) {
        if octets.is_empty() {
            return;
        }
        self.preface = self.preface.after(octets);
        match self.preface {
            PrefaceReading::Http1 => self.clock.head_began(),
            PrefaceReading::Http2 | PrefaceReading::Undecided { .. } => {}
        }
    }

    /// The refusal of the head now late: a `408` answer when part of an
    /// HTTP/1.1 head arrived and no response waits for a flush, else the close
    /// alone.
    fn late_head_refusal(&self) -> Refusal {
        match (self.preface, self.unflushed) {
            (PrefaceReading::Http1, false) => Refusal::Answering {
                answer: request_timeout_answer(SystemTime::now()),
                written: 0,
            },
            (PrefaceReading::Http1, true) | (PrefaceReading::Http2 | PrefaceReading::Undecided { .. }, _) => Refusal::Closing,
        }
    }
}

/// The `408 Request Timeout` answer to a late head, dated `now` (RFC 9110
/// section 6.6.1), announcing the close and carrying no content.
fn request_timeout_answer(now: SystemTime) -> Vec<u8> {
    format!(
        "HTTP/1.1 408 Request Timeout\r\ndate: {}\r\nconnection: close\r\ncontent-length: 0\r\n\r\n",
        httpdate::fmt_http_date(now)
    )
    .into_bytes()
}

impl<Io> ClockedStream<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    /// Read from the connection, recording what arrives.
    fn poll_receive(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        ready!(Pin::new(&mut self.io).poll_read(cx, buf))?;
        let octets = buf.filled().get(before..).unwrap_or_default();
        self.received(octets);
        Poll::Ready(Ok(()))
    }

    /// Whether `deadline` has passed; while it has not, the timer wakes the
    /// task when it does.
    fn poll_deadline(&mut self, cx: &mut Context<'_>, deadline: Instant) -> Poll<()> {
        match &mut self.timer {
            Some(timer) if timer.deadline() == deadline => timer.as_mut().poll(cx),
            Some(timer) => {
                timer.as_mut().reset(deadline);
                timer.as_mut().poll(cx)
            }
            None => self.timer.insert(Box::pin(sleep_until(deadline))).as_mut().poll(cx),
        }
    }

    /// Carry the refusal of a late head forward; ready once the stream is shut
    /// down, when every read ends.
    fn poll_refuse(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            match &mut self.refusal {
                Some(Refusal::Answering { answer, written }) => {
                    let remaining = answer.get(*written..).unwrap_or_default();
                    let sent = ready!(Pin::new(&mut self.io).poll_write(cx, remaining))?;
                    if sent == 0 {
                        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                    }
                    *written = written.saturating_add(sent);
                    if *written >= answer.len() {
                        self.announcement.announce();
                        self.refusal = Some(Refusal::Flushing);
                    }
                }
                Some(Refusal::Flushing) => {
                    ready!(Pin::new(&mut self.io).poll_flush(cx))?;
                    self.refusal = Some(Refusal::Closing);
                }
                Some(Refusal::Closing) => {
                    ready!(Pin::new(&mut self.io).poll_shutdown(cx))?;
                    self.refusal = Some(Refusal::Closed);
                }
                Some(Refusal::Closed) | None => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl<Io> AsyncRead for ClockedStream<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        if stream.refusal.is_none() {
            let Some(deadline) = stream.clock.phase().head_deadline(stream.head) else {
                return stream.poll_receive(cx, buf);
            };
            // Checked before reading, so a head trickling in an octet at a
            // time cannot keep the read ready past its deadline.
            if Instant::now() < deadline {
                match stream.poll_receive(cx, buf) {
                    Poll::Pending => ready!(stream.poll_deadline(cx, deadline)),
                    received @ Poll::Ready(_) => return received,
                }
            }
            stream.refusal = Some(stream.late_head_refusal());
        }
        stream.poll_refuse(cx)
    }
}

impl<Io> AsyncWrite for ClockedStream<Io>
where
    Io: AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let stream = self.get_mut();
        let written = ready!(Pin::new(&mut stream.io).poll_write(cx, buf))?;
        stream.unflushed |= written > 0;
        Poll::Ready(Ok(written))
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[io::IoSlice<'_>]) -> Poll<io::Result<usize>> {
        let stream = self.get_mut();
        let written = ready!(Pin::new(&mut stream.io).poll_write_vectored(cx, bufs))?;
        stream.unflushed |= written > 0;
        Poll::Ready(Ok(written))
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        ready!(Pin::new(&mut stream.io).poll_flush(cx))?;
        stream.unflushed = false;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        if stream.head_left_unserved() {
            stream.announcement.announce();
        }
        Pin::new(&mut stream.io).poll_shutdown(cx)
    }
}

impl<Io> ClockedStream<Io> {
    /// Whether an HTTP/1.1 head arrived that no request was served for: the
    /// HTTP server answers a head it cannot read (400, 414 or 431) itself and
    /// shuts the connection down, while the client may still be sending what
    /// follows the head.
    fn head_left_unserved(&self) -> bool {
        matches!(self.preface, PrefaceReading::Http1) && matches!(self.clock.phase(), Phase::Opening { .. } | Phase::Receiving { .. })
    }
}

#[cfg(test)]
mod tests;
