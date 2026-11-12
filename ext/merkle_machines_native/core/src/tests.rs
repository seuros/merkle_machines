use super::*;

const V: Versions = Versions {
    local: 7,
    remote: 9,
};

fn must<T>(value: Option<T>) -> T {
    let Some(value) = value else {
        panic!("expected a value");
    };
    value
}

fn at(index: u64) -> usize {
    let Ok(index) = usize::try_from(index) else {
        panic!("index fits a usize");
    };
    index
}

fn schema(fanout: u8, depth: u8) -> Schema {
    must(Schema::new(fanout, depth, 1, 1))
}

/// Naive reference tree: a node's digest is a mix of its children's digests.
struct Tree {
    schema: Schema,
    leaves: [u64; 256],
}

impl Tree {
    fn digest(&self, node: Node) -> u64 {
        if node.level == self.schema.depth() {
            return self.leaves[at(node.index)];
        }
        let mut mixed = 0x9E37_79B9_7F4A_7C15_u64;
        for child in 0..u64::from(self.schema.fanout()) {
            let below = self.digest(Node {
                level: node.level + 1,
                index: node.index * u64::from(self.schema.fanout()) + child,
            });
            mixed = (mixed.rotate_left(7) ^ below).wrapping_mul(0x0100_0000_01B3);
        }
        mixed
    }

    fn children(&self, node: Node, into: &mut [u64; 64]) -> usize {
        let fanout = self.schema.fanout();
        for child in 0..fanout {
            into[usize::from(child)] = self.digest(Node {
                level: node.level + 1,
                index: node.index * u64::from(fanout) + u64::from(child),
            });
        }
        usize::from(fanout)
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

fn reply<const P: usize, const O: usize>(
    survey: &mut Survey<P, O>,
    request: Request,
    ours: &Tree,
    theirs: &Tree,
) -> Result<(), Refusal> {
    let (mut a, mut b) = ([0; 64], [0; 64]);
    let n = ours.children(request.node(), &mut a);
    theirs.children(request.node(), &mut b);
    survey.answer(request, V, &a[..n], &b[..n])
}

/// Drive one survey to completion under a random schedule and compare the
/// emitted leaves with a leaf-by-leaf diff.
fn run<const P: usize, const O: usize>(rng: &mut Rng, fanout: u8, depth: u8) {
    let shape = schema(fanout, depth);
    let count = at(shape.leaves());
    let mut ours = Tree {
        schema: shape,
        leaves: [0; 256],
    };
    for leaf in &mut ours.leaves[..count] {
        *leaf = rng.next();
    }
    let mut theirs = Tree {
        schema: shape,
        leaves: ours.leaves,
    };
    let density = rng.below(4);
    for leaf in &mut theirs.leaves[..count] {
        let flip = match density {
            0 => false,
            1 => rng.below(16) == 0,
            2 => rng.below(2) == 0,
            _ => true,
        };
        if flip {
            *leaf ^= 1 | rng.next();
        }
    }

    let mut survey = Survey::<P, O>::new();
    let roots = (ours.digest(Node::ROOT), theirs.digest(Node::ROOT));
    let begun = survey.begin(shape, shape, V, &roots.0, &roots.1);
    let same = ours.leaves == theirs.leaves;
    assert_eq!(
        begun,
        Ok(if same {
            Begun::Identical
        } else {
            Begun::Diverged
        })
    );

    let mut seen = [false; 256];
    let mut flying = [None::<Request>; 64];
    let mut steps = 0_u32;
    while survey.phase() != Phase::Complete {
        steps += 1;
        assert!(steps < 100_000, "survey stalled: {survey:?}");
        assert!(survey.pending() <= P && survey.buffered() <= O);
        match rng.below(8) {
            0..=2 => {
                if let Some(request) = survey.next_request() {
                    let spot = must(flying.iter().position(Option::is_none));
                    flying[spot] = Some(request);
                }
            }
            3..=5 => {
                let pick = at(rng.below(64));
                if let Some(request) = flying[pick..].iter_mut().find_map(Option::take) {
                    let frozen = survey;
                    assert_eq!(reply(&mut survey, request, &ours, &theirs), Ok(()));
                    assert_ne!(survey, frozen);
                    let frozen = survey;
                    let again = reply(&mut survey, request, &ours, &theirs);
                    assert_eq!(again, Err(Refusal::Unexpected));
                    assert_eq!(survey, frozen);
                }
            }
            6 => {
                let mut taken = [LeafRange { start: 0, end: 0 }; 3];
                let want = at(rng.below(4));
                let got = survey.drain(&mut taken[..want]);
                for range in &taken[..got] {
                    assert!(!range.is_empty() && range.end <= shape.leaves());
                    for leaf in range.start..range.end {
                        let leaf = at(leaf);
                        assert!(!seen[leaf], "leaf {leaf} emitted twice");
                        seen[leaf] = true;
                    }
                }
                for pair in taken[..got].windows(2) {
                    assert!(pair[0].end < pair[1].start, "sorted and coalesced");
                }
            }
            _ => {
                if rng.below(4) == 0 {
                    let lost = flying.iter().flatten().count();
                    assert_eq!(survey.in_flight(), lost);
                    assert_eq!(survey.requeue_all(), lost);
                    flying = [None; 64];
                } else if let Some(request) = flying.iter_mut().find_map(Option::take) {
                    assert!(survey.requeue(request));
                    assert!(!survey.requeue(request));
                }
            }
        }
    }
    let differs = |leaf: usize| ours.leaves[leaf] != theirs.leaves[leaf];
    assert!(
        (0..count).all(|leaf| seen[leaf] == differs(leaf)),
        "{seen:?}"
    );
    assert!(survey.next_request().is_none() && survey.pop_range().is_none());
}

#[test]
fn randomized_schedules_match_a_leaf_by_leaf_diff() {
    let mut rng = Rng(0x5EED_CAFE_F00D_0001);
    for _ in 0..150 {
        run::<1, 1>(&mut rng, 2, 1);
        run::<4, 1>(&mut rng, 2, 4);
        run::<8, 1>(&mut rng, 2, 8);
        run::<3, 2>(&mut rng, 4, 3);
        run::<4, 3>(&mut rng, 4, 4);
        run::<9, 5>(&mut rng, 3, 5);
        run::<2, 1>(&mut rng, 16, 2);
        run::<16, 32>(&mut rng, 2, 7);
        run::<0, 1>(&mut rng, 2, 0);
        run::<5, 4>(&mut rng, 64, 1);
    }
}

#[test]
fn schema_validation_and_spans() {
    assert!(Schema::new(1, 3, 0, 0).is_none());
    assert!(Schema::new(65, 1, 0, 0).is_none());
    assert!(Schema::new(2, 64, 0, 0).is_none());
    assert!(Schema::new(64, 11, 0, 0).is_none());
    let wide = must(Schema::new(2, 63, 0, 0));
    assert_eq!(wide.leaves(), 1 << 63);
    assert_eq!(wide.width(200), 1 << 63);
    let last = Node {
        level: 63,
        index: (1 << 63) - 1,
    };
    let span = must(wide.span(last));
    assert_eq!(
        (span.start, span.end, span.len()),
        ((1 << 63) - 1, 1 << 63, 1)
    );
    assert_eq!(must(wide.span(Node::ROOT)).len(), 1 << 63);
    assert!(
        wide.span(Node {
            level: 64,
            index: 0
        })
        .is_none()
    );
    assert!(wide.span(Node { level: 1, index: 2 }).is_none());
    let small = must(Schema::new(4, 2, 5, 6));
    assert_eq!(
        (
            small.fanout(),
            small.depth(),
            small.partitioning(),
            small.digest()
        ),
        (4, 2, 5, 6)
    );
}

#[test]
fn mismatched_schemas_are_refused_without_a_session() {
    let base = must(Schema::new(4, 3, 10, 20));
    let cases = [
        (Schema::new(8, 3, 10, 20), Mismatch::Fanout),
        (Schema::new(4, 2, 10, 20), Mismatch::Depth),
        (Schema::new(4, 3, 11, 20), Mismatch::Partitioning),
        (Schema::new(4, 3, 10, 21), Mismatch::Digest),
    ];
    let mut survey = Survey::<4, 4>::new();
    for (other, mismatch) in cases {
        let refused = survey.begin(base, must(other), V, &1, &2);
        assert_eq!(refused, Err(Refusal::Schema(mismatch)));
        assert_eq!(survey, Survey::new());
    }
    assert_eq!(
        (survey.phase(), survey.schema(), survey.versions()),
        (Phase::Idle, None, None)
    );
}

#[test]
fn capacity_and_session_refusals_leave_the_survey_untouched() {
    let deep = schema(2, 5);
    assert_eq!(
        Survey::<4, 4>::new().begin(deep, deep, V, &1, &2),
        Err(Refusal::Undersized)
    );
    assert_eq!(
        Survey::<5, 0>::new().begin(deep, deep, V, &1, &2),
        Err(Refusal::Undersized)
    );

    let mut survey = Survey::<5, 1>::new();
    assert_eq!(survey.begin(deep, deep, V, &1, &2), Ok(Begun::Diverged));
    let frozen = survey;
    assert_eq!(survey.begin(deep, deep, V, &1, &2), Err(Refusal::Busy));
    assert_eq!(survey, frozen);
    assert!(survey.abort());
    assert!(!survey.abort());
    assert_eq!(survey.phase(), Phase::Idle);

    survey.session = u64::MAX;
    let frozen = survey;
    assert_eq!(survey.begin(deep, deep, V, &1, &2), Err(Refusal::Exhausted));
    assert_eq!(survey, frozen);
}

#[test]
fn identical_roots_and_single_leaf_trees() {
    let mut survey = Survey::<0, 1>::new();
    let leaf = schema(2, 0);
    assert_eq!(survey.begin(leaf, leaf, V, &5, &5), Ok(Begun::Identical));
    assert_eq!(survey.phase(), Phase::Complete);
    assert_eq!(survey.begin(leaf, leaf, V, &5, &6), Ok(Begun::Diverged));
    assert_eq!((survey.phase(), survey.session()), (Phase::Draining, 2));
    assert_eq!(survey.next_request(), None);
    assert_eq!(survey.pop_range(), Some(LeafRange { start: 0, end: 1 }));
    assert_eq!(survey.phase(), Phase::Complete);
}

#[test]
fn answers_are_checked_before_anything_moves() {
    let shape = schema(2, 2);
    let mut survey = Survey::<2, 2>::new();
    let forged = Request::new(1, Node::ROOT);
    assert_eq!(
        survey.answer(forged, V, &[1, 2], &[1, 3]),
        Err(Refusal::Stale)
    );
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(
        survey.answer(forged, V, &[1, 2], &[1, 3]),
        Err(Refusal::Unexpected)
    );
    let request = must(survey.next_request());
    assert_eq!((request, survey.in_flight()), (forged, 1));
    let frozen = survey;
    let moved = Versions {
        local: 8,
        remote: 9,
    };
    let wrong_node = Request::new(1, Node { level: 1, index: 0 });
    let refusals = [
        (
            survey.answer(Request::new(0, Node::ROOT), V, &[1, 2], &[1, 3]),
            Refusal::Stale,
        ),
        (
            survey.answer(wrong_node, V, &[1, 2], &[1, 3]),
            Refusal::Unexpected,
        ),
        (
            survey.answer(request, moved, &[1, 2], &[1, 3]),
            Refusal::VersionChanged,
        ),
        (survey.answer(request, V, &[1], &[1, 3]), Refusal::Arity),
        (
            survey.answer(request, V, &[1, 2], &[1, 3, 4]),
            Refusal::Arity,
        ),
        (
            survey.answer(request, V, &[1, 2], &[1, 2]),
            Refusal::Inconsistent,
        ),
    ];
    for (got, want) in refusals {
        assert_eq!(got, Err(want));
    }
    assert_eq!(survey, frozen);

    assert_eq!(survey.answer(request, V, &[1, 2], &[1, 3]), Ok(()));
    let child = must(survey.next_request());
    assert_eq!(child.node(), Node { level: 1, index: 1 });
    assert!(survey.abort());
    assert_eq!(
        survey.answer(child, V, &[1, 2], &[3, 4]),
        Err(Refusal::Stale)
    );
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(
        survey.answer(child, V, &[1, 2], &[3, 4]),
        Err(Refusal::Stale)
    );
    assert!(!survey.requeue(child));
}

#[test]
fn full_buffers_hold_nodes_back_until_the_caller_drains() {
    // Leaves 0, 2, 4 and 6 differ: four separate ranges through a one-range buffer.
    let shape = schema(2, 3);
    let mut survey = Survey::<3, 1>::new();
    assert_eq!(survey.begin(shape, shape, V, &0, &1), Ok(Begun::Diverged));
    let mut taken = [LeafRange { start: 0, end: 0 }; 8];
    let mut got = 0;
    let mut most_pending = 0;
    while survey.phase() != Phase::Complete {
        most_pending = most_pending.max(survey.pending());
        if let Some(request) = survey.next_request() {
            let leaf_parent = request.node().level == 2;
            let theirs = if leaf_parent { [1, 0] } else { [1, 1] };
            assert_eq!(survey.answer(request, V, &[0, 0], &theirs), Ok(()));
        } else {
            assert_eq!(survey.buffered(), 1);
            got += survey.drain(&mut taken[got..=got]);
        }
    }
    taken[..got].sort_unstable_by_key(|range| range.start);
    let starts = [0, 2, 4, 6].map(|start| LeafRange {
        start,
        end: start + 1,
    });
    assert_eq!((&taken[..got], most_pending), (&starts[..], 3));
}

#[test]
fn ranges_coalesce_across_nodes_at_the_top_of_the_index_space() {
    // Fanout 2, depth 63: follow the rightmost path, then its left neighbour.
    let shape = schema(2, 63);
    let mut survey = Survey::<63, 2>::new();
    assert_eq!(survey.begin(shape, shape, V, &0, &1), Ok(Begun::Diverged));
    let mut requests = 0;
    while let Some(request) = survey.next_request() {
        requests += 1;
        let node = request.node();
        let theirs = match node.level {
            61 => [1, 1],
            62 if node.index % 2 == 0 => [0, 1],
            62 => [1, 0],
            _ => [0, 1],
        };
        assert_eq!(survey.answer(request, V, &[0, 0], &theirs), Ok(()));
    }
    let top = 1_u64 << 63;
    let joined = LeafRange {
        start: top - 3,
        end: top - 1,
    };
    assert_eq!((requests, survey.pop_range()), (64, Some(joined)));
    assert_eq!(
        (survey.pop_range(), survey.phase()),
        (None, Phase::Complete)
    );
}
