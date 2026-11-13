//! The bounded, resumable comparison traversal.

use periscope_machines::{Issuer, scope_of, scoped};

use crate::{
    refusal::Refusal,
    schema::{LeafRange, Node, Schema},
};

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
    #[must_use]
    pub const fn new(session: u64, node: Node) -> Self {
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

/// Outcome of a successful [`Survey::begin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begun {
    /// The root digests are equal; nothing to traverse.
    Identical,
    /// The root digests differ; requests are waiting.
    Diverged,
}

/// Where a survey stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No session: never begun, or aborted.
    Idle,
    /// Nodes are still waiting to be requested, answered or expanded.
    Surveying,
    /// Every node is resolved; leaf ranges are still buffered.
    Draining,
    /// Every node is resolved and every range has been taken.
    Complete,
}

/// What [`Survey::abort`] did. Both are honoured; neither is a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aborting {
    /// A session was open and it was dropped.
    Aborted,
    /// No session was open: an idempotent repeat that changed nothing.
    AlreadyIdle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Queued,
    InFlight,
    /// Answered; the mask holds the differing children not yet moved out.
    Expanded(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    node: Node,
    state: State,
}

const FREE: Slot = Slot {
    node: Node::ROOT,
    state: State::Free,
};
const EMPTY: LeafRange = LeafRange { start: 0, end: 0 };

/// Locates the leaves on which two digest trees differ.
///
/// `P` bounds the nodes held at once (queued, in flight or partly expanded) and
/// `O` bounds the buffered leaf ranges. The survey stores no digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Survey<const P: usize, const O: usize> {
    schema: Option<Schema>,
    versions: Versions,
    /// Issues session ids; the latest one is the open (or last) session.
    issuer: Issuer,
    slots: [Slot; P],
    out: [LeafRange; O],
    buffered: usize,
}

impl<const P: usize, const O: usize> Default for Survey<P, O> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const P: usize, const O: usize> Survey<P, O> {
    /// An idle survey in scope zero.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_scope(0)
    }

    /// An idle survey whose sessions, and so its requests, carry `scope`.
    ///
    /// Requests are only meaningful to the survey that issued them. Two
    /// surveys in the same scope issue the same session ids, so give every
    /// survey whose requests could meet its own scope.
    #[must_use]
    pub const fn with_scope(scope: u16) -> Self {
        Self::build(Issuer::new(scope))
    }

    /// An idle survey that continues `scope` after a restart: `issued` is
    /// what [`issued`](Self::issued) answered when the caller saved it.
    ///
    /// Only the session sequence is restored. No session is open and nothing
    /// is queued or buffered: every request issued before the restart is
    /// [`Refusal::Stale`], and comparing again is the caller's job.
    /// [`session`](Self::session) names the last session issued before the
    /// restart, which is not open. The next session carries sequence
    /// `issued + 1`, so none is reissued as long as the saved count is not
    /// lower than what the survey had really issued; save it before sending a
    /// request out.
    ///
    /// `None` when `issued` is beyond `periscope_machines::SEQUENCE_MAX`,
    /// which no survey reaches.
    #[must_use]
    pub const fn resumed(scope: u16, issued: u64) -> Option<Self> {
        match Issuer::resumed(scope, issued) {
            Some(sequence) => Some(Self::build(sequence)),
            None => None,
        }
    }

    const fn build(issuer: Issuer) -> Self {
        Self {
            schema: None,
            versions: Versions {
                local: 0,
                remote: 0,
            },
            issuer,
            slots: [FREE; P],
            out: [EMPTY; O],
            buffered: 0,
        }
    }

    /// Open a session comparing two trees whose root digests the caller read at
    /// `versions`.
    ///
    /// # Errors
    /// [`Refusal::Busy`], [`Refusal::Schema`], [`Refusal::Undersized`] or
    /// [`Refusal::Exhausted`]; the survey is unchanged.
    pub fn begin<D: PartialEq>(
        &mut self,
        local: Schema,
        remote: Schema,
        versions: Versions,
        local_root: &D,
        remote_root: &D,
    ) -> Result<Begun, Refusal> {
        if matches!(self.phase(), Phase::Surveying | Phase::Draining) {
            return Err(Refusal::Busy);
        }
        local.compatible(&remote).map_err(Refusal::Schema)?;
        if O == 0 || P < usize::from(local.depth()) {
            return Err(Refusal::Undersized);
        }
        // Last check: a session id is only spent on a session that opens.
        match self.issuer.issue() {
            Ok(_) => {}
            Err(periscope_machines::Refusal::Exhausted) => return Err(Refusal::Exhausted),
        }
        self.schema = Some(local);
        self.versions = versions;
        if local_root == remote_root {
            return Ok(Begun::Identical);
        }
        if local.depth() == 0 {
            self.out[0] = LeafRange { start: 0, end: 1 };
            self.buffered = 1;
        } else {
            self.slots[0] = Slot {
                node: Node::ROOT,
                state: State::Queued,
            };
        }
        Ok(Begun::Diverged)
    }

    /// Hand out the next node whose child digests are wanted, deepest first.
    /// `None` when nothing is queued: either answers or drained output are
    /// awaited, or the survey is finished.
    pub fn next_request(&mut self) -> Option<Request> {
        let mut best: Option<usize> = None;
        for (i, slot) in self.slots.iter().enumerate() {
            if slot.state != State::Queued {
                continue;
            }
            let better = best.is_none_or(|b| {
                let held = self.slots[b].node;
                (slot.node.level, held.index) > (held.level, slot.node.index)
            });
            if better {
                best = Some(i);
            }
        }
        let i = best?;
        self.slots[i].state = State::InFlight;
        Some(Request::new(self.session(), self.slots[i].node))
    }

    /// Supply both trees' child digests for an in-flight node, in child order.
    ///
    /// # Errors
    /// [`Refusal::Stale`] (a request of another session or another survey's
    /// scope), [`Refusal::Unexpected`], [`Refusal::VersionChanged`],
    /// [`Refusal::Arity`] or [`Refusal::Inconsistent`]; the survey is unchanged
    /// and the node stays in flight.
    pub fn answer<D: PartialEq>(
        &mut self,
        request: Request,
        versions: Versions,
        local: &[D],
        remote: &[D],
    ) -> Result<(), Refusal> {
        let schema = self.live(request).ok_or(Refusal::Stale)?;
        let slot = self
            .position(request.node, State::InFlight)
            .ok_or(Refusal::Unexpected)?;
        if versions != self.versions {
            return Err(Refusal::VersionChanged);
        }
        let fanout = usize::from(schema.fanout());
        if local.len() != fanout || remote.len() != fanout {
            return Err(Refusal::Arity);
        }
        let mut mask = 0_u64;
        for (bit, (ours, theirs)) in local.iter().zip(remote).enumerate() {
            if ours != theirs {
                mask |= 1 << bit;
            }
        }
        if mask == 0 {
            return Err(Refusal::Inconsistent);
        }
        self.slots[slot].state = State::Expanded(mask);
        self.settle();
        Ok(())
    }

    /// Put an in-flight node back in the queue, for a request that was lost.
    ///
    /// # Errors
    /// [`Refusal::Stale`] when the request belongs to another session or
    /// another survey's scope, or no session is open; [`Refusal::Unexpected`]
    /// when the node is not in flight; the survey is unchanged.
    pub fn requeue(&mut self, request: Request) -> Result<(), Refusal> {
        self.live(request).ok_or(Refusal::Stale)?;
        let slot = self
            .position(request.node, State::InFlight)
            .ok_or(Refusal::Unexpected)?;
        self.slots[slot].state = State::Queued;
        Ok(())
    }

    /// Put every in-flight node back in the queue, for a dropped connection.
    /// Returns how many were requeued.
    pub fn requeue_all(&mut self) -> usize {
        let mut moved = 0;
        for slot in &mut self.slots {
            if slot.state == State::InFlight {
                slot.state = State::Queued;
                moved += 1;
            }
        }
        moved
    }

    /// Take the lowest buffered leaf range. Taking makes room, so nodes held
    /// back by a full buffer move on.
    pub fn pop_range(&mut self) -> Option<LeafRange> {
        let mut one = [EMPTY];
        (self.drain(&mut one) == 1).then_some(one[0])
    }

    /// Move buffered ranges into the front of `out`, lowest first, until it is
    /// full or no range is left, then let held-back nodes use the room. Returns
    /// how many were written; those are sorted and do not touch each other, and
    /// the rest of `out` is untouched.
    pub fn drain(&mut self, out: &mut [LeafRange]) -> usize {
        let taken = self.buffered.min(out.len());
        out[..taken].copy_from_slice(&self.out[..taken]);
        self.out.copy_within(taken..self.buffered, 0);
        self.buffered -= taken;
        if taken > 0 {
            self.settle();
        }
        taken
    }

    /// Drop the session. Outstanding requests become stale and buffered ranges
    /// are discarded. Never refuses: [`Aborting::AlreadyIdle`] when no session
    /// was open, which is the effect asked for already holding, and nothing changed.
    pub const fn abort(&mut self) -> Aborting {
        if self.schema.is_none() {
            return Aborting::AlreadyIdle;
        }
        self.schema = None;
        self.slots = [FREE; P];
        self.buffered = 0;
        Aborting::Aborted
    }

    /// Where the survey stands.
    #[must_use]
    pub fn phase(&self) -> Phase {
        if self.schema.is_none() {
            Phase::Idle
        } else if self.pending() > 0 {
            Phase::Surveying
        } else if self.buffered > 0 {
            Phase::Draining
        } else {
            Phase::Complete
        }
    }

    /// Id of the latest session; requests carry it. Zero, which is never
    /// issued, until the first session begins.
    #[must_use]
    pub const fn session(&self) -> u64 {
        match self.issuer.issued() {
            0 => 0,
            sequence => scoped(self.issuer.scope(), sequence),
        }
    }

    /// Sessions issued in this scope so far, the open one included: the count
    /// to save for [`resumed`](Self::resumed).
    #[must_use]
    pub const fn issued(&self) -> u64 {
        self.issuer.issued()
    }

    /// The scope carried by every session, and so every request, of this survey.
    #[must_use]
    pub const fn scope(&self) -> u16 {
        self.issuer.scope()
    }

    /// Schema of the open session.
    #[must_use]
    pub const fn schema(&self) -> Option<Schema> {
        self.schema
    }

    /// Tree versions pinned by the open session.
    #[must_use]
    pub const fn versions(&self) -> Option<Versions> {
        if self.schema.is_some() {
            Some(self.versions)
        } else {
            None
        }
    }

    /// Nodes held: queued, in flight or partly expanded. Never above `P`.
    #[must_use]
    pub fn pending(&self) -> usize {
        P - self.free()
    }

    /// Nodes requested and not yet answered.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.count(State::InFlight)
    }

    /// Leaf ranges waiting to be taken. Never above `O`.
    #[must_use]
    pub const fn buffered(&self) -> usize {
        self.buffered
    }

    /// Origin first: a request whose session this survey's issuer never
    /// handed out names nothing here, whatever node it carries.
    fn live(&self, request: Request) -> Option<Schema> {
        if !self.issuer.owns(request.session) {
            return None;
        }
        self.schema.filter(|_| request.session == self.session())
    }

    fn position(&self, node: Node, state: State) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.state == state && slot.node == node)
    }

    fn count(&self, state: State) -> usize {
        self.slots.iter().filter(|s| s.state == state).count()
    }

    fn free(&self) -> usize {
        self.count(State::Free)
    }

    /// Move differing children out of answered nodes for as long as the
    /// pending slots and the output buffer allow.
    fn settle(&mut self) {
        let Some(schema) = self.schema else { return };
        loop {
            let mut progressed = false;
            for slot in 0..P {
                progressed |= self.spill(slot, schema);
            }
            if !progressed {
                break;
            }
        }
    }

    fn spill(&mut self, slot: usize, schema: Schema) -> bool {
        let Slot {
            node,
            state: State::Expanded(mut mask),
        } = self.slots[slot]
        else {
            return false;
        };
        let level = node.level + 1;
        let base = node.index * u64::from(schema.fanout());
        let mut progressed = false;
        while mask != 0 {
            let low = mask.trailing_zeros();
            if level == schema.depth() {
                let run = (mask >> low).trailing_ones();
                let start = base + u64::from(low);
                if !self.buffer(start, start + u64::from(run)) {
                    break;
                }
                mask &= !((u64::MAX >> (64 - run)) << low);
            } else {
                let child = Slot {
                    node: Node {
                        level,
                        index: base + u64::from(low),
                    },
                    state: State::Queued,
                };
                if mask.is_power_of_two() {
                    // The last child inherits its parent's slot.
                    self.slots[slot] = child;
                    return true;
                }
                // Keep enough free slots to walk one path from this child down
                // to the leaves, so some node can always finish and free a slot.
                let reserve = usize::from(schema.depth() - 1 - level);
                let Some(empty) = self.position(Node::ROOT, State::Free) else {
                    break;
                };
                if self.free() - 1 < reserve {
                    break;
                }
                self.slots[empty] = child;
                mask &= mask - 1;
            }
            progressed = true;
        }
        self.slots[slot].state = if mask == 0 {
            State::Free
        } else {
            State::Expanded(mask)
        };
        if mask == 0 {
            self.slots[slot].node = Node::ROOT;
        }
        progressed
    }

    /// Insert a run of leaves into the sorted buffer, joining touching ranges.
    fn buffer(&mut self, start: u64, end: u64) -> bool {
        let live = &self.out[..self.buffered];
        let at = live
            .iter()
            .position(|r| r.start > start)
            .unwrap_or(live.len());
        let joins_left = at > 0 && self.out[at - 1].end == start;
        let joins_right = at < self.buffered && self.out[at].start == end;
        match (joins_left, joins_right) {
            (true, true) => {
                self.out[at - 1].end = self.out[at].end;
                self.out.copy_within(at + 1..self.buffered, at);
                self.buffered -= 1;
            }
            (true, false) => self.out[at - 1].end = end,
            (false, true) => self.out[at].start = start,
            (false, false) => {
                if self.buffered == O {
                    return false;
                }
                self.out.copy_within(at..self.buffered, at + 1);
                self.out[at] = LeafRange { start, end };
                self.buffered += 1;
            }
        }
        true
    }
}
