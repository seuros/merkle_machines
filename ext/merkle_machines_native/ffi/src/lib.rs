//! Thin binding to the bounded digest-tree survey.
mod init;

use std::{cell::RefCell, cmp::Ordering};

use magnus::{
    Error, Integer, RArray, RString, Ruby, Symbol, TryConvert, Value, prelude::*, typed_data::Obj,
};
use merkle_machines::{
    Aborting, Begun, LeafRange, Mismatch, Node, Phase, Refusal, Request, Schema, Versions,
};

const PENDING_CAPACITY: usize = 64;
const OUTPUT_CAPACITY: usize = 64;
const MAX_FANOUT: u8 = merkle_machines::MAX_FANOUT;

fn integer(value: Value) -> Result<Value, Error> {
    if Integer::from_value(value).is_none() {
        return Err(Error::new(
            Ruby::get_with(value).exception_type_error(),
            "expected an Integer",
        ));
    }
    Ok(value)
}

#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        u64::try_convert(integer(value)?).map(Self)
    }
}

#[derive(Clone, Copy)]
struct Small(u8);

impl TryConvert for Small {
    fn try_convert(value: Value) -> Result<Self, Error> {
        u8::try_convert(integer(value)?).map(Self)
    }
}

/// A caller-supplied digest: a Ruby String compared byte for byte.
#[derive(Clone, Copy)]
struct Digest(RString);

impl TryConvert for Digest {
    fn try_convert(value: Value) -> Result<Self, Error> {
        RString::from_value(value).map(Self).ok_or_else(|| {
            Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected a String digest",
            )
        })
    }
}

impl PartialEq for Digest {
    fn eq(&self, other: &Self) -> bool {
        self.0.cmp(other.0) == Ordering::Equal
    }
}

fn digests(list: Value) -> Result<Vec<Digest>, Error> {
    let Some(list) = RArray::from_value(list) else {
        return Err(Error::new(
            Ruby::get_with(list).exception_type_error(),
            "expected an Array of String digests",
        ));
    };
    list.into_iter().map(Digest::try_convert).collect()
}

/// One symbol per `Mismatch`, shared by `Schema#compatible` and `Survey#begin`.
const fn mismatch(mismatch: Mismatch) -> &'static str {
    match mismatch {
        Mismatch::Fanout => "fanout_mismatch",
        Mismatch::Depth => "depth_mismatch",
        Mismatch::Partitioning => "partitioning_mismatch",
        Mismatch::Digest => "digest_mismatch",
    }
}

const fn refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Schema(reason) => mismatch(reason),
        Refusal::Undersized => "undersized",
        Refusal::Busy => "busy",
        Refusal::Exhausted => "exhausted",
        Refusal::Stale => "stale",
        Refusal::Unexpected => "unexpected",
        Refusal::VersionChanged => "version_changed",
        Refusal::Arity => "arity",
        Refusal::Inconsistent => "inconsistent",
    }
}

type Shape = (Small, Small, Number, Number);

const INVALID_SCHEMA: &str =
    "fanout must be in 2..=MAX_FANOUT and fanout**depth must fit in 64 bits";

/// The core's `Schema::new`, which Ruby's `Schema.new` answers for: a shape
/// the core has no value for is malformed input, not a refusal.
fn schema(ruby: &Ruby, (fanout, depth, partitioning, digest): Shape) -> Result<Schema, Error> {
    Schema::new(fanout.0, depth.0, partitioning.0, digest.0)
        .ok_or_else(|| Error::new(ruby.exception_range_error(), INVALID_SCHEMA))
}

/// Raises exactly when the core cannot build this schema.
fn check_schema(
    ruby: &Ruby,
    fanout: Small,
    depth: Small,
    partitioning: Number,
    digest: Number,
) -> Result<(), Error> {
    schema(ruby, (fanout, depth, partitioning, digest)).map(|_| ())
}

fn schema_leaves(ruby: &Ruby, shape: Shape) -> Result<u64, Error> {
    Ok(schema(ruby, shape)?.leaves())
}

fn schema_width(ruby: &Ruby, shape: Shape, level: Small) -> Result<u64, Error> {
    Ok(schema(ruby, shape)?.width(level.0))
}

fn schema_span(ruby: &Ruby, shape: Shape, node: Place) -> Result<Option<(u64, u64)>, Error> {
    let node = Node {
        level: node.0.0,
        index: node.1.0,
    };
    Ok(schema(ruby, shape)?.span(node).map(bounds))
}

fn schema_compatible(ruby: &Ruby, shape: Shape, other: Shape) -> Result<Symbol, Error> {
    let compatible = schema(ruby, shape)?.compatible(&schema(ruby, other)?);
    Ok(ruby.to_symbol(match compatible {
        Ok(()) => "compatible",
        Err(reason) => mismatch(reason),
    }))
}

const fn bounds(range: LeafRange) -> (u64, u64) {
    (range.start, range.end)
}

type Place = (Small, Number);
type Pins = (Number, Number);
type Ticket = (Number, Small, Number);

const fn versions(pins: Pins) -> Versions {
    Versions {
        local: pins.0.0,
        remote: pins.1.0,
    }
}

const fn request(ticket: Ticket) -> Request {
    Request::new(
        ticket.0.0,
        Node {
            level: ticket.1.0,
            index: ticket.2.0,
        },
    )
}

#[magnus::wrap(class = "MerkleMachines::Survey", free_immediately, size)]
struct Survey {
    core: RefCell<merkle_machines::Survey<PENDING_CAPACITY, OUTPUT_CAPACITY>>,
}

impl Survey {
    const fn new() -> Self {
        Self {
            core: RefCell::new(merkle_machines::Survey::new()),
        }
    }

    fn begin(
        ruby: &Ruby,
        this: Obj<Self>,
        local: Shape,
        remote: Shape,
        pins: Pins,
        local_root: Digest,
        remote_root: Digest,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (local, remote) = (schema(ruby, local)?, schema(ruby, remote)?);
        let begun =
            this.core
                .borrow_mut()
                .begin(local, remote, versions(pins), &local_root, &remote_root);
        Ok(ruby.to_symbol(match begun {
            Ok(Begun::Identical) => "identical",
            Ok(Begun::Diverged) => "diverged",
            Err(refused) => refusal(refused),
        }))
    }

    fn next_request(this: Obj<Self>) -> Result<Option<(u64, u8, u64)>, Error> {
        this.check_frozen()?;
        let request = this.core.borrow_mut().next_request();
        Ok(request.map(|r| (r.session(), r.node().level, r.node().index)))
    }

    fn answer(
        ruby: &Ruby,
        this: Obj<Self>,
        ticket: Ticket,
        pins: Pins,
        local: Value,
        remote: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (local, remote) = (digests(local)?, digests(remote)?);
        let answered =
            this.core
                .borrow_mut()
                .answer(request(ticket), versions(pins), &local, &remote);
        Ok(ruby.to_symbol(answered.map_or_else(refusal, |()| "accepted")))
    }

    fn requeue(ruby: &Ruby, this: Obj<Self>, ticket: Ticket) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let requeued = this.core.borrow_mut().requeue(request(ticket));
        Ok(ruby.to_symbol(requeued.map_or_else(refusal, |()| "requeued")))
    }

    fn pop_range(this: Obj<Self>) -> Result<Option<(u64, u64)>, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().pop_range().map(bounds))
    }

    fn drain(this: Obj<Self>, limit: Number) -> Result<Vec<(u64, u64)>, Error> {
        this.check_frozen()?;
        let mut taken = [LeafRange::default(); OUTPUT_CAPACITY];
        let limit = usize::try_from(limit.0).map_or(OUTPUT_CAPACITY, |n| n.min(OUTPUT_CAPACITY));
        let count = this.core.borrow_mut().drain(&mut taken[..limit]);
        Ok(taken[..count].iter().copied().map(bounds).collect())
    }

    fn requeue_all(this: Obj<Self>) -> Result<usize, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().requeue_all())
    }

    fn abort(ruby: &Ruby, this: Obj<Self>) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let aborting = this.core.borrow_mut().abort();
        Ok(ruby.to_symbol(match aborting {
            Aborting::Aborted => "aborted",
            Aborting::AlreadyIdle => "already_idle",
        }))
    }

    fn phase(ruby: &Ruby, this: &Self) -> Symbol {
        ruby.to_symbol(match this.core.borrow().phase() {
            Phase::Idle => "idle",
            Phase::Surveying => "surveying",
            Phase::Draining => "draining",
            Phase::Complete => "complete",
        })
    }

    fn session(&self) -> u64 {
        self.core.borrow().session()
    }

    fn schema(&self) -> Option<(u8, u8, u64, u64)> {
        let schema = self.core.borrow().schema()?;
        Some((
            schema.fanout(),
            schema.depth(),
            schema.partitioning(),
            schema.digest(),
        ))
    }

    fn versions(&self) -> Option<(u64, u64)> {
        let versions = self.core.borrow().versions()?;
        Some((versions.local, versions.remote))
    }

    fn pending(&self) -> usize {
        self.core.borrow().pending()
    }

    fn in_flight(&self) -> usize {
        self.core.borrow().in_flight()
    }

    fn buffered(&self) -> usize {
        self.core.borrow().buffered()
    }
}
