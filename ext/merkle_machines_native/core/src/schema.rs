//! Tree shape, node addresses and leaf ranges.

/// Largest supported number of children per node.
pub const MAX_FANOUT: u8 = 64;

/// Half-open range of leaf indices, `start..end`, never backwards.
///
/// `LeafRange::default()` is the empty range `0..0`, a placeholder for
/// pre-filling an output buffer; a survey never emits an empty range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeafRange {
    // Crate-visible so the survey's sorted buffer can join ranges in place;
    // it only ever widens a range, so `start <= end` holds.
    pub(crate) start: u64,
    pub(crate) end: u64,
}

impl LeafRange {
    /// The leaves `start..end`, e.g. rebuilt after crossing a wire.
    ///
    /// `None` when `start > end`: no range runs backwards.
    #[must_use]
    pub const fn new(start: u64, end: u64) -> Option<Self> {
        if start > end {
            return None;
        }
        Some(Self { start, end })
    }

    /// First leaf in the range.
    #[must_use]
    pub const fn start(&self) -> u64 {
        self.start
    }

    /// One past the last leaf in the range.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.end
    }

    /// Number of leaves covered.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }

    /// `true` when the range covers no leaf.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.end == self.start
    }
}

/// Address of a tree node: `level` 0 is the root, `index` counts left to right
/// within the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    /// Distance from the root.
    pub level: u8,
    /// Position within the level.
    pub index: u64,
}

impl Node {
    /// The root of every tree.
    pub const ROOT: Self = Self { level: 0, index: 0 };
}

/// First field on which two schemas disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// Different number of children per node.
    Fanout,
    /// Different number of levels below the root.
    Depth,
    /// Different key-to-leaf partitioning.
    Partitioning,
    /// Different digest algorithm or encoding.
    Digest,
}

/// Shape and identity of a fixed-depth digest tree.
///
/// `partitioning` and `digest` are opaque identifiers chosen by the caller for
/// the key-to-leaf mapping and for the digest algorithm. Two trees can be
/// compared only when all four fields are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schema {
    fanout: u8,
    depth: u8,
    partitioning: u64,
    digest: u64,
}

impl Schema {
    /// A schema with `fanout` children per node and `depth` levels below the
    /// root. `None` when `fanout` is outside `2..=MAX_FANOUT` or the leaf count
    /// `fanout^depth` does not fit in a `u64`.
    #[must_use]
    pub const fn new(fanout: u8, depth: u8, partitioning: u64, digest: u64) -> Option<Self> {
        if fanout < 2 || fanout > MAX_FANOUT {
            return None;
        }
        if (fanout as u64).checked_pow(depth as u32).is_none() {
            return None;
        }
        Some(Self {
            fanout,
            depth,
            partitioning,
            digest,
        })
    }

    /// Children per node.
    #[must_use]
    pub const fn fanout(&self) -> u8 {
        self.fanout
    }

    /// Levels below the root; leaves live at this level.
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Caller-defined identifier of the key-to-leaf mapping.
    #[must_use]
    pub const fn partitioning(&self) -> u64 {
        self.partitioning
    }

    /// Caller-defined identifier of the digest algorithm.
    #[must_use]
    pub const fn digest(&self) -> u64 {
        self.digest
    }

    /// Total number of leaves.
    #[must_use]
    pub const fn leaves(&self) -> u64 {
        self.width(self.depth)
    }

    /// Number of nodes at `level`, saturating past the leaf level.
    #[must_use]
    pub const fn width(&self, level: u8) -> u64 {
        let level = if level > self.depth {
            self.depth
        } else {
            level
        };
        (self.fanout as u64).pow(level as u32)
    }

    /// Leaves below `node`, or `None` when the node is not part of the tree.
    #[must_use]
    pub const fn span(&self, node: Node) -> Option<LeafRange> {
        if node.level > self.depth || node.index >= self.width(node.level) {
            return None;
        }
        let each = self.width(self.depth - node.level);
        Some(LeafRange {
            start: node.index * each,
            end: (node.index + 1) * each,
        })
    }

    /// `Ok` when `other` describes a tree comparable with this one.
    ///
    /// # Errors
    /// The first field on which the schemas differ.
    pub const fn compatible(&self, other: &Self) -> Result<(), Mismatch> {
        if self.fanout != other.fanout {
            return Err(Mismatch::Fanout);
        }
        if self.depth != other.depth {
            return Err(Mismatch::Depth);
        }
        if self.partitioning != other.partitioning {
            return Err(Mismatch::Partitioning);
        }
        if self.digest != other.digest {
            return Err(Mismatch::Digest);
        }
        Ok(())
    }
}
