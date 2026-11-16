//! The bounded, resumable comparison traversal.

use periscope_machines::Issuer;

mod buffer;

use crate::{
    refusal::Refusal,
    request::{Request, Versions},
    schema::{LeafRange, Node, Schema},
};

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

/// The session a survey has open, with everything it pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Open {
    session: u64,
    schema: Schema,
    versions: Versions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    node: Node,
    state: State,
}

/// The entry a diverged session starts from, borrowed before `begin` changes
/// anything.
enum Start<'a> {
    Range(&'a mut LeafRange),
    Root(&'a mut Slot),
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
    /// The open session; `None` while idle.
    open: Option<Open>,
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
    /// [`session`](Self::session) answers the last session issued before the
    /// restart, which is not open (`None` when `issued` is zero: nothing was
    /// ever begun in this scope). The next session carries sequence
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
            open: None,
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
        if P < usize::from(local.depth()) {
            return Err(Refusal::Undersized);
        }
        // Where a diverged session starts, taken before anything changes: a
        // single-leaf tree buffers its one range, a deeper one queues the root.
        // A survey without room for it (`O == 0`) is undersized.
        let start = if local.depth() == 0 {
            Start::Range(self.out.first_mut().ok_or(Refusal::Undersized)?)
        } else {
            if O == 0 {
                return Err(Refusal::Undersized);
            }
            Start::Root(self.slots.first_mut().ok_or(Refusal::Undersized)?)
        };
        // Last check: a session id is only spent on a session that opens.
        let session = match self.issuer.issue() {
            Ok(session) => session,
            Err(periscope_machines::Refusal::Exhausted) => return Err(Refusal::Exhausted),
        };
        self.open = Some(Open {
            session,
            schema: local,
            versions,
        });
        if local_root == remote_root {
            return Ok(Begun::Identical);
        }
        match start {
            Start::Range(range) => {
                *range = LeafRange { start: 0, end: 1 };
                self.buffered = 1;
            }
            Start::Root(slot) => {
                *slot = Slot {
                    node: Node::ROOT,
                    state: State::Queued,
                };
            }
        }
        Ok(Begun::Diverged)
    }

    /// Hand out the next node whose child digests are wanted, deepest first.
    /// `None` when nothing is queued: either answers or drained output are
    /// awaited, or the survey is finished.
    pub fn next_request(&mut self) -> Option<Request> {
        // Idle: nothing is queued, and no session to name.
        let session = self.open?.session;
        // Deepest first, then leftmost; on a tie the earlier slot stays.
        let slot = self
            .slots
            .iter_mut()
            .filter(|slot| slot.state == State::Queued)
            .reduce(|held, slot| {
                if (slot.node.level, held.node.index) > (held.node.level, slot.node.index) {
                    slot
                } else {
                    held
                }
            })?;
        slot.state = State::InFlight;
        Some(Request::issued(session, slot.node))
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
        let (open, slot) = self.in_flight_slot(request)?;
        if versions != open.versions {
            return Err(Refusal::VersionChanged);
        }
        let fanout = usize::from(open.schema.fanout());
        if local.len() != fanout || remote.len() != fanout {
            return Err(Refusal::Arity);
        }
        let mut mask = 0_u64;
        // `bit < fanout <= MAX_FANOUT == 64`, so the shift stays in range.
        for (bit, (ours, theirs)) in local.iter().zip(remote).enumerate() {
            if ours != theirs {
                mask |= 1 << bit;
            }
        }
        if mask == 0 {
            return Err(Refusal::Inconsistent);
        }
        slot.state = State::Expanded(mask);
        self.settle(open.schema);
        Ok(())
    }

    /// Put an in-flight node back in the queue, for a request that was lost.
    ///
    /// # Errors
    /// [`Refusal::Stale`] when the request belongs to another session or
    /// another survey's scope, or no session is open; [`Refusal::Unexpected`]
    /// when the node is not in flight; the survey is unchanged.
    pub fn requeue(&mut self, request: Request) -> Result<(), Refusal> {
        let (_, slot) = self.in_flight_slot(request)?;
        slot.state = State::Queued;
        Ok(())
    }

    /// Put every in-flight node back in the queue, for a dropped connection.
    /// Returns how many were requeued.
    pub fn requeue_all(&mut self) -> usize {
        let mut moved = 0;
        for slot in &mut self.slots {
            if slot.state == State::InFlight {
                slot.state = State::Queued;
                // At most `P` slots, so this cannot overflow.
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
        let Some(live) = self.out.get_mut(..self.buffered) else {
            debug_assert!(false, "buffered never exceeds O");
            return 0;
        };
        let mut taken = 0;
        for (slot, range) in out.iter_mut().zip(live.iter()) {
            *slot = *range;
            taken += 1;
        }
        // The zip bounds `taken <= live.len() == buffered`, so the shift's
        // range is in bounds and the subtraction cannot wrap.
        live.copy_within(taken.., 0);
        self.buffered -= taken;
        // Taking makes room; an idle survey buffers nothing and holds nothing.
        if taken > 0
            && let Some(open) = self.open
        {
            self.settle(open.schema);
        }
        taken
    }

    /// Drop the session. Outstanding requests become stale and buffered ranges
    /// are discarded. Never refuses: [`Aborting::AlreadyIdle`] when no session
    /// was open, which is the effect asked for already holding, and nothing changed.
    pub const fn abort(&mut self) -> Aborting {
        if self.open.is_none() {
            return Aborting::AlreadyIdle;
        }
        self.open = None;
        self.slots = [FREE; P];
        self.buffered = 0;
        Aborting::Aborted
    }

    /// Where the survey stands.
    #[must_use]
    pub fn phase(&self) -> Phase {
        if self.open.is_none() {
            Phase::Idle
        } else if self.pending() > 0 {
            Phase::Surveying
        } else if self.buffered > 0 {
            Phase::Draining
        } else {
            Phase::Complete
        }
    }

    /// Id of the latest session issued in this scope; its requests carry it.
    /// `None` while no session was ever begun.
    ///
    /// It is the latest session, open or not: [`phase`](Self::phase) says
    /// whether one is open. After [`abort`](Self::abort) it still answers the
    /// aborted session, and a [`resumed`](Self::resumed) survey, idle until
    /// its next `begin`, answers the last session issued before the restart
    /// (`None` when it was resumed from a count of zero).
    #[must_use]
    pub const fn session(&self) -> Option<u64> {
        self.issuer.last()
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
        match self.open {
            Some(open) => Some(open.schema),
            None => None,
        }
    }

    /// Tree versions pinned by the open session.
    #[must_use]
    pub const fn versions(&self) -> Option<Versions> {
        match self.open {
            Some(open) => Some(open.versions),
            None => None,
        }
    }

    /// Nodes held: queued, in flight or partly expanded. Never above `P`.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.state != State::Free)
            .count()
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
    fn live(&self, request: Request) -> Option<Open> {
        if !self.issuer.owns(request.session()) {
            return None;
        }
        self.open.filter(|open| open.session == request.session())
    }

    /// The open session and the slot of the request's node, which must be in
    /// flight: [`Refusal::Stale`] first, then [`Refusal::Unexpected`].
    fn in_flight_slot(&mut self, request: Request) -> Result<(Open, &mut Slot), Refusal> {
        let open = self.live(request).ok_or(Refusal::Stale)?;
        let node = request.node();
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.state == State::InFlight && slot.node == node)
            .ok_or(Refusal::Unexpected)?;
        Ok((open, slot))
    }

    fn count(&self, state: State) -> usize {
        self.slots.iter().filter(|s| s.state == state).count()
    }

    fn free(&self) -> usize {
        self.count(State::Free)
    }

    /// Move differing children out of answered nodes for as long as the
    /// pending slots and the output buffer allow.
    fn settle(&mut self, schema: Schema) {
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

    /// Overwrite the slot at `slot`. Every caller read that index with
    /// `get` first and `P` is fixed, so the slot is always there.
    fn put(&mut self, slot: usize, value: Slot) {
        if let Some(held) = self.slots.get_mut(slot) {
            *held = value;
        }
    }

    fn spill(&mut self, slot: usize, schema: Schema) -> bool {
        let Some(&Slot {
            node,
            state: State::Expanded(mut mask),
        }) = self.slots.get(slot)
        else {
            return false;
        };
        // Only nodes above the leaf level are ever queued, so an expanded
        // node has `node.level < depth <= u8::MAX` and `node.index <
        // fanout^level`: the child level fits a `u8`, and every child index
        // up to `base + fanout` is at most `fanout^depth`, which `Schema::new`
        // proved fits a `u64`.
        let level = node.level + 1;
        let base = node.index * u64::from(schema.fanout());
        let mut progressed = false;
        while mask != 0 {
            let low = mask.trailing_zeros();
            if level == schema.depth() {
                // Bit `low` is set, so `1 <= run <= 64 - low` and the shift
                // below is at most 63.
                let run = (mask >> low).trailing_ones();
                let start = base + u64::from(low);
                if !self.buffer(start, start + u64::from(run)) {
                    break;
                }
                mask &= !((u64::MAX >> (u64::BITS - run)) << low);
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
                    self.put(slot, child);
                    return true;
                }
                // Keep enough free slots to walk one path from this child down
                // to the leaves, so some node can always finish and free a slot.
                // This arm has `level < depth`, so the reserve cannot wrap.
                let reserve = usize::from(schema.depth() - 1 - level);
                let free = self.free();
                let Some(empty) = self
                    .slots
                    .iter_mut()
                    .find(|slot| slot.state == State::Free && slot.node == Node::ROOT)
                else {
                    break;
                };
                // `empty` is one of the `free` slots, so after taking it
                // `free - 1` remain: keep going only while that covers `reserve`.
                if free <= reserve {
                    break;
                }
                *empty = child;
                // `mask != 0` inside the loop, so this clears its lowest bit.
                mask &= mask - 1;
            }
            progressed = true;
        }
        // The parent's own slot is never the free one taken above (it is
        // expanded), so it still holds `node`.
        self.put(
            slot,
            if mask == 0 {
                FREE
            } else {
                Slot {
                    node,
                    state: State::Expanded(mask),
                }
            },
        );
        progressed
    }
}
