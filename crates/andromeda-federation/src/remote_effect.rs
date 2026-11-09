//! What a forwarded write that did not complete may have left at its context
//! source.
//!
//! A write the source answered with an error status, or one that never left
//! this broker, left nothing there. A write that reached the source and got no
//! answer back may have been carried out all the same, so whatever this broker
//! keeps to recognise its result (the identifier of a placed subscription
//! copy, say) has to stay until the source says otherwise. A creation the
//! source refused because what it would create is there already found it
//! there, which is how a creation sent again under the same identifier is
//! answered.

/// What a forwarded write that did not complete may have left at its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEffect {
    /// The source holds nothing of the write, because it was never sent, it
    /// never reached the source, or the source answered that it did not carry
    /// it out.
    Untouched,
    /// The write reached the source and no answer came back, so the source may
    /// have carried it out.
    Possible,
    /// The source answered that what the write creates already exists there
    /// (`AlreadyExists`, clause 5.5.2), so it holds it whatever this write did.
    Present,
}
