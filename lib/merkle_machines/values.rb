# frozen_string_literal: true

module MerkleMachines
  # The value classes are defined before the native extension loads, so it
  # resolves each one once, at init. Each `initialize` calls a private native
  # hook that holds the value to the core's constructor.

  # Address of a tree node: `level` 0 is the root, `index` counts left to right
  # within the level. `level` is an Integer in 0..255, `index` in 0...2**64.
  Node = Data.define(:level, :index) do
    def initialize(level:, index:)
      MerkleMachines.__send__(:check_node, level, index)
      super
    end
  end
  # Shape and identity of a fixed-depth digest tree. Two trees are comparable
  # only when all four fields are equal. A shape the core has no schema for
  # raises: `fanout` outside 2..MAX_FANOUT, or more leaves than fit in 64 bits.
  Schema = Data.define(:fanout, :depth, :partitioning, :digest) do
    def initialize(fanout:, depth:, partitioning:, digest:)
      MerkleMachines.__send__(:check_schema, fanout, depth, partitioning, digest)
      super
    end

    # Total number of leaves.
    def leaves = MerkleMachines.__send__(:schema_leaves, self)

    # Number of nodes at `level`, saturating past the leaf level.
    def width(level) = MerkleMachines.__send__(:schema_width, self, level)

    # Leaves below `node` as a half-open Range, nil when the node is not part
    # of the tree.
    def span(node) = MerkleMachines.__send__(:schema_span, self, node)

    # :compatible, or the first field on which `other` differs:
    # :fanout_mismatch, :depth_mismatch, :partitioning_mismatch, :digest_mismatch.
    def compatible(other) = MerkleMachines.__send__(:schema_compatible, self, other)
  end

  # Tree versions every digest of one session must be read from: Integers in
  # 0...2**64.
  Versions = Data.define(:local, :remote) do
    def initialize(local:, remote:)
      MerkleMachines.__send__(:check_versions, local, remote)
      super
    end
  end

  # A request for the child digests of one node, on both trees. `session` is
  # the scoped id of the session it belongs to; only the survey that issued it
  # honours the request. A session of sequence number zero names no session
  # and raises RangeError.
  Request = Data.define(:session, :node) do
    def initialize(session:, node:)
      MerkleMachines.__send__(:check_request, session, node)
      super
    end

    # The scope of the survey that issued the request's session.
    def scope = MerkleMachines.__send__(:request_scope, session)
  end
end
