//! Sessions are scoped ids: only the survey that issued a request honours it.

use super::*;
use matryona_machines::testing::refuses;

/// Two surveys, the same calls: a request from one answered into, or requeued
/// on, the other named a node in flight there, because both were in session 1.
/// Scopes are the caller's: two surveys left in one scope still alias.
#[test]
fn a_request_from_another_scope_is_refused_and_changes_nothing() {
    let shape = schema(2, 2);
    let mut a = Survey::<2, 2>::with_scope(1);
    let mut b = Survey::<2, 2>::with_scope(2);
    assert_eq!((a.scope(), b.scope()), (1, 2));
    diverge(&mut a, shape);
    diverge(&mut b, shape);
    let from_a = must(a.next_request());
    let from_b = must(b.next_request());
    assert_eq!((from_a.scope(), from_b.scope()), (1, 2));
    assert_eq!(from_a.node(), from_b.node());
    assert_eq!(sequence_of(from_a.session()), sequence_of(from_b.session()));
    refuse_stale(&mut b, from_a);
    refuse_stale(&mut a, from_b);

    assert_eq!(a.answer(from_a, V, &[1, 2], &[1, 3]), Ok(()));
    assert_eq!(b.requeue(from_b), Ok(()));

    let mut twin = Survey::<2, 2>::with_scope(1);
    diverge(&mut twin, shape);
    assert_eq!(twin.next_request(), Some(from_a));
    assert_eq!(twin.answer(from_a, V, &[1, 2], &[1, 3]), Ok(()));
}

/// `session` is the id most recently issued, never a rebuilt non-id: nothing
/// before the first `begin`, whatever the scope, and nothing after a restart
/// from a count of zero. Sessions count from one; `new` is scope zero.
#[test]
fn sessions_count_from_one_and_are_nothing_until_one_was_begun() {
    assert_eq!(Survey::<1, 1>::new(), Survey::with_scope(0));
    assert_eq!(Survey::<1, 1>::default(), Survey::with_scope(0));
    assert_eq!(Survey::<1, 1>::resumed(7, 0), Some(Survey::with_scope(7)));
    assert_ne!(Survey::<1, 1>::with_scope(1), Survey::with_scope(2));
    let shape = schema(2, 1);
    let deep = schema(2, 5);
    for scope in [0, 7, u16::MAX] {
        let mut survey = Survey::<1, 1>::with_scope(scope);
        assert_eq!((survey.scope(), survey.session()), (scope, None));
        assert_eq!(survey.next_request(), None);
        assert_eq!(survey.abort(), Aborting::AlreadyIdle);
        // A refused begin issues nothing, so there is still no session.
        let refused = survey.begin(deep, deep, V, &1, &2);
        assert_eq!(
            (refused, survey.session()),
            (Err(Refusal::Undersized), None)
        );
        diverge(&mut survey, shape);
        assert_eq!(survey.session(), Some(scoped(scope, 1)));
        let request = must(survey.next_request());
        assert_eq!(Request::new(scoped(scope, 1), Node::ROOT), Some(request));
        assert_eq!(request.scope(), scope);
        // Aborted: idle again, and it still answers the latest session issued.
        assert_eq!(survey.abort(), Aborting::Aborted);
        assert_eq!(
            (survey.phase(), survey.session()),
            (Phase::Idle, Some(scoped(scope, 1)))
        );
        assert_eq!(survey.begin(shape, shape, V, &1, &1), Ok(Begun::Identical));
        assert_eq!(survey.session(), Some(scoped(scope, 2)));
    }
    let fresh = must(Survey::<1, 1>::resumed(7, 0));
    assert_eq!((fresh.phase(), fresh.session()), (Phase::Idle, None));
    let idle = must(Survey::<1, 1>::resumed(7, 9));
    assert_eq!(
        (idle.phase(), idle.session()),
        (Phase::Idle, Some(scoped(7, 9)))
    );
}

#[test]
fn a_session_of_this_scope_that_was_never_issued_is_stale() {
    let mut survey = Survey::<1, 1>::with_scope(3);
    diverge(&mut survey, schema(2, 1));
    let live = must(survey.next_request());
    // Sequence zero is no id at all: there is no request to build.
    assert_eq!(Request::new(scoped(3, 0), live.node()), None);
    assert_eq!(Request::new(0, live.node()), None);
    for session in [scoped(3, 2), scoped(4, 1), 1] {
        refuse_stale(&mut survey, must(Request::new(session, live.node())));
    }
}

#[test]
fn the_last_session_of_a_scope_is_issued_and_then_begin_is_exhausted() {
    let shape = schema(2, 1);
    let mut survey = must(Survey::<1, 1>::resumed(3, SEQUENCE_MAX - 1));
    // A call refused for another reason spends no session id.
    let deep = schema(2, 5);
    refuses(&mut survey, Refusal::Undersized, |s| {
        s.begin(deep, deep, V, &1, &2)
    });
    diverge(&mut survey, shape);
    assert_eq!(survey.session(), Some(scoped(3, SEQUENCE_MAX)));
    let last = must(survey.next_request());
    assert_eq!((last.session(), last.scope()), (scoped(3, SEQUENCE_MAX), 3));
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Err(Refusal::Busy));
    // The last session still runs to its end.
    assert_eq!(survey.answer(last, V, &[1, 2], &[1, 3]), Ok(()));
    assert_eq!(survey.pop_range(), Some(range(1, 2)));
    assert_eq!(survey.phase(), Phase::Complete);
    for aborting in [Aborting::Aborted, Aborting::AlreadyIdle] {
        refuses(&mut survey, Refusal::Exhausted, |s| {
            s.begin(shape, shape, V, &1, &2)
        });
        assert_eq!(survey.abort(), aborting);
    }
    refuse_stale(&mut survey, last);

    // Resumed at the last sequence: spent; one past it is not a state.
    let mut spent = must(Survey::<1, 1>::resumed(3, SEQUENCE_MAX));
    assert_eq!(
        (spent.scope(), spent.issued(), spent.phase()),
        (3, SEQUENCE_MAX, Phase::Idle)
    );
    refuses(&mut spent, Refusal::Exhausted, |s| {
        s.begin(shape, shape, V, &1, &2)
    });
    assert_eq!(Survey::<1, 1>::resumed(3, SEQUENCE_MAX + 1), None);
    assert_eq!(Survey::<1, 1>::resumed(3, u64::MAX), None);
}

/// A fresh survey would open session 1 again, and a request that outlived the
/// restart would name a node in flight in the new session.
#[test]
fn a_resumed_survey_continues_its_scope_and_refuses_every_earlier_request() {
    let shape = schema(2, 2);
    let mut first = Survey::<2, 2>::with_scope(5);
    diverge(&mut first, shape);
    let aborted = must(first.next_request());
    assert_eq!(first.abort(), Aborting::Aborted);
    assert_eq!(first.begin(shape, shape, V, &1, &1), Ok(Begun::Identical));
    let identical = must(Request::new(must(first.session()), Node::ROOT));
    diverge(&mut first, shape);
    let open = must(first.next_request());
    assert_eq!((first.issued(), first.session()), (3, Some(scoped(5, 3))));
    let earlier = [aborted, identical, open];
    let refuse_earlier = |survey: &mut Survey<2, 2>| {
        for request in earlier {
            refuse_stale(survey, request);
        }
    };

    let mut survey = must(Survey::<2, 2>::resumed(first.scope(), first.issued()));
    assert_eq!((survey.scope(), survey.issued()), (5, 3));
    // Only the sequence came back: no session is open, not even the last one.
    assert_eq!(
        (survey.phase(), survey.session()),
        (Phase::Idle, Some(open.session()))
    );
    assert_eq!((survey.schema(), survey.versions()), (None, None));
    assert_eq!((survey.pending(), survey.buffered()), (0, 0));
    refuse_earlier(&mut survey);
    assert_eq!(survey.abort(), Aborting::AlreadyIdle);

    diverge(&mut survey, shape);
    assert_eq!((survey.issued(), survey.session()), (4, Some(scoped(5, 4))));
    let next = must(survey.next_request());
    // The same node is in flight again, under an id no earlier request has.
    assert_eq!((next.session(), next.node()), (scoped(5, 4), open.node()));
    refuse_earlier(&mut survey);
    assert_eq!(survey.answer(next, V, &[1, 2], &[1, 3]), Ok(()));
}
