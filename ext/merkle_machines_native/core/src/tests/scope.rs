//! Sessions are scoped ids: only the survey that issued a request honours it.

use super::*;

/// Two surveys, the same calls: a request from one answered into, or requeued
/// on, the other named a node in flight there, because both were in session 1.
#[test]
fn a_request_from_another_scope_is_refused_and_changes_nothing() {
    let shape = schema(2, 2);
    let mut a = Survey::<2, 2>::with_scope(1);
    let mut b = Survey::<2, 2>::with_scope(2);
    assert_eq!((a.scope(), b.scope()), (1, 2));
    assert_eq!(a.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(b.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    let from_a = must(a.next_request());
    let from_b = must(b.next_request());
    assert_eq!((from_a.scope(), from_b.scope()), (1, 2));
    assert_eq!(from_a.node(), from_b.node());
    assert_eq!(sequence_of(from_a.session()), sequence_of(from_b.session()));
    let (a_frozen, b_frozen) = (a, b);

    assert_eq!(b.answer(from_a, V, &[1, 2], &[1, 3]), Err(Refusal::Stale));
    assert_eq!(b.requeue(from_a), Err(Refusal::Stale));
    assert_eq!(b, b_frozen);
    assert_eq!(a.answer(from_b, V, &[1, 2], &[1, 3]), Err(Refusal::Stale));
    assert_eq!(a.requeue(from_b), Err(Refusal::Stale));
    assert_eq!(a, a_frozen);

    assert_eq!(a.answer(from_a, V, &[1, 2], &[1, 3]), Ok(()));
    assert_eq!(b.requeue(from_b), Ok(()));
}

/// Scopes are the caller's: the core reads no clock and no random source, so
/// two surveys left in one scope issue the same sessions and still alias.
#[test]
fn two_surveys_left_in_the_same_scope_still_alias() {
    let shape = schema(2, 2);
    let mut a = Survey::<2, 2>::new();
    let mut b = Survey::<2, 2>::new();
    assert_eq!(a.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(b.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    let from_a = must(a.next_request());
    assert_eq!(Some(from_a), b.next_request());
    assert_eq!(b.answer(from_a, V, &[1, 2], &[1, 3]), Ok(()));
}

#[test]
fn new_is_scope_zero_and_sessions_count_from_one() {
    assert_eq!(Survey::<1, 1>::new(), Survey::with_scope(0));
    assert_eq!(Survey::<1, 1>::default(), Survey::with_scope(0));
    assert_eq!(Survey::<1, 1>::resumed(7, 0), Some(Survey::with_scope(7)));
    assert_ne!(Survey::<1, 1>::with_scope(1), Survey::with_scope(2));

    let shape = schema(2, 1);
    let mut plain = Survey::<1, 1>::new();
    assert_eq!((plain.scope(), plain.session()), (0, 0));
    assert_eq!(plain.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(plain.session(), 1);
    assert_eq!(plain.next_request(), Some(Request::new(1, Node::ROOT)));
    assert_eq!(plain.abort(), Aborting::Aborted);
    assert_eq!(plain.begin(shape, shape, V, &1, &1), Ok(Begun::Identical));
    assert_eq!(plain.session(), 2);

    let mut scoped_one = Survey::<1, 1>::with_scope(7);
    // No session yet: zero, the id no issuer hands out, whatever the scope.
    assert_eq!((scoped_one.scope(), scoped_one.session()), (7, 0));
    assert_eq!(
        scoped_one.begin(shape, shape, V, &1, &2),
        Ok(Begun::Diverged)
    );
    assert_eq!(scoped_one.session(), scoped(7, 1));
    let request = must(scoped_one.next_request());
    assert_eq!((request.session(), request.scope()), (scoped(7, 1), 7));
}

#[test]
fn a_session_of_this_scope_that_was_never_issued_is_stale() {
    let shape = schema(2, 1);
    let mut survey = Survey::<1, 1>::with_scope(3);
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    let live = must(survey.next_request());
    let frozen = survey;
    for session in [scoped(3, 2), scoped(3, 0), 0, 1] {
        let unissued = Request::new(session, live.node());
        assert_eq!(survey.answer(unissued, V, &[1], &[2]), Err(Refusal::Stale));
        assert_eq!(survey.requeue(unissued), Err(Refusal::Stale));
    }
    assert_eq!(survey, frozen);
}

#[test]
fn the_last_session_of_a_scope_is_issued_and_then_begin_is_exhausted() {
    let shape = schema(2, 1);
    let mut survey = must(Survey::<1, 1>::resumed(3, SEQUENCE_MAX - 1));
    // A call refused for another reason spends no session id.
    let frozen = survey;
    let deep = schema(2, 5);
    assert_eq!(
        survey.begin(deep, deep, V, &1, &2),
        Err(Refusal::Undersized)
    );
    assert_eq!(survey, frozen);
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(survey.session(), scoped(3, SEQUENCE_MAX));
    let last = must(survey.next_request());
    assert_eq!((last.session(), last.scope()), (scoped(3, SEQUENCE_MAX), 3));
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Err(Refusal::Busy));
    // The last session still runs to its end.
    assert_eq!(survey.answer(last, V, &[1, 2], &[1, 3]), Ok(()));
    assert_eq!(survey.pop_range(), Some(LeafRange { start: 1, end: 2 }));
    assert_eq!(survey.phase(), Phase::Complete);
    let frozen = survey;
    assert_eq!(
        survey.begin(shape, shape, V, &1, &2),
        Err(Refusal::Exhausted)
    );
    assert_eq!(survey, frozen);
    assert_eq!(survey.abort(), Aborting::Aborted);
    assert_eq!(
        survey.begin(shape, shape, V, &1, &2),
        Err(Refusal::Exhausted)
    );
    assert_eq!(
        survey.answer(last, V, &[1, 2], &[1, 3]),
        Err(Refusal::Stale)
    );
}

/// A fresh survey would open session 1 again, and a request that outlived the
/// restart would name a node in flight in the new session.
#[test]
fn a_resumed_survey_continues_its_scope_and_refuses_every_earlier_request() {
    let shape = schema(2, 2);
    let mut first = Survey::<2, 2>::with_scope(5);
    assert_eq!(first.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    let aborted = must(first.next_request());
    assert_eq!(first.abort(), Aborting::Aborted);
    assert_eq!(first.begin(shape, shape, V, &1, &1), Ok(Begun::Identical));
    let identical = Request::new(first.session(), Node::ROOT);
    assert_eq!(first.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    let open = must(first.next_request());
    assert_eq!((first.issued(), first.session()), (3, scoped(5, 3)));
    let earlier = [aborted, identical, open];

    let mut survey = must(Survey::<2, 2>::resumed(first.scope(), first.issued()));
    assert_eq!((survey.scope(), survey.issued()), (5, 3));
    // Only the sequence came back: no session is open, not even the last one.
    assert_eq!(
        (survey.phase(), survey.session()),
        (Phase::Idle, open.session())
    );
    assert_eq!((survey.schema(), survey.versions()), (None, None));
    assert_eq!((survey.pending(), survey.buffered()), (0, 0));
    let frozen = survey;
    for request in earlier {
        assert_eq!(
            survey.answer(request, V, &[1, 2], &[1, 3]),
            Err(Refusal::Stale)
        );
        assert_eq!(survey.requeue(request), Err(Refusal::Stale));
    }
    assert_eq!(survey, frozen);
    assert_eq!(survey.abort(), Aborting::AlreadyIdle);

    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!((survey.issued(), survey.session()), (4, scoped(5, 4)));
    let next = must(survey.next_request());
    assert_eq!(next.session(), scoped(5, 4));
    // The same node is in flight again, under an id no earlier request has.
    assert_eq!(next.node(), open.node());
    let frozen = survey;
    for request in earlier {
        assert_ne!(request.session(), next.session());
        assert_eq!(
            survey.answer(request, V, &[1, 2], &[1, 3]),
            Err(Refusal::Stale)
        );
        assert_eq!(survey.requeue(request), Err(Refusal::Stale));
    }
    assert_eq!(survey, frozen);
    assert_eq!(survey.answer(next, V, &[1, 2], &[1, 3]), Ok(()));
}

#[test]
fn a_survey_resumed_at_the_last_sequence_is_spent_and_one_past_it_is_none() {
    let shape = schema(2, 1);
    let mut spent = must(Survey::<1, 1>::resumed(3, SEQUENCE_MAX));
    assert_eq!((spent.scope(), spent.issued()), (3, SEQUENCE_MAX));
    assert_eq!(spent.phase(), Phase::Idle);
    let frozen = spent;
    assert_eq!(
        spent.begin(shape, shape, V, &1, &2),
        Err(Refusal::Exhausted)
    );
    assert_eq!(spent, frozen);
    assert_eq!(Survey::<1, 1>::resumed(3, SEQUENCE_MAX + 1), None);
    assert_eq!(Survey::<1, 1>::resumed(3, u64::MAX), None);
}
