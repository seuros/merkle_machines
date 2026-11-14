//! The value classes the binding reads and builds, resolved once at init.
//!
//! `lib/merkle_machines/values.rb` defines `Node`, `Schema`, `Versions` and
//! `Request` before this extension loads. Sealing them stops accidents, not
//! reflection, so the binding is the authority on what Ruby hands in: an
//! instance of exactly the class asked for, whose members are read straight
//! from the `Data` struct with `exact_fields` (no reader runs) and then
//! checked again by the core constructor at each use.

use magnus::{Error, RClass, RModule, RStruct, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry};

/// The classes as resolved at init.
pub struct Classes {
    pub node: Class,
    pub schema: Class,
    pub versions: Class,
    pub request: Class,
}

static CLASSES: Registry<Classes> = Registry::new("merkle_machines");

pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let classes = Classes {
        node: class("Node")?,
        schema: class("Schema")?,
        versions: class("Versions")?,
        request: class("Request")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
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
