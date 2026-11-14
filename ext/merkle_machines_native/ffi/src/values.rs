//! The value classes the binding reads and builds, resolved once at init.
//!
//! `lib/merkle_machines/values.rb` defines `Node`, `Schema`, `Versions` and
//! `Request` before this extension loads. Sealing them stops accidents, not
//! reflection, so [`fields`] is the authority on what Ruby hands in: an
//! instance of exactly the class asked for, whose members are read straight
//! from the `Data` struct (no reader runs) and then checked again by the core
//! constructor at each use.

use std::sync::OnceLock;

use magnus::rb_sys::AsRawValue;
use magnus::value::Opaque;
use magnus::{Error, RArray, RClass, RModule, RStruct, Ruby, TryConvert, Value, prelude::*};

/// `Node`, `Schema`, `Versions`, `Request`, in that order.
static CLASSES: OnceLock<[Opaque<RClass>; 4]> = OnceLock::new();

/// The classes as resolved at init.
#[derive(Clone, Copy)]
pub struct Classes {
    pub node: RClass,
    pub schema: RClass,
    pub versions: RClass,
    pub request: RClass,
}

pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name: &str| {
        let class: RClass = module.const_get(name)?;
        // Held in a static, so pinned: `GC.compact` must not move it.
        ruby.gc_register_mark_object(class);
        Ok::<_, Error>(Opaque::from(class))
    };
    let resolved = [
        class("Node")?,
        class("Schema")?,
        class("Versions")?,
        class("Request")?,
    ];
    // A second load keeps the first classes: they are the same constants.
    let _ = CLASSES.get_or_init(|| resolved);
    Ok(())
}

pub fn classes(ruby: &Ruby) -> Result<Classes, Error> {
    let [node, schema, versions, request] = CLASSES.get().ok_or_else(|| {
        Error::new(
            ruby.exception_load_error(),
            "the merkle_machines extension is not initialised",
        )
    })?;
    Ok(Classes {
        node: ruby.get_inner(*node),
        schema: ruby.get_inner(*schema),
        versions: ruby.get_inner(*versions),
        request: ruby.get_inner(*request),
    })
}

/// Whether `value` is an instance of exactly `class`: the classes compared by
/// identity, which no Ruby method can be redefined to fake. A subclass (whose
/// constant leaks past the seal) is refused, and so is a value given a
/// singleton class.
fn instance_of(value: Value, class: RClass) -> bool {
    value.class().as_raw() == class.as_raw()
}

/// `MerkleMachines.fields(value, type)`: the members of `value`, which must be
/// an instance of exactly `type`, one of the four value classes. `TypeError`
/// otherwise, a subclass included. The members are not trusted: every caller
/// converts them to the core's types and through its constructors.
pub fn fields(ruby: &Ruby, value: Value, class: RClass) -> Result<RArray, Error> {
    let known = classes(ruby)?;
    let ours = [known.node, known.schema, known.versions, known.request]
        .into_iter()
        .any(|candidate| candidate.as_raw() == class.as_raw());
    if !ours {
        return Err(Error::new(
            ruby.exception_type_error(),
            "fields reads only the MerkleMachines value classes",
        ));
    }
    if !instance_of(value, class) {
        return Err(Error::new(
            ruby.exception_type_error(),
            format!("expected a {class}, got {}", value.class()),
        ));
    }
    let members = RStruct::try_convert(value)?;
    let len = members.members()?.len();
    let out = ruby.ary_new_capa(len);
    for index in 0..len {
        out.push(members.aref::<_, Value>(index)?)?;
    }
    Ok(out)
}

/// A frozen instance of the `Data` class `class` holding `members`, in order.
/// Runs no Ruby code (no `new`, no `initialize`): the members come from the
/// core, which only holds valid values.
pub fn build<const N: usize>(class: RClass, members: [Value; N]) -> Result<Value, Error> {
    let instance = RStruct::try_convert(class.obj_alloc()?)?;
    for (index, member) in members.into_iter().enumerate() {
        instance.aset(index, member)?;
    }
    instance.freeze();
    Ok(instance.as_value())
}
