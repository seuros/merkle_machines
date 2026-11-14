# frozen_string_literal: true

module MerkleMachines
  private_class_method :check_schema, :check_node, :check_versions, :check_request, :schema_leaves,
                       :schema_width, :schema_span, :schema_compatible, :request_scope, :fields

  # The root of every tree.
  Node::ROOT = Node.new(0, 0)

  # The values are sealed. `initialize` is where each validates, and a
  # subclass could override it, or `allocate` build an instance that never ran
  # it: subclassing raises TypeError and `allocate` is gone. Sealing stops
  # accidents, not reflection (a named subclass's constant is bound before
  # `inherited` raises; `Marshal.load` skips `initialize`), so the native
  # `fields` reader is the authority: it takes an instance of exactly the
  # class asked for and the core's constructors check every field again.
  [Node, Schema, Versions, Request].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end

  # Value objects around the native survey. The traversal itself is native,
  # and so is every value a mutating call returns (`next_request`, `pop_range`,
  # `drain`), built from a copy of the survey that is kept only once the answer
  # exists: frozen meanwhile is FrozenError, changed meanwhile RuntimeError,
  # and in both cases nothing is applied.
  class Survey
    # Each mutating call raises FrozenError on a frozen survey before any
    # argument is read, as the native methods do.
    def begin(local, remote, versions, local_root, remote_root)
      unfrozen!
      native_begin(fields(local, Schema), fields(remote, Schema), fields(versions, Versions), local_root, remote_root)
    end

    def answer(request, versions, local, remote)
      unfrozen!
      native_answer(ticket(request), fields(versions, Versions), local, remote)
    end

    def requeue(request)
      unfrozen!
      native_requeue(ticket(request))
    end

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

    private

    def unfrozen!
      raise FrozenError.new("can't modify frozen #{self.class}: #{inspect}", receiver: self) if frozen?
    end

    def fields(value, type) = MerkleMachines.__send__(:fields, value, type)

    def ticket(request)
      session, node = fields(request, Request)
      [session, *fields(node, Node)]
    end
  end
end
