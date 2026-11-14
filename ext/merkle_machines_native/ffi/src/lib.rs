//! Thin binding to the bounded digest-tree survey.
mod init;
mod values;

use values::{build, classes};

use std::{cell::RefCell, cmp::Ordering};

use magnus::{
    Error, RArray, RString, Range, Ruby, Symbol, TryConvert, Value, prelude::*, typed_data::Obj,
};
use matryona_machines::ruby::{exact_array, exact_fields, integer, keep, symbol};
use merkle_machines::{
    Aborting, Begun, LeafRange, Mismatch, Node, Phase, Refusal, Request, Schema, Versions,
};

const PENDING_CAPACITY: usize = 64;
const OUTPUT_CAPACITY: usize = 64;
const MAX_FANOUT: u8 = merkle_machines::MAX_FANOUT;

fn type_error(value: Value, message: &'static str) -> Error {
    Error::new(Ruby::get_with(value).exception_type_error(), message)
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

/// Read natively from exactly an `Array`: a subclass is a `TypeError`.
fn digests(ruby: &Ruby, list: Value) -> Result<Vec<Digest>, Error> {
    let list = exact_array(ruby, list, "expected an Array of String digests")?;
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

/// A `MerkleMachines::Schema`, as it arrives.
type Shape = Value;

/// A schema's fields as the core's types.
type Fields = (u8, u8, u64, u64);

/// Reads every field of `shape`, exactly a `Schema`, from the struct; the
/// core's `Schema::new` checks them after.
fn shape_of(ruby: &Ruby, shape: Shape) -> Result<Fields, Error> {
    let class = classes(ruby)?.schema.get(ruby);
    let [fanout, depth, partitioning, digest] =
        exact_fields(ruby, shape, class, "expected a MerkleMachines::Schema")?;
    fields_of(fanout, depth, partitioning, digest)
}

fn fields_of(
    fanout: Value,
    depth: Value,
    partitioning: Value,
    digest: Value,
) -> Result<Fields, Error> {
    Ok((
        integer(fanout, "fanout")?,
        integer(depth, "depth")?,
        integer(partitioning, "partitioning")?,
        integer(digest, "digest")?,
    ))
}

const INVALID_SCHEMA: &str =
    "fanout must be in 2..=MAX_FANOUT and fanout**depth must fit in 64 bits";

/// The core's `Schema::new`, which Ruby's `Schema.new` answers for: a shape
/// the core has no value for is malformed input, not a refusal.
fn schema(ruby: &Ruby, (fanout, depth, partitioning, digest): Fields) -> Result<Schema, Error> {
    Schema::new(fanout, depth, partitioning, digest)
        .ok_or_else(|| Error::new(ruby.exception_range_error(), INVALID_SCHEMA))
}

/// Raises exactly when the core cannot build this schema.
fn check_schema(
    ruby: &Ruby,
    fanout: Value,
    depth: Value,
    partitioning: Value,
    digest: Value,
) -> Result<(), Error> {
    let shape = fields_of(fanout, depth, partitioning, digest)?;
    schema(ruby, shape).map(drop)
}

fn schema_leaves(ruby: &Ruby, shape: Shape) -> Result<u64, Error> {
    Ok(schema(ruby, shape_of(ruby, shape)?)?.leaves())
}

fn schema_width(ruby: &Ruby, shape: Shape, level: Value) -> Result<u64, Error> {
    let (shape, level) = (shape_of(ruby, shape)?, integer(level, "level")?);
    Ok(schema(ruby, shape)?.width(level))
}

fn schema_span(ruby: &Ruby, shape: Shape, node: Place) -> Result<Option<Range>, Error> {
    let (shape, node) = (shape_of(ruby, shape)?, self::node(ruby, node)?);
    schema(ruby, shape)?
        .span(node)
        .map(|span| leaves(ruby, span))
        .transpose()
}

fn schema_compatible(ruby: &Ruby, shape: Shape, other: Shape) -> Result<Symbol, Error> {
    let (shape, other) = (shape_of(ruby, shape)?, shape_of(ruby, other)?);
    let compatible = schema(ruby, shape)?.compatible(&schema(ruby, other)?);
    Ok(symbol(
        ruby,
        match compatible {
            Ok(()) => "compatible",
            Err(reason) => mismatch(reason),
        },
    ))
}

/// A leaf range as the half-open Ruby `Range` `start...end`.
fn leaves(ruby: &Ruby, range: LeafRange) -> Result<Range, Error> {
    ruby.range_new(range.start(), range.end(), true)
}

/// A `MerkleMachines::Node`, as it arrives.
type Place = Value;
/// A `MerkleMachines::Versions`, as it arrives.
type Pins = Value;
/// A `MerkleMachines::Request`, as it arrives.
type Ticket = Value;

fn versions(ruby: &Ruby, pins: Pins) -> Result<Versions, Error> {
    let class = classes(ruby)?.versions.get(ruby);
    let [local, remote] = exact_fields(ruby, pins, class, "expected a MerkleMachines::Versions")?;
    versions_of(local, remote)
}

fn versions_of(local: Value, remote: Value) -> Result<Versions, Error> {
    Ok(Versions {
        local: integer(local, "local")?,
        remote: integer(remote, "remote")?,
    })
}

fn node(ruby: &Ruby, place: Place) -> Result<Node, Error> {
    let class = classes(ruby)?.node.get(ruby);
    let [level, index] = exact_fields(ruby, place, class, "expected a MerkleMachines::Node")?;
    node_of(level, index)
}

fn node_of(level: Value, index: Value) -> Result<Node, Error> {
    Ok(Node {
        level: integer(level, "level")?,
        index: integer(index, "index")?,
    })
}

/// A request's session and node, read before the core is asked for it.
fn ticket(ruby: &Ruby, ticket: Ticket) -> Result<(u64, Node), Error> {
    let class = classes(ruby)?.request.get(ruby);
    let [session, node] = exact_fields(ruby, ticket, class, "expected a MerkleMachines::Request")?;
    ticket_of(ruby, session, node)
}

fn ticket_of(ruby: &Ruby, session: Value, node: Place) -> Result<(u64, Node), Error> {
    Ok((integer(session, "session")?, self::node(ruby, node)?))
}

/// The core's `Request::new`: a session of sequence zero names no session, so
/// no request exists for it.
fn request(ruby: &Ruby, (session, node): (u64, Node)) -> Result<Request, Error> {
    Request::new(session, node).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "session has sequence number zero, which no survey issues",
        )
    })
}

/// `Node.new`: every field converts to the core's type or raises.
fn check_node(level: Value, index: Value) -> Result<(), Error> {
    node_of(level, index).map(drop)
}

/// `Versions.new`: every field converts to the core's type or raises.
fn check_versions(local: Value, remote: Value) -> Result<(), Error> {
    versions_of(local, remote).map(drop)
}

/// `Request.new`: raises exactly when the core has no request for it, or
/// `node` is not exactly a `Node`.
fn check_request(ruby: &Ruby, session: Value, node: Place) -> Result<(), Error> {
    request(ruby, ticket_of(ruby, session, node)?).map(drop)
}

/// `Request#scope`: the core's `Request::scope`, so the id layout stays the
/// core's business.
fn request_scope(ruby: &Ruby, session: Value) -> Result<u16, Error> {
    let root = Node { level: 0, index: 0 };
    Ok(request(ruby, (integer(session, "session")?, root))?.scope())
}

type Core = merkle_machines::Survey<PENDING_CAPACITY, OUTPUT_CAPACITY>;

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

    fn with_scope(scope: Value) -> Result<Self, Error> {
        Ok(Self {
            core: RefCell::new(merkle_machines::Survey::with_scope(integer(
                scope, "scope",
            )?)),
        })
    }

    fn resumed(ruby: &Ruby, scope: Value, issued: Value) -> Result<Self, Error> {
        let (scope, issued) = (integer(scope, "scope")?, integer(issued, "issued")?);
        let core = merkle_machines::Survey::resumed(scope, issued).ok_or_else(|| {
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
        local_root: Value,
        remote_root: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (local, remote) = (shape_of(ruby, local)?, shape_of(ruby, remote)?);
        let pins = versions(ruby, pins)?;
        let local_root = Digest::try_convert(local_root)?;
        let remote_root = Digest::try_convert(remote_root)?;
        let (local, remote) = (schema(ruby, local)?, schema(ruby, remote)?);
        let begun = this
            .core
            .borrow_mut()
            .begin(local, remote, pins, &local_root, &remote_root);
        Ok(symbol(
            ruby,
            match begun {
                Ok(Begun::Identical) => "identical",
                Ok(Begun::Diverged) => "diverged",
                Err(refused) => refusal(refused),
            },
        ))
    }

    /// Decided on a copy, kept once the `Request` is built (see [`keep`]).
    fn next_request(ruby: &Ruby, this: Obj<Self>) -> Result<Option<Value>, Error> {
        let classes = classes(ruby)?;
        keep(ruby, this, &this.core, Core::next_request, |request| {
            let Some(request) = request else {
                return Ok(None);
            };
            let Node { level, index } = request.node();
            let node = build(
                classes.node.get(ruby),
                [
                    ruby.integer_from_u64(level.into()).as_value(),
                    ruby.integer_from_u64(index).as_value(),
                ],
            )?;
            let session = ruby.integer_from_u64(request.session()).as_value();
            build(classes.request.get(ruby), [session, node]).map(Some)
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
        let (ticket, pins) = (self::ticket(ruby, ticket)?, versions(ruby, pins)?);
        let request = request(ruby, ticket)?;
        let (local, remote) = (digests(ruby, local)?, digests(ruby, remote)?);
        let answered = this
            .core
            .borrow_mut()
            .answer(request, pins, &local, &remote);
        Ok(symbol(ruby, answered.map_or_else(refusal, |()| "accepted")))
    }

    fn requeue(ruby: &Ruby, this: Obj<Self>, ticket: Ticket) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let ticket = self::ticket(ruby, ticket)?;
        let request = request(ruby, ticket)?;
        let requeued = this.core.borrow_mut().requeue(request);
        Ok(symbol(ruby, requeued.map_or_else(refusal, |()| "requeued")))
    }

    fn pop_range(ruby: &Ruby, this: Obj<Self>) -> Result<Option<Range>, Error> {
        keep(ruby, this, &this.core, Core::pop_range, |range| {
            range.map(|range| leaves(ruby, range)).transpose()
        })
    }

    /// The survey never buffers more than `OUTPUT_CAPACITY` ranges, so a
    /// larger limit takes everything and loses nothing. The ranges are read
    /// into a buffer on the copy, then built, inside one [`keep`].
    fn drain(ruby: &Ruby, this: Obj<Self>, limit: Value) -> Result<RArray, Error> {
        this.check_frozen()?;
        let limit: u64 = integer(limit, "limit")?;
        let limit = usize::try_from(limit).map_or(OUTPUT_CAPACITY, |n| n.min(OUTPUT_CAPACITY));
        keep(
            ruby,
            this,
            &this.core,
            |core| {
                let mut taken = [LeafRange::default(); OUTPUT_CAPACITY];
                let count = core.drain(&mut taken[..limit]);
                (taken, count)
            },
            |(taken, count)| {
                let ranges = ruby.ary_new_capa(count);
                for &range in &taken[..count] {
                    ranges.push(leaves(ruby, range)?)?;
                }
                Ok(ranges)
            },
        )
    }

    fn requeue_all(this: Obj<Self>) -> Result<usize, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().requeue_all())
    }

    fn abort(ruby: &Ruby, this: Obj<Self>) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let aborting = this.core.borrow_mut().abort();
        Ok(symbol(
            ruby,
            match aborting {
                Aborting::Aborted => "aborted",
                Aborting::AlreadyIdle => "already_idle",
            },
        ))
    }

    fn phase(ruby: &Ruby, this: &Self) -> Symbol {
        symbol(
            ruby,
            match this.core.borrow().phase() {
                Phase::Idle => "idle",
                Phase::Surveying => "surveying",
                Phase::Draining => "draining",
                Phase::Complete => "complete",
            },
        )
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
