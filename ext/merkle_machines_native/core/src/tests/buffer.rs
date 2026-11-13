use super::*;

#[test]
fn a_default_buffer_is_filled_from_the_front() {
    let shape = schema(2, 1);
    let mut survey = Survey::<2, 2>::new();
    assert_eq!(survey.begin(shape, shape, V, &0, &1), Ok(Begun::Diverged));
    let request = must(survey.next_request());
    assert_eq!(survey.answer(request, V, &[0, 0], &[1, 0]), Ok(()));
    let mut out = [LeafRange::default(); 3];
    assert!(out[0].is_empty());
    assert_eq!(survey.drain(&mut out), 1);
    assert_eq!(out[0], LeafRange { start: 0, end: 1 });
    assert_eq!(out[1..], [LeafRange::default(); 2]);
}
