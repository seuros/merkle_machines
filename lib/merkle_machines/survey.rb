# frozen_string_literal: true

module MerkleMachines
  private_class_method :check_schema, :check_node, :check_versions, :check_request, :schema_leaves,
                       :schema_width, :schema_span, :schema_compatible, :request_scope

  # The root of every tree.
  Node::ROOT = Node.new(0, 0)

  # The values are sealed. `initialize` is where each validates, and a
  # subclass could override it, or `allocate` build an instance that never ran
  # it: subclassing raises TypeError and `allocate` is gone. Sealing stops
  # accidents, not reflection (a named subclass's constant is bound before
  # `inherited` raises; `Marshal.load` skips `initialize`), so the binding is
  # the authority: it takes an instance of exactly the class asked for, reads
  # its fields from the struct (no reader runs) and the core's constructors
  # check every field again.
  [Node, Schema, Versions, Request].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end

  # Value objects around the native survey. The traversal itself is native,
  # and so is every value a mutating call returns (`next_request`, `pop_range`,
  # `drain`), built from a copy of the survey that is kept only once the answer
  # exists: frozen meanwhile is FrozenError, changed meanwhile RuntimeError,
  # and in both cases nothing is applied. Each mutating call (`begin`,
  # `answer`, `requeue`, ...) is native and raises FrozenError on a frozen
  # survey before any argument is read.
  class Survey
    # The schema of the open session, nil when the survey is idle.
    def schema
      shape = native_schema
      shape && Schema.new(*shape)
    end

    # The versions the open session pinned, nil when the survey is idle.
    def versions
      pins = native_versions
      pins && Versions.new(*pins)
    end
  end
end
