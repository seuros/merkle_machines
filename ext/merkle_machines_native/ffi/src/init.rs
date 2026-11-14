//! Private native entry point.
use crate::{
    MAX_FANOUT, OUTPUT_CAPACITY, PENDING_CAPACITY, Survey, check_node, check_request, check_schema,
    check_versions, request_scope, schema_compatible, schema_leaves, schema_span, schema_width,
};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("MerkleMachines")?;
    crate::values::resolve(ruby, module)?;
    module.const_set("PENDING_CAPACITY", PENDING_CAPACITY)?;
    module.const_set("OUTPUT_CAPACITY", OUTPUT_CAPACITY)?;
    module.const_set("MAX_FANOUT", MAX_FANOUT)?;
    module.define_singleton_method("check_schema", function!(check_schema, 4))?;
    module.define_singleton_method("check_node", function!(check_node, 2))?;
    module.define_singleton_method("check_versions", function!(check_versions, 2))?;
    module.define_singleton_method("check_request", function!(check_request, 2))?;
    module.define_singleton_method("schema_leaves", function!(schema_leaves, 1))?;
    module.define_singleton_method("schema_width", function!(schema_width, 2))?;
    module.define_singleton_method("schema_span", function!(schema_span, 2))?;
    module.define_singleton_method("schema_compatible", function!(schema_compatible, 2))?;
    module.define_singleton_method("request_scope", function!(request_scope, 1))?;
    let class = module.define_class("Survey", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Survey::new, 0))?;
    class.define_singleton_method("with_scope", function!(Survey::with_scope, 1))?;
    class.define_singleton_method("resumed", function!(Survey::resumed, 2))?;
    class.define_method("scope", method!(Survey::scope, 0))?;
    class.define_method("issued", method!(Survey::issued, 0))?;
    class.define_method("begin", method!(Survey::begin, 5))?;
    class.define_method("next_request", method!(Survey::next_request, 0))?;
    class.define_method("answer", method!(Survey::answer, 4))?;
    class.define_method("requeue", method!(Survey::requeue, 1))?;
    class.define_method("pop_range", method!(Survey::pop_range, 0))?;
    class.define_method("drain", method!(Survey::drain, 1))?;
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
