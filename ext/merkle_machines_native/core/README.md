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
at once. Every refusal leaves the survey unchanged. Requests carry a session
counter, so answers to an aborted or replaced session are stale.

Buffered ranges are sorted, disjoint and joined when they touch. Ranges taken at
different times may still touch each other. The traversal visits only nodes
whose digests differ, deepest first; it makes no claim about minimal exchange or
round count. Equal digests are trusted to mean equal subtrees, so a digest
collision hides a difference, and a parent that differs while none of its
children do is refused as inconsistent.

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
assert_eq!(survey.pop_range(), Some(LeafRange { start: 2, end: 3 }));
assert_eq!(survey.phase(), Phase::Complete);
```
