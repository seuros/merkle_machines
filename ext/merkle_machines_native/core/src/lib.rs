#![no_std]
#![doc = include_str!("../README.md")]

mod schema;
mod survey;

pub use schema::{LeafRange, MAX_FANOUT, Mismatch, Node, Schema};
pub use survey::{Begun, Phase, Refusal, Request, Survey, Versions};

#[cfg(test)]
mod tests;
