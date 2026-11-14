//! What travels between a survey and the replica answering it.

use periscope_machines::{scope_of, sequence_of};

use crate::schema::Node;

/// Tree versions the digests of one session must all be read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    /// Version of the local tree.
    pub local: u64,
    /// Version of the remote tree.
    pub remote: u64,
}

/// A request for the child digests of one node, on both trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    session: u64,
    node: Node,
}

impl Request {
    /// Rebuild a request that crossed a wire. A forged request is refused by
    /// [`Survey::answer`] unless it names a node that is in flight.
    ///
    /// `None` when `session` has sequence number zero: no survey issues that
    /// id, so it names no session at all.
    #[must_use]
    pub const fn new(session: u64, node: Node) -> Option<Self> {
        if sequence_of(session) == 0 {
            return None;
        }
        Some(Self { session, node })
    }

    /// A request for a session the survey issued, so never sequence zero.
    pub(crate) const fn issued(session: u64, node: Node) -> Self {
        Self { session, node }
    }

    /// Session the request belongs to: a scoped id, the issuing survey's
    /// scope in the top 16 bits and its session count (from one) below.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The scope of the survey that issued the request's session.
    #[must_use]
    pub const fn scope(&self) -> u16 {
        scope_of(self.session)
    }

    /// Node whose children are wanted.
    #[must_use]
    pub const fn node(&self) -> Node {
        self.node
    }
}
