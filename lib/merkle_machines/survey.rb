# frozen_string_literal: true

module MerkleMachines
  private_class_method :check_schema, :schema_leaves, :schema_width, :schema_span, :schema_compatible,
                       :request_scope

  # Reads a value object's fields, refusing anything that is not one.
  def self.fields(value, type)
    raise TypeError, "expected a #{type}, got #{value.class}" unless value.is_a?(type)

    value.deconstruct
  end
  private_class_method :fields

  # Address of a tree node: `level` 0 is the root, `index` counts left to right
  # within the level.
  Node = Data.define(:level, :index)
  # The root of every tree.
  Node::ROOT = Node.new(0, 0)

  # Shape and identity of a fixed-depth digest tree. Two trees are comparable
  # only when all four fields are equal. A shape the core has no schema for
  # raises: `fanout` outside 2..MAX_FANOUT, or more leaves than fit in 64 bits.
  Schema = Data.define(:fanout, :depth, :partitioning, :digest) do
    def initialize(fanout:, depth:, partitioning:, digest:)
      MerkleMachines.__send__(:check_schema, fanout, depth, partitioning, digest)
      super
    end

    # Total number of leaves.
    def leaves = MerkleMachines.__send__(:schema_leaves, deconstruct)

    # Number of nodes at `level`, saturating past the leaf level.
    def width(level) = MerkleMachines.__send__(:schema_width, deconstruct, level)

    # Leaves below `node` as a half-open Range, nil when the node is not part
    # of the tree.
    def span(node)
      first, last = MerkleMachines.__send__(:schema_span, deconstruct, MerkleMachines.__send__(:fields, node, Node))
      first && (first...last)
    end

    # :compatible, or the first field on which `other` differs:
    # :fanout_mismatch, :depth_mismatch, :partitioning_mismatch, :digest_mismatch.
    def compatible(other)
      MerkleMachines.__send__(:schema_compatible, deconstruct, MerkleMachines.__send__(:fields, other, Schema))
    end
  end

  # Tree versions every digest of one session must be read from.
  Versions = Data.define(:local, :remote)

  # A request for the child digests of one node, on both trees. `session` is
  # the scoped id of the session it belongs to; only the survey that issued it
  # honours the request.
  Request = Data.define(:session, :node) do
    # The scope of the survey that issued the request's session.
    def scope = MerkleMachines.__send__(:request_scope, session)
  end

  # Value objects around the native survey. The traversal itself is native.
  class Survey
    def begin(local, remote, versions, local_root, remote_root)
      native_begin(fields(local, Schema), fields(remote, Schema), fields(versions, Versions), local_root, remote_root)
    end

    def next_request
      session, level, index = native_next_request
      session && Request.new(session, Node.new(level, index))
    end

    def answer(request, versions, local, remote)
      native_answer(ticket(request), fields(versions, Versions), local, remote)
    end

    def requeue(request) = native_requeue(ticket(request))

    # The lowest buffered leaf range, nil when none is buffered.
    def pop_range
      first, last = native_pop_range
      first && (first...last)
    end

    # At most `limit` buffered leaf ranges, lowest first.
    def drain(limit)
      native_drain(limit).map { |first, last| first...last }
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

    def fields(value, type) = MerkleMachines.__send__(:fields, value, type)

    def ticket(request)
      session, node = fields(request, Request)
      [session, *fields(node, Node)]
    end
  end
end
