# merkle-machines

> **AMS Merkle**, NCC-4224, *Merkle*-class survey cutter.

Engine-room flight manual: chart where two digest trees disagree, a bounded
number of nodes at a time.

Allocation-free `no_std` comparison traversal over two fixed-depth digest trees
of the same shape. The survey hands out requests for the child digests of nodes
whose digests differ, the caller answers with both trees' children, and the
survey emits the ranges of leaves that differ. It stores no digests and computes
none: hashing, storage and the exchange with the other replica belong to the
caller.

Two trees are comparable only when their `Schema` matches: fanout, depth, and the
caller's identifiers for key-to-leaf partitioning and digest algorithm. A
mismatch is refused by name and opens no session. A session pins one version of
each tree; an answer read from any other version is refused, because digests
from a tree that moved cannot be compared with digests from before it moved.

Buffers are fixed. `P` bounds the nodes held at once (queued, in flight or
partly expanded) and must be at least the tree depth; `O` bounds the buffered
leaf ranges and must be at least one. An answered node keeps its differing
children as a bit mask and releases them only as slots and output space allow,
so an answer is never refused for lack of room and a full buffer delays the
traversal instead of losing work. Lost requests are requeued one by one or all
at once. Every refusal is a `Refusal`, the crate's one refusal type, and leaves
the survey unchanged: `begin`, `answer` and `requeue` return
`Result<_, Refusal>`. Requests carry a session
id, so answers to an aborted or replaced session are stale. `abort` never
refuses: it answers `Aborting::Aborted`, or `Aborting::AlreadyIdle` when no
session was open, an idempotent repeat that changes nothing.

**Scopes.** A request is only meaningful to the survey that issued it. Session
ids come from a `periscope_machines::Issuer`: the survey's scope in the top 16
bits, a sequence counted from one below, never reissued.
`Survey::with_scope(scope)` chooses the scope and `new()` is scope zero;
`Survey::scope` and `Request::scope` read it. `answer` and `requeue` answer
`Stale` for a request whose session the survey did not issue, before looking at
any node. Give each survey its own scope: the core reads no clock and no random
source, so it cannot pick one, and two surveys left in the same scope (two
`new()` included) issue the same sessions and honour each other's requests. A
scope is `Exhausted` after 2^48 - 1 sessions. `session()` is the latest session
issued in the scope, open or not (`Issuer::last`): `None` until the first
session begins, the aborted session after `abort`. `Request::new` rebuilds a
request that crossed a wire and is `None` for a session of sequence zero, which
no survey issues.

**Restarts.** A fresh survey opens session one again, so a request that
outlived a restart could name a node in flight in a new session. Save
`Survey::issued` and rebuild with `Survey::resumed(scope, issued)`, or give
each incarnation a new scope. `resumed` restores only the session sequence: the
survey is idle, every earlier request is `Stale`, and comparing again is the
caller's job. It is `None` when `issued` is beyond
`periscope_machines::SEQUENCE_MAX`. A resumed survey's `session()` is the last
session issued before the restart, which is not open, or `None` when resumed
from zero.

**No clock.** The survey reads and records no time; `Versions` are tree
versions the caller pins for one session, not instants.

The Ruby gem mirrors `Schema`, `Versions`, `Node` and `Request` as `Data`
values of the same names and fields, each checked by the core when built: where
`Schema::new` or `Request::new` answers `None`, or a field does not fit the
core's type, `new` raises `RangeError` (`TypeError` for a wrong type). A
`LeafRange` is a half-open Ruby `Range`.

Buffered ranges are sorted, disjoint and joined when they touch. Ranges taken at
different times may still touch each other. The traversal visits only nodes
whose digests differ, deepest first; it makes no claim about minimal exchange or
round count. Equal digests are trusted to mean equal subtrees, so a digest
collision hides a difference, and a parent that differs while none of its
children do is refused as inconsistent.

`pop_range` takes one range; `drain(out)` writes as many as fit into the front
of `out`, returns the count and leaves the rest untouched. Build the buffer as
`[LeafRange::default(); N]`: the default is the empty range `0..0`, a
placeholder the survey never emits. A `LeafRange` never runs backwards: its
fields are private, read through `start()` and `end()`, and
`LeafRange::new(start, end)` answers `None` when `start > end`, so `len()`
cannot underflow.

```rust
use merkle_machines::{Begun, LeafRange, Phase, Schema, Survey, Versions};

// Two trees of four leaves; only leaf 2 differs.
let ours = [[10_u64, 20], [1, 2], [3, 4]];
let theirs = [[10_u64, 21], [1, 2], [9, 4]];

let schema = Schema::new(2, 2, 1, 1).expect("valid shape");
let versions = Versions { local: 40, remote: 57 };
let mut survey = Survey::<4, 4>::new();
assert_eq!(survey.begin(schema, schema, versions, &100_u64, &101_u64), Ok(Begun::Diverged));

while let Some(request) = survey.next_request() {
    let node = request.node();
    let row = if node.level == 0 { 0 } else { 1 + node.index as usize };
    assert_eq!(survey.answer(request, versions, &ours[row], &theirs[row]), Ok(()));
}
assert_eq!(survey.pop_range(), LeafRange::new(2, 3));
assert_eq!(survey.phase(), Phase::Complete);

// Another survey, another scope: it refuses a request it did not issue.
use merkle_machines::Refusal;
let mut north = Survey::<4, 4>::with_scope(1);
let mut south = Survey::<4, 4>::with_scope(2);
north.begin(schema, schema, versions, &100_u64, &101_u64).expect("comparable");
south.begin(schema, schema, versions, &100_u64, &101_u64).expect("comparable");
let request = north.next_request().expect("the root");
south.next_request().expect("the root");
assert_eq!(request.scope(), 1);
assert_eq!(south.answer(request, versions, &ours[0], &theirs[0]), Err(Refusal::Stale));
assert_eq!(north.answer(request, versions, &ours[0], &theirs[0]), Ok(()));
```
