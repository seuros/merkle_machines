# frozen_string_literal: true

module MerkleMachines
  # Shape and identity of a fixed-depth digest tree. Two trees are comparable
  # only when all four fields are equal.
  Schema = Data.define(:fanout, :depth, :partitioning, :digest)

  # Tree versions every digest of one session must be read from.
  Versions = Data.define(:local, :remote)

  # A request for the child digests of one node, on both trees.
  Request = Data.define(:session, :level, :index)

  # Value objects around the native survey. The traversal itself is native.
  class Survey
    def begin(local, remote, versions, local_root, remote_root)
      native_begin(fields(local, Schema), fields(remote, Schema), fields(versions, Versions), local_root, remote_root)
    end

    def next_request
      found = native_next_request
      found && Request.new(*found)
    end

    def answer(request, versions, local_children, remote_children)
      native_answer(fields(request, Request), fields(versions, Versions), local_children, remote_children)
    end

    def requeue(request) = native_requeue(fields(request, Request))

    def take_ranges(limit = OUTPUT_CAPACITY)
      native_take_ranges(limit).map { |first, last| first...last }
    end

    private

    def fields(value, type)
      raise TypeError, "expected a #{type}, got #{value.class}" unless value.is_a?(type)

      value.deconstruct
    end
  end
end
