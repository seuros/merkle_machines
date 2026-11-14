//! Thin binding to the bounded digest-tree survey.
mod init;
mod values;

use values::{build, classes};

use std::{cell::RefCell, cmp::Ordering};

use magnus::{
    Error, Integer, RArray, RString, Range, Ruby, Symbol, TryConvert, Value, prelude::*,
    typed_data::Obj,
};
use merkle_machines::{
    Aborting, Begun, LeafRange, Mismatch, Node, Phase, Refusal, Request, Schema, Versions,
};

const PENDING_CAPACITY: usize = 64;
const OUTPUT_CAPACITY: usize = 64;
const MAX_FANOUT: u8 = merkle_machines::MAX_FANOUT;

fn type_error(value: Value, message: &'static str) -> Error {
    Error::new(Ruby::get_with(value).exception_type_error(), message)
}

fn integer(value: Value) -> Result<Value, Error> {
    if Integer::from_value(value).is_none() {
        return Err(type_error(value, "expected an Integer"));
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

/// An issuer scope: an Integer in `0..=65535`.
#[derive(Clone, Copy)]
struct Scope(u16);

impl TryConvert for Scope {
    fn try_convert(value: Value) -> Result<Self, Error> {
        u16::try_convert(integer(value)?).map(Self)
    }
}

/// A caller-supplied digest: a Ruby String compared byte for byte.
#[derive(Clone, Copy)]
struct Digest(RString);

impl TryConvert for Digest {
    fn try_convert(value: Value) -> Result<Self, Error> {
        RString::from_value(value)
            .map(Self)
            .ok_or_else(|| type_error(value, "expected a String digest"))
    }
}

impl PartialEq for Digest {
    fn eq(&self, other: &Self) -> bool {
        self.0.cmp(other.0) == Ordering::Equal
    }
}

fn digests(list: Value) -> Result<Vec<Digest>, Error> {
    let Some(list) = RArray::from_value(list) else {
        return Err(type_error(list, "expected an Array of String digests"));
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

fn schema_span(ruby: &Ruby, shape: Shape, node: Place) -> Result<Option<Range>, Error> {
    schema(ruby, shape)?
        .span(self::node(node))
        .map(|span| leaves(ruby, span))
        .transpose()
}

fn schema_compatible(ruby: &Ruby, shape: Shape, other: Shape) -> Result<Symbol, Error> {
    let compatible = schema(ruby, shape)?.compatible(&schema(ruby, other)?);
    Ok(ruby.to_symbol(match compatible {
        Ok(()) => "compatible",
        Err(reason) => mismatch(reason),
    }))
}

/// A leaf range as the half-open Ruby `Range` `start...end`.
fn leaves(ruby: &Ruby, range: LeafRange) -> Result<Range, Error> {
    ruby.range_new(range.start(), range.end(), true)
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

const fn node(place: Place) -> Node {
    Node {
        level: place.0.0,
        index: place.1.0,
    }
}

/// The core's `Request::new`: a session of sequence zero names no session, so
/// no request exists for it.
fn request(ruby: &Ruby, ticket: Ticket) -> Result<Request, Error> {
    Request::new(ticket.0.0, node((ticket.1, ticket.2))).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "session has sequence number zero, which no survey issues",
        )
    })
}

/// `Node.new`: every field converts to the core's type or raises.
const fn check_node(_level: Small, _index: Number) {}

/// `Versions.new`: every field converts to the core's type or raises.
const fn check_versions(_local: Number, _remote: Number) {}

/// `Request.new`: raises exactly when the core has no request for it.
fn check_request(ruby: &Ruby, session: Number, level: Small, index: Number) -> Result<(), Error> {
    request(ruby, (session, level, index)).map(|_| ())
}

/// `Request#scope`: the core's `Request::scope`, so the id layout stays the
/// core's business.
fn request_scope(ruby: &Ruby, session: Number) -> Result<u16, Error> {
    Ok(request(ruby, (session, Small(0), Number(0)))?.scope())
}

type Core = merkle_machines::Survey<PENDING_CAPACITY, OUTPUT_CAPACITY>;

/// Runs `act`, which builds a Ruby answer, on a copy of the survey and keeps
/// the copy only once the answer exists. The receiver is checked frozen up
/// front and again right before the store; frozen meanwhile raises
/// `FrozenError`, changed meanwhile (a call back in while the answer was
/// built) raises `RuntimeError`, and in both cases nothing is applied. No
/// borrow is held while `act` runs.
fn on_copy<R>(
    ruby: &Ruby,
    this: Obj<Survey>,
    act: impl FnOnce(&mut Core) -> Result<R, Error>,
) -> Result<R, Error> {
    this.check_frozen()?;
    let before = *this.core.borrow();
    let mut after = before;
    let answer = act(&mut after)?;
    this.check_frozen()?;
    let mut held = this.core.borrow_mut();
    if *held != before {
        return Err(Error::new(
            ruby.exception_runtime_error(),
            "the survey changed while the answer was built; the call was not applied",
        ));
    }
    *held = after;
    Ok(answer)
}

#[magnus::wrap(class = "MerkleMachines::Survey", free_immediately, size)]
struct Survey {
    core: RefCell<Core>,
}

impl Survey {
    const fn new() -> Self {
        Self {
            core: RefCell::new(merkle_machines::Survey::new()),
        }
    }

    const fn with_scope(scope: Scope) -> Self {
        Self {
            core: RefCell::new(merkle_machines::Survey::with_scope(scope.0)),
        }
    }

    fn resumed(ruby: &Ruby, scope: Scope, issued: Number) -> Result<Self, Error> {
        let core = merkle_machines::Survey::resumed(scope.0, issued.0).ok_or_else(|| {
            Error::new(
                ruby.exception_range_error(),
                "issued is past the last sequence of a scope (2**48 - 1)",
            )
        })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn scope(&self) -> u16 {
        self.core.borrow().scope()
    }

    fn issued(&self) -> u64 {
        self.core.borrow().issued()
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

    /// Decided on a copy, kept once the `Request` is built (see [`on_copy`]).
    fn next_request(ruby: &Ruby, this: Obj<Self>) -> Result<Option<Value>, Error> {
        let classes = classes(ruby)?;
        on_copy(ruby, this, |core| {
            let Some(request) = core.next_request() else {
                return Ok(None);
            };
            let Node { level, index } = request.node();
            let node = build(
                classes.node,
                [
                    ruby.integer_from_u64(level.into()).as_value(),
                    ruby.integer_from_u64(index).as_value(),
                ],
            )?;
            let session = ruby.integer_from_u64(request.session()).as_value();
            build(classes.request, [session, node]).map(Some)
        })
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
        let request = request(ruby, ticket)?;
        let (local, remote) = (digests(local)?, digests(remote)?);
        let answered = this
            .core
            .borrow_mut()
            .answer(request, versions(pins), &local, &remote);
        Ok(ruby.to_symbol(answered.map_or_else(refusal, |()| "accepted")))
    }

    fn requeue(ruby: &Ruby, this: Obj<Self>, ticket: Ticket) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let request = request(ruby, ticket)?;
        let requeued = this.core.borrow_mut().requeue(request);
        Ok(ruby.to_symbol(requeued.map_or_else(refusal, |()| "requeued")))
    }

    fn pop_range(ruby: &Ruby, this: Obj<Self>) -> Result<Option<Range>, Error> {
        on_copy(ruby, this, |core| {
            core.pop_range()
                .map(|range| leaves(ruby, range))
                .transpose()
        })
    }

    /// The survey never buffers more than `OUTPUT_CAPACITY` ranges, so a
    /// larger limit takes everything and loses nothing. The ranges are read
    /// into a buffer, then built, inside one [`on_copy`].
    fn drain(ruby: &Ruby, this: Obj<Self>, limit: Number) -> Result<RArray, Error> {
        let limit = usize::try_from(limit.0).map_or(OUTPUT_CAPACITY, |n| n.min(OUTPUT_CAPACITY));
        on_copy(ruby, this, |core| {
            let mut taken = [LeafRange::default(); OUTPUT_CAPACITY];
            let count = core.drain(&mut taken[..limit]);
            let ranges = ruby.ary_new_capa(count);
            for &range in &taken[..count] {
                ranges.push(leaves(ruby, range)?)?;
            }
            Ok(ranges)
        })
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

    fn session(&self) -> Option<u64> {
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
