use crate::{announced_close::AnnouncedClose, clocked_stream::ClockedStream, connection_clock::ConnectionClock};
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex, split},
    task::JoinHandle,
    time::{Instant, advance, sleep, timeout},
};

/// The head bound every fixture stream applies.
const HEAD: Duration = Duration::from_secs(30);

/// A clocked stream over one end of a pipe, the other end, and the stream's
/// clock and close announcement.
fn fixture() -> (ClockedStream<DuplexStream>, DuplexStream, ConnectionClock, AnnouncedClose) {
    let (peer, io) = duplex(4096);
    let clock = ConnectionClock::opened();
    let announcement = AnnouncedClose::default();
    (ClockedStream::new(io, clock.clone(), HEAD, announcement.clone()), peer, clock, announcement)
}

/// Read `stream` to its end in a task of its own, as the HTTP server reads a
/// connection waiting for a head, and report what it read.
fn read_to_end(mut stream: ClockedStream<DuplexStream>) -> JoinHandle<io::Result<Vec<u8>>> {
    tokio::spawn(async move {
        let mut read = Vec::new();
        stream.read_to_end(&mut read).await?;
        Ok(read)
    })
}

/// Everything the peer receives until the stream's end.
async fn received(peer: &mut DuplexStream) -> io::Result<String> {
    let mut answer = Vec::new();
    peer.read_to_end(&mut answer).await?;
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

#[tokio::test(start_paused = true)]
async fn a_partial_http1_head_late_on_a_new_connection_is_answered_408_and_the_close_is_announced() -> io::Result<()> {
    let (stream, mut peer, _clock, announcement) = fixture();
    let opened = Instant::now();
    let reader = read_to_end(stream);
    peer.write_all(b"GET /ngsi-ld/v1/types HTTP/1.1\r\nHost: localhost\r\n").await?;

    let answer = received(&mut peer).await?;

    assert!(answer.starts_with("HTTP/1.1 408 Request Timeout\r\n"), "{answer}");
    assert!(answer.contains("\r\nconnection: close\r\n"), "{answer}");
    assert!(answer.contains("\r\ndate: "), "{answer}");
    assert!(answer.ends_with("\r\ncontent-length: 0\r\n\r\n"), "{answer}");
    pretty_assertions::assert_eq!(opened.elapsed(), HEAD);
    assert!(announcement.is_announced());
    pretty_assertions::assert_eq!(reader.await??, b"GET /ngsi-ld/v1/types HTTP/1.1\r\nHost: localhost\r\n".to_vec());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_new_connection_that_sends_nothing_ends_at_the_head_bound_without_an_answer() -> io::Result<()> {
    let (stream, mut peer, _clock, announcement) = fixture();
    let opened = Instant::now();
    let reader = read_to_end(stream);

    pretty_assertions::assert_eq!(received(&mut peer).await?, "");
    pretty_assertions::assert_eq!(opened.elapsed(), HEAD);
    assert!(!announcement.is_announced());
    pretty_assertions::assert_eq!(reader.await??, Vec::<u8>::new());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_partial_http2_preface_ends_at_the_head_bound_without_an_answer() -> io::Result<()> {
    let (stream, mut peer, _clock, _announcement) = fixture();
    let reader = read_to_end(stream);
    peer.write_all(b"PRI * HTTP/2.0\r\n").await?;

    pretty_assertions::assert_eq!(received(&mut peer).await?, "");
    pretty_assertions::assert_eq!(reader.await??, b"PRI * HTTP/2.0\r\n".to_vec());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_head_trickling_in_an_octet_a_second_is_refused_at_its_deadline() -> io::Result<()> {
    let (stream, peer, _clock, _announcement) = fixture();
    let opened = Instant::now();
    let reader = read_to_end(stream);
    let (mut from_broker, mut to_broker) = split(peer);
    let trickle = tokio::spawn(async move {
        for octet in b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Fill: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" {
            if to_broker.write_all(&[*octet]).await.is_err() {
                break;
            }
            sleep(Duration::from_secs(1)).await;
        }
    });

    let mut answer = Vec::new();
    from_broker.read_to_end(&mut answer).await?;

    assert!(answer.starts_with(b"HTTP/1.1 408 "), "{}", String::from_utf8_lossy(&answer));
    assert!(opened.elapsed() <= HEAD + Duration::from_secs(1), "refused after {:?}", opened.elapsed());
    reader.await??;
    trickle.abort();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_connection_with_a_request_in_flight_waits_past_the_head_bound() -> io::Result<()> {
    let (mut stream, _peer, clock, _announcement) = fixture();
    let request = clock.request_began();

    let read = timeout(HEAD * 2, stream.read(&mut [0; 16])).await;

    assert!(read.is_err(), "the read ended with {read:?}");
    drop(request);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_idle_connection_gets_the_head_bound_from_the_first_octet_of_its_next_head() -> io::Result<()> {
    let (stream, mut peer, clock, _announcement) = fixture();
    drop(clock.request_began());
    let reader = read_to_end(stream);
    advance(HEAD * 3).await;
    peer.write_all(b"GE").await?;
    let began = Instant::now();

    let answer = received(&mut peer).await?;

    assert!(answer.starts_with("HTTP/1.1 408 "), "{answer}");
    pretty_assertions::assert_eq!(began.elapsed(), HEAD);
    reader.await??;
    Ok(())
}

/// Send `octets` to `stream`, read them through it, and shut it down, as the
/// HTTP server does once it has answered what it read, and report whether the
/// close was announced.
async fn announced_after(octets: &[u8], served: Served) -> io::Result<bool> {
    let (mut stream, mut peer, clock, announcement) = fixture();
    peer.write_all(octets).await?;
    let mut read = vec![0; octets.len()];
    stream.read_exact(&mut read).await?;
    if let Served::Request = served {
        drop(clock.request_began());
    }
    stream.shutdown().await?;
    Ok(announcement.is_announced())
}

/// Whether a request was served for what the stream read.
#[derive(Clone, Copy)]
enum Served {
    /// A request reached the service and its response ended.
    Request,
    /// Nothing reached the service.
    Nothing,
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_with_an_http1_head_received_and_no_request_served_announces_the_close() -> io::Result<()> {
    assert!(announced_after(b"POST /ngsi-ld/v1/entities HTTP/1.1\r\nHost: localhost\r\nX-Fill: aaaa", Served::Nothing).await?);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_after_the_request_was_served_announces_nothing() -> io::Result<()> {
    assert!(!announced_after(b"GET /ngsi-ld/v1/types HTTP/1.1\r\nHost: localhost\r\n\r\n", Served::Request).await?);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_of_an_http2_connection_announces_nothing() -> io::Result<()> {
    assert!(!announced_after(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n", Served::Nothing).await?);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_of_a_connection_that_received_nothing_announces_nothing() -> io::Result<()> {
    let (mut stream, _peer, _clock, announcement) = fixture();

    stream.shutdown().await?;

    assert!(!announcement.is_announced());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_unflushed_response_is_not_followed_by_a_408() -> io::Result<()> {
    let (mut stream, mut peer, _clock, _announcement) = fixture();
    peer.write_all(b"GET / HT").await?;
    stream.write_all(b"HTTP/1.1 200 OK\r\n").await?;
    let reader = read_to_end(stream);

    pretty_assertions::assert_eq!(received(&mut peer).await?, "HTTP/1.1 200 OK\r\n");
    reader.await??;
    Ok(())
}
