//! Thin binding to the bounded digest-tree survey.
mod init;

use std::{cell::RefCell, cmp::Ordering};

use magnus::{
    Error, Integer, RArray, RString, Ruby, Symbol, TryConvert, Value, prelude::*, typed_data::Obj,
};
use merkle_machines::{
    Begun, LeafRange, Mismatch, Node, Phase, Refusal, Request, Schema, Versions,
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

const fn refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Schema(Mismatch::Fanout) => "fanout_mismatch",
        Refusal::Schema(Mismatch::Depth) => "depth_mismatch",
        Refusal::Schema(Mismatch::Partitioning) => "partitioning_mismatch",
        Refusal::Schema(Mismatch::Digest) => "digest_mismatch",
        Refusal::Undersized => "undersized",
        Refusal::Busy => "busy",
        Refusal::Exhausted => "exhausted",
        Refusal::Stale => "stale",
        Refusal::Unexpected => "unexpected",
        Refusal::VersionChanged => "version_changed",
        Refusal::Arity => "arity",
        Refusal::Inconsistent => "inconsistent",
        _ => "refused",
    }
}

type Shape = (Small, Small, Number, Number);
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
        let shape = |(fanout, depth, partitioning, digest): Shape| {
            Schema::new(fanout.0, depth.0, partitioning.0, digest.0)
        };
        let (Some(local), Some(remote)) = (shape(local), shape(remote)) else {
            return Ok(ruby.to_symbol("invalid_schema"));
        };
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

    fn requeue(this: Obj<Self>, ticket: Ticket) -> Result<bool, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().requeue(request(ticket)))
    }

    fn take_ranges(this: Obj<Self>, limit: Number) -> Result<Vec<(u64, u64)>, Error> {
        this.check_frozen()?;
        let mut taken = [LeafRange { start: 0, end: 0 }; OUTPUT_CAPACITY];
        let limit = usize::try_from(limit.0).map_or(OUTPUT_CAPACITY, |n| n.min(OUTPUT_CAPACITY));
        let count = this.core.borrow_mut().drain(&mut taken[..limit]);
        Ok(taken[..count].iter().map(|r| (r.start, r.end)).collect())
    }

    fn requeue_all(this: Obj<Self>) -> Result<usize, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().requeue_all())
    }

    fn abort(this: Obj<Self>) -> Result<bool, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().abort())
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
