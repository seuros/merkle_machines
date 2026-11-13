//! Private native entry point.
use crate::{
    MAX_FANOUT, OUTPUT_CAPACITY, PENDING_CAPACITY, Survey, check_schema, schema_compatible,
    schema_leaves, schema_span, schema_width,
};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("MerkleMachines")?;
    module.const_set("PENDING_CAPACITY", PENDING_CAPACITY)?;
    module.const_set("OUTPUT_CAPACITY", OUTPUT_CAPACITY)?;
    module.const_set("MAX_FANOUT", MAX_FANOUT)?;
    module.define_singleton_method("check_schema", function!(check_schema, 4))?;
    module.define_singleton_method("schema_leaves", function!(schema_leaves, 1))?;
    module.define_singleton_method("schema_width", function!(schema_width, 2))?;
    module.define_singleton_method("schema_span", function!(schema_span, 2))?;
    module.define_singleton_method("schema_compatible", function!(schema_compatible, 2))?;
    let class = module.define_class("Survey", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Survey::new, 0))?;
    class.define_private_method("native_begin", method!(Survey::begin, 5))?;
    class.define_private_method("native_next_request", method!(Survey::next_request, 0))?;
    class.define_private_method("native_answer", method!(Survey::answer, 4))?;
    class.define_private_method("native_requeue", method!(Survey::requeue, 1))?;
    class.define_private_method("native_pop_range", method!(Survey::pop_range, 0))?;
    class.define_private_method("native_drain", method!(Survey::drain, 1))?;
    class.define_private_method("native_schema", method!(Survey::schema, 0))?;
    class.define_private_method("native_versions", method!(Survey::versions, 0))?;
    class.define_method("requeue_all", method!(Survey::requeue_all, 0))?;
    class.define_method("abort", method!(Survey::abort, 0))?;
    class.define_method("phase", method!(Survey::phase, 0))?;
    class.define_method("session", method!(Survey::session, 0))?;
    class.define_method("pending", method!(Survey::pending, 0))?;
    class.define_method("in_flight", method!(Survey::in_flight, 0))?;
    class.define_method("buffered", method!(Survey::buffered, 0))?;
    Ok(())
}
