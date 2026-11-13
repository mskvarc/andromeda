//! What a forwarded write that did not complete may have left at its context
//! source.
//!
//! A write that never left this broker, or that the source answered with an
//! error status, left nothing there. A write that reached the source and got
//! no answer back may have been carried out all the same, and so may one a
//! gateway answered for the source with 502 or 504, which says the source
//! behind it gave no valid answer in time (RFC 9110 sections 15.6.3 and
//! 15.6.5). Whatever this broker keeps to recognise the result of such a write
//! (the identifier of a placed subscription copy, say) has to stay until the
//! source says otherwise. A creation the source refused because what it would
//! create is there already found it there, which is how a creation sent again
//! under the same identifier is answered.

/// What a forwarded write that did not complete may have left at its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEffect {
    /// The source holds nothing of the write, because it was never sent, it
    /// never reached the source, or the source answered that it did not carry
    /// it out.
    Untouched,
    /// The write reached the source and no answer of the source's came back,
    /// none at all or only a gateway's saying the source gave none in time, so
    /// the source may have carried it out.
    Possible,
    /// The source answered that what the write creates already exists there
    /// (`AlreadyExists`, clause 5.5.2), so it holds it whatever this write did.
    Present,
}
