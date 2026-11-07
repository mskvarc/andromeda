//! A response body written in fixed-size chunks.
//!
//! A typed entity response is serialised straight into the body. Writing it
//! into one growing buffer would hold, at the moment the buffer doubles, the
//! old and the new copy of everything written so far; chunks of a fixed size
//! hold the body once and one partly filled chunk beside it, whatever the size
//! of the entity. The chunks go to the wire as they are, with the body's exact
//! length announced.

use axum::body::{Body, Bytes, HttpBody};
use http_body::{Frame, SizeHint};
use std::{
    collections::VecDeque,
    convert::Infallible,
    io::{Result as IoResult, Write},
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

/// The size of one chunk of a written body.
const CHUNK_SIZE: usize = 64 * 1024;

/// A body being written, chunk by chunk.
#[derive(Debug)]
pub(crate) struct WrittenBody {
    /// The chunks already filled.
    filled: VecDeque<Bytes>,
    /// The chunk being filled.
    current: Vec<u8>,
}

impl WrittenBody {
    /// An empty body.
    pub(crate) fn new() -> WrittenBody {
        WrittenBody {
            filled: VecDeque::new(),
            current: Vec::with_capacity(CHUNK_SIZE),
        }
    }

    /// The bytes written so far.
    pub(crate) fn len(&self) -> usize {
        self.filled.iter().map(Bytes::len).sum::<usize>() + self.current.len()
    }

    /// The body, ready to be sent.
    pub(crate) fn into_body(self) -> Body {
        let WrittenBody { mut filled, current } = self;
        if !current.is_empty() {
            filled.push_back(Bytes::from(current));
        }
        let length = filled.iter().map(Bytes::len).sum::<usize>();
        Body::new(WrittenChunks {
            chunks: filled,
            remaining: u64::try_from(length).ok(),
        })
    }
}

impl Write for WrittenBody {
    fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
        let mut rest = buffer;
        while !rest.is_empty() {
            if self.current.len() == CHUNK_SIZE {
                let full = replace(&mut self.current, Vec::with_capacity(CHUNK_SIZE));
                self.filled.push_back(Bytes::from(full));
            }
            let (taken, left) = rest.split_at(rest.len().min(CHUNK_SIZE - self.current.len()));
            self.current.extend_from_slice(taken);
            rest = left;
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

/// The filled chunks of a written body, sent in order.
#[derive(Debug)]
struct WrittenChunks {
    /// The chunks not yet sent.
    chunks: VecDeque<Bytes>,
    /// The bytes not yet sent, when the length is representable.
    remaining: Option<u64>,
}

impl HttpBody for WrittenChunks {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let chunks = self.get_mut();
        let next = chunks.chunks.pop_front();
        if let Some(chunk) = &next {
            chunks.remaining = chunks
                .remaining
                .zip(u64::try_from(chunk.len()).ok())
                .map(|(remaining, sent)| remaining.saturating_sub(sent));
        }
        Poll::Ready(next.map(|chunk| Ok(Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.chunks.is_empty()
    }

    fn size_hint(&self) -> SizeHint {
        self.remaining.map_or_else(SizeHint::default, SizeHint::with_exact)
    }
}

#[cfg(test)]
mod tests {
    use crate::written_body::{CHUNK_SIZE, WrittenBody};
    use axum::body::{HttpBody, to_bytes};
    use std::io::Write;

    /// A body written in many pieces across several chunks reads back as
    /// what was written, and announces its exact length.
    #[tokio::test]
    async fn a_body_spanning_several_chunks_reads_back_whole_with_its_length() {
        let written: Vec<u8> = (0..(CHUNK_SIZE * 2 + 17)).map(|index| u8::try_from(index % 251).expect("below 251")).collect();
        let mut body = WrittenBody::new();
        for piece in written.chunks(1000) {
            body.write_all(piece).expect("an in-memory body accepts every write");
        }
        let body = body.into_body();
        pretty_assertions::assert_eq!(body.size_hint().exact(), u64::try_from(written.len()).ok());
        let read = to_bytes(body, usize::MAX).await.expect("the body reads");
        pretty_assertions::assert_eq!(read.as_ref(), written.as_slice());
    }

    /// The length counts the filled chunks and the one being filled.
    #[test]
    fn the_length_of_a_body_spanning_several_chunks_is_what_was_written() {
        let mut body = WrittenBody::new();
        body.write_all(&vec![7_u8; CHUNK_SIZE * 2 + 5]).expect("an in-memory body accepts every write");
        pretty_assertions::assert_eq!(body.len(), CHUNK_SIZE * 2 + 5);
    }

    /// An empty body has nothing to send and says so.
    #[tokio::test]
    async fn an_empty_body_sends_nothing() {
        let body = WrittenBody::new().into_body();
        pretty_assertions::assert_eq!(body.size_hint().exact(), Some(0));
        let read = to_bytes(body, usize::MAX).await.expect("the body reads");
        assert!(read.is_empty());
    }
}
