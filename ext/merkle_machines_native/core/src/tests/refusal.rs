use super::*;

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
    assert_eq!(survey.abort(), Aborting::Aborted);
    assert_eq!(survey.abort(), Aborting::AlreadyIdle);
    assert_eq!(survey.phase(), Phase::Idle);

    survey.session = u64::MAX;
    let frozen = survey;
    assert_eq!(survey.begin(deep, deep, V, &1, &2), Err(Refusal::Exhausted));
    assert_eq!(survey, frozen);
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
    assert_eq!(survey.abort(), Aborting::Aborted);
    assert_eq!(
        survey.answer(child, V, &[1, 2], &[3, 4]),
        Err(Refusal::Stale)
    );
    assert_eq!(survey.begin(shape, shape, V, &1, &2), Ok(Begun::Diverged));
    assert_eq!(
        survey.answer(child, V, &[1, 2], &[3, 4]),
        Err(Refusal::Stale)
    );
    let frozen = survey;
    assert_eq!(survey.requeue(child), Err(Refusal::Stale));
    let fresh = must(survey.next_request());
    let frozen_flying = survey;
    let never = Request::new(fresh.session(), child.node());
    assert_eq!(survey.requeue(never), Err(Refusal::Unexpected));
    assert_eq!(survey, frozen_flying);
    assert_eq!(survey.requeue(fresh), Ok(()));
    assert_eq!(survey, frozen);
    assert_eq!(survey.requeue(fresh), Err(Refusal::Unexpected));
    assert_eq!(survey, frozen);
    assert_eq!(survey.abort(), Aborting::Aborted);
    let idle = survey;
    assert_eq!(survey.requeue(fresh), Err(Refusal::Stale));
    assert_eq!(survey.abort(), Aborting::AlreadyIdle);
    assert_eq!(survey, idle);
}

#[test]
fn refusals_describe_themselves() {
    extern crate alloc;
    use alloc::string::ToString;
    let error: &dyn core::error::Error = &Refusal::Stale;
    assert_eq!(
        error.to_string(),
        "request does not belong to the open session"
    );
    assert_ne!(
        Refusal::Schema(Mismatch::Fanout).to_string(),
        Refusal::Schema(Mismatch::Depth).to_string()
    );
}
