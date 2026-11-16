//! The survey's sorted leaf-range buffer.

use super::Survey;
use crate::schema::LeafRange;

impl<const P: usize, const O: usize> Survey<P, O> {
    /// Insert a run of leaves into the sorted buffer, joining touching ranges.
    ///
    /// `false` when it does not fit: the buffer is full and nothing changed.
    /// Every cell touched lies in the live prefix, or (inserting) in
    /// `at..=buffered` with `buffered < O`.
    pub(super) fn buffer(&mut self, start: u64, end: u64) -> bool {
        let Some(live) = self.out.get_mut(..self.buffered) else {
            debug_assert!(false, "buffered never exceeds O");
            return false;
        };
        let at = live
            .iter()
            .position(|r| r.start > start)
            .unwrap_or(live.len());
        // `at <= live.len()`, so the split always succeeds.
        let Some((before, after)) = live.split_at_mut_checked(at) else {
            debug_assert!(false, "the insertion point lies within the live prefix");
            return false;
        };
        let left = before.last_mut().filter(|r| r.end == start);
        let right = after.first_mut().filter(|r| r.start == end);
        match (left, right) {
            (Some(left), Some(right)) => {
                left.end = right.end;
                // `right` is `live[at]`, so `at + 1 <= live.len()` and
                // `buffered >= 1`.
                live.copy_within(at + 1.., at);
                self.buffered -= 1;
            }
            (Some(left), None) => left.end = end,
            (None, Some(right)) => right.start = start,
            (None, None) => {
                if self.buffered == O {
                    return false;
                }
                // `at <= buffered < O`: the window is in bounds and nonempty.
                // Rotating moves the unused cell at `buffered` to the front,
                // shifting `at..buffered` up by one, then the run takes it.
                let Some(window) = self.out.get_mut(at..=self.buffered) else {
                    debug_assert!(false, "buffered stays below O here");
                    return false;
                };
                window.rotate_right(1);
                let Some(first) = window.first_mut() else {
                    debug_assert!(false, "the window holds at least one cell");
                    return false;
                };
                *first = LeafRange { start, end };
                // `buffered < O`, so this cannot overflow.
                self.buffered += 1;
            }
        }
        true
    }
}
