#![no_std]
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
#![doc = include_str!("../README.md")]

mod refusal;
mod request;
mod schema;
mod survey;

pub use refusal::Refusal;
pub use request::{Request, Versions};
pub use schema::{LeafRange, MAX_FANOUT, Mismatch, Node, Schema};
pub use survey::{Aborting, Begun, Phase, Survey};

#[cfg(test)]
mod tests;
