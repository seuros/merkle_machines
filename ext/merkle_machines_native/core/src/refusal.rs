//! The one refusal type of the crate.

use core::fmt;

use crate::schema::Mismatch;

/// Why a call was refused. A refused call changes nothing.
///
/// Every operation that can refuse answers with this type:
/// [`Survey::begin`](crate::Survey::begin), [`Survey::answer`](crate::Survey::answer)
/// and [`Survey::requeue`](crate::Survey::requeue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The two trees are not comparable.
    Schema(Mismatch),
    /// Pending capacity is below the tree depth, or output capacity is zero.
    Undersized,
    /// A session is still surveying or draining; abort it first.
    Busy,
    /// The session counter is spent.
    Exhausted,
    /// The request belongs to another session, or no session is open.
    Stale,
    /// The node is not in flight: never requested, already answered or requeued.
    Unexpected,
    /// The digests were read from a different tree version than the session pinned.
    VersionChanged,
    /// A child digest list does not hold exactly `fanout` digests.
    Arity,
    /// The parents differed but no child does: the digests are not from one tree.
    Inconsistent,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Schema(Mismatch::Fanout) => "the trees differ in fanout",
            Self::Schema(Mismatch::Depth) => "the trees differ in depth",
            Self::Schema(Mismatch::Partitioning) => "the trees differ in partitioning",
            Self::Schema(Mismatch::Digest) => "the trees differ in digest",
            Self::Undersized => "capacity is below what the tree needs",
            Self::Busy => "a session is still open",
            Self::Exhausted => "session counter exhausted",
            Self::Stale => "request does not belong to the open session",
            Self::Unexpected => "node is not in flight",
            Self::VersionChanged => "digests were read from another tree version",
            Self::Arity => "child digest list does not match the fanout",
            Self::Inconsistent => "parents differ but no child does",
        })
    }
}

impl core::error::Error for Refusal {}
