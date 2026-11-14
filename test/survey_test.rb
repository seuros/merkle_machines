# frozen_string_literal: true
require 'minitest/autorun'
require 'merkle_machines'

class SurveyTest < Minitest::Test
  include MerkleMachines

  PINS = Versions.new(local: 40, remote: 57)

  # Naive digest tree over an array of leaf strings; hashing stays in the test.
  class Tree
    def initialize(schema, leaves)
      @schema = schema
      @leaves = leaves
    end

    def digest(level, index)
      return @leaves.fetch(index).b if level == @schema.depth

      children(level, index).join('|').hash.to_s.b
    end

    def children(level, index)
      Array.new(@schema.fanout) { |child| digest(level + 1, (index * @schema.fanout) + child) }
    end
  end

  def schema(fanout: 4, depth: 3, partitioning: 1, digest: 1)
    Schema.new(fanout:, depth:, partitioning:, digest:)
  end

  def started(shape = schema(fanout: 2, depth: 2))
    survey = Survey.new
    assert_equal :diverged, survey.begin(shape, shape, PINS, 'a', 'b')
    survey
  end

  def at(request, **fields) = request.with(node: request.node.with(**fields))

  def deeper(request) = at(request, level: 1)

  def reply(survey, request, ours, theirs)
    node = request.node
    survey.answer(request, PINS, ours.children(node.level, node.index), theirs.children(node.level, node.index))
  end

  def test_locates_differing_leaf_ranges
    shape = schema
    leaves = Array.new(64) { |i| "leaf-#{i}" }
    changed = leaves.dup
    [5, 6, 7, 8, 40, 63].each { |i| changed[i] = "other-#{i}" }
    ours = Tree.new(shape, leaves)
    theirs = Tree.new(shape, changed)
    survey = Survey.new
    assert_equal :diverged, survey.begin(shape, shape, PINS, ours.digest(0, 0), theirs.digest(0, 0))
    assert_equal :surveying, survey.phase
    while (request = survey.next_request)
      assert_equal :accepted, reply(survey, request, ours, theirs)
    end
    assert_equal :draining, survey.phase
    assert_equal 3, survey.buffered
    assert_equal [5...9], survey.drain(1)
    assert_equal [], survey.drain(0)
    assert_equal 40...41, survey.pop_range
    assert_equal [63...64], survey.drain(OUTPUT_CAPACITY)
    assert_equal :complete, survey.phase
    assert_equal [], survey.drain(OUTPUT_CAPACITY)
    assert_nil survey.pop_range
    assert_raises(ArgumentError) { survey.drain }
  end

  def test_identical_roots_and_single_leaf_tree
    survey = Survey.new
    assert_equal :idle, survey.phase
    assert_equal :identical, survey.begin(schema, schema, PINS, 'same', 'same')
    assert_equal :complete, survey.phase
    assert_nil survey.next_request
    leaf = schema(fanout: 2, depth: 0)
    assert_equal :diverged, survey.begin(leaf, leaf, PINS, 'a', 'b')
    assert_equal 2, survey.session
    assert_equal [0...1], survey.drain(8)
  end

  def test_schema_refusals_open_no_session
    survey = Survey.new
    base = schema
    {
      fanout_mismatch: schema(fanout: 8), depth_mismatch: schema(depth: 2),
      partitioning_mismatch: schema(partitioning: 2), digest_mismatch: schema(digest: 2)
    }.each do |refusal, other|
      assert_equal refusal, survey.begin(base, other, PINS, 'a', 'b')
    end
    assert_equal :idle, survey.phase
    assert_nil survey.session
  end

  # The core has no Schema for these shapes, so neither does Ruby.
  def test_schema_is_validated_like_the_core
    assert_equal [2, 63], schema(fanout: 2, depth: 63).deconstruct.first(2)
    assert_equal [64, 10], schema(fanout: 64, depth: 10).deconstruct.first(2)
    [[1, 3], [0, 0], [65, 1], [2, 64], [64, 11]].each do |fanout, depth|
      assert_raises(RangeError) { schema(fanout:, depth:) }
      assert_raises(RangeError) { schema.with(fanout:, depth:) }
    end
    assert_raises(RangeError) { schema(partitioning: 2**64) }
    assert_raises(RangeError) { schema(digest: -1) }
    [1.0, nil, '2'].each { |bad| assert_raises(TypeError) { schema(fanout: bad) } }
    assert_raises(NoMethodError) { MerkleMachines.check_schema(4, 3, 1, 1) }
  end

  def test_values_mirror_the_core
    [Schema, Versions, Node, Request].each { assert_operator _1, :<, Data }
    assert_equal %i[fanout depth partitioning digest], Schema.members
    assert_equal %i[local remote], Versions.members
    assert_equal %i[level index], Node.members
    assert_equal %i[session node], Request.members
  end

  def test_schema_arithmetic_mirrors_the_core
    shape = schema
    assert_equal 64, shape.leaves
    assert_equal [1, 4, 16, 64, 64, 64], [0, 1, 2, 3, 4, 255].map { shape.width(it) }
    assert_equal Node.new(0, 0), Node::ROOT
    assert_predicate Node::ROOT, :frozen?
    assert_equal 0...64, shape.span(Node::ROOT)
    assert_equal 16...32, shape.span(Node.new(1, 1))
    assert_equal 63...64, shape.span(Node.new(3, 63))
    assert_nil shape.span(Node.new(3, 64))
    assert_nil shape.span(Node.new(4, 0))
    assert_equal 2**63, schema(fanout: 2, depth: 63).leaves
    assert_equal 0...1, schema(fanout: 2, depth: 0).span(Node::ROOT)
    assert_raises(TypeError) { shape.width(1.0) }
    assert_raises(RangeError) { shape.width(256) }
    assert_raises(TypeError) { shape.span([0, 0]) }
    assert_raises(RangeError) { shape.span(Node.new(0, -1)) }
    assert_raises(TypeError) { shape.span(Node.new(nil, 0)) }
    %i[schema_leaves schema_width schema_span schema_compatible fields].each do |name|
      refute_respond_to MerkleMachines, name
    end
  end

  def test_schema_compatibility_names_the_first_difference
    base = schema
    assert_equal :compatible, base.compatible(schema)
    assert_equal :fanout_mismatch, base.compatible(schema(fanout: 8, depth: 2, partitioning: 2, digest: 2))
    assert_equal :depth_mismatch, base.compatible(schema(depth: 2, partitioning: 2))
    assert_equal :partitioning_mismatch, base.compatible(schema(partitioning: 2, digest: 2))
    assert_equal :digest_mismatch, base.compatible(schema(digest: 2))
    assert_raises(TypeError) { base.compatible([4, 3, 1, 1]) }
  end

  def test_schema_and_versions_of_the_open_session
    survey = Survey.new
    assert_nil survey.schema
    assert_nil survey.versions
    shape = schema(fanout: 2, depth: 2)
    assert_equal :identical, survey.begin(shape, shape, PINS, 'a', 'a')
    assert_equal [shape, PINS], [survey.schema, survey.versions]
    assert_equal :aborted, survey.abort
    assert_nil survey.schema
    assert_nil survey.versions
  end

  def test_busy_abort_and_stale_requests
    survey = started
    assert_equal :busy, survey.begin(schema, schema, PINS, 'a', 'b')
    request = survey.next_request
    assert_equal Request.new(session: 1, node: Node.new(level: 0, index: 0)), request
    assert_predicate request, :frozen?
    assert_equal :aborted, survey.abort
    assert_equal :already_idle, survey.abort
    assert_equal :already_idle, Survey.new.abort
    # Idle again, and `session` still answers the aborted session.
    assert_equal [:idle, 1], [survey.phase, survey.session]
    assert_equal :stale, survey.answer(request, PINS, %w[a b], %w[a c])
    assert_equal :stale, survey.requeue(request)
    started_again = schema(fanout: 2, depth: 2)
    assert_equal :diverged, survey.begin(started_again, started_again, PINS, 'a', 'b')
    assert_equal :stale, survey.answer(request, PINS, %w[a b], %w[a c])
  end

  # Two surveys, the same calls: a request from one named a node in flight on
  # the other. In different scopes each refuses the other's.
  def test_request_from_another_scope_is_refused_and_changes_nothing
    shape = schema(fanout: 2, depth: 2)
    a = Survey.with_scope(1)
    b = Survey.with_scope(2)
    [a, b].each { |survey| assert_equal :diverged, survey.begin(shape, shape, PINS, 'a', 'b') }
    from_a = a.next_request
    from_b = b.next_request
    assert_equal [1, 2, 1, 2], [a.scope, b.scope, from_a.scope, from_b.scope]
    assert_equal [(1 << 48) | 1, (2 << 48) | 1, from_a.node], [from_a.session, from_b.session, from_b.node]
    assert_equal [from_a.session, from_b.session], [a.session, b.session]
    snapshot = ->(survey) { [survey.phase, survey.session, survey.pending, survey.in_flight, survey.buffered] }
    before = [snapshot.(a), snapshot.(b)]
    assert_equal :stale, b.answer(from_a, PINS, %w[a b], %w[a c])
    assert_equal :stale, b.requeue(from_a)
    assert_equal :stale, a.answer(from_b, PINS, %w[a b], %w[a c])
    assert_equal :stale, a.requeue(from_b)
    assert_equal before, [snapshot.(a), snapshot.(b)]
    assert_equal :accepted, a.answer(from_a, PINS, %w[a b], %w[a c])
    assert_equal :requeued, b.requeue(from_b)
    # The core cannot pick a scope: two surveys left in one scope issue the
    # same sessions, and one honours the other's request.
    twin = Survey.with_scope(2)
    assert_equal :diverged, twin.begin(shape, shape, PINS, 'a', 'b')
    assert_equal from_b, twin.next_request
    assert_equal :accepted, twin.answer(from_b, PINS, %w[a b], %w[a c])
  end

  def test_with_scope_takes_an_integer_scope
    survey = Survey.with_scope(65_535)
    assert_equal [65_535, nil, 0, :idle], [survey.scope, survey.session, survey.issued, survey.phase]
    assert_equal [0, 0], [Survey.new.scope, Survey.with_scope(0).scope]
    [1.0, nil, '1', Object.new].each { |value| assert_raises(TypeError) { Survey.with_scope(value) } }
    [-1, 65_536, 2**64].each { |scope| assert_raises(RangeError) { Survey.with_scope(scope) } }
    assert_equal 65_535, Request.new((65_535 << 48) | 9, Node::ROOT).scope
    assert_raises(TypeError) { Request.new('1', Node::ROOT).scope }
    assert_raises(RangeError) { Request.new(2**64, Node::ROOT).scope }
  end

  # A fresh survey opens session 1 again, so a request that outlived a restart
  # would name a node in flight there. A resumed one continues the scope.
  def test_resumed_survey_continues_its_scope_and_refuses_earlier_requests
    shape = schema(fanout: 2, depth: 2)
    first = Survey.with_scope(5)
    earlier = 2.times.map do
      first.abort
      assert_equal :diverged, first.begin(shape, shape, PINS, 'a', 'b')
      first.next_request
    end
    assert_equal [2, (5 << 48) | 2], [first.issued, first.session]

    survey = Survey.resumed(first.scope, first.issued)
    assert_equal [5, 2, :idle, nil, 0], [survey.scope, survey.issued, survey.phase, survey.schema, survey.pending]
    # Resumed and idle: the last session issued before the restart, not open.
    assert_equal first.session, survey.session
    assert_nil Survey.resumed(5, 0).session
    refused = -> { earlier.flat_map { |old| [survey.answer(old, PINS, %w[a b], %w[a c]), survey.requeue(old)] } }
    assert_equal [:stale] * 4, refused.()
    assert_equal :diverged, survey.begin(shape, shape, PINS, 'a', 'b')
    request = survey.next_request
    assert_equal [(5 << 48) | 3, 3, earlier.last.node], [request.session, survey.issued, request.node]
    assert_equal [:stale] * 4, refused.()
    assert_equal [1, 1], [survey.pending, survey.in_flight]
    assert_equal :accepted, survey.answer(request, PINS, %w[a b], %w[a c])
  end

  def test_resumed_at_the_last_sequence_is_spent_and_past_it_raises
    shape = schema(fanout: 2, depth: 2)
    last = (2**48) - 1
    survey = Survey.resumed(3, last - 1)
    assert_equal :diverged, survey.begin(shape, shape, PINS, 'a', 'b')
    assert_equal [(3 << 48) | last, last], [survey.session, survey.issued]
    assert_equal :busy, survey.begin(shape, shape, PINS, 'a', 'b')
    assert_equal :aborted, survey.abort
    assert_equal :exhausted, survey.begin(shape, shape, PINS, 'a', 'b')
    spent = Survey.resumed(3, last)
    assert_equal [:exhausted, :idle, last], [spent.begin(shape, shape, PINS, 'a', 'b'), spent.phase, spent.issued]
    assert_raises(RangeError) { Survey.resumed(3, last + 1) }
    [-1, 65_536].each { |scope| assert_raises(RangeError) { Survey.resumed(scope, 0) } }
    assert_raises(RangeError) { Survey.resumed(3, -1) }
    [1.0, nil, '1'].each do |value|
      assert_raises(TypeError) { Survey.resumed(value, 0) }
      assert_raises(TypeError) { Survey.resumed(3, value) }
    end
  end

  def test_answer_refusals_change_nothing
    survey = started
    request = survey.next_request
    assert_equal :unexpected, survey.answer(deeper(request), PINS, %w[a b], %w[a c])
    assert_equal :version_changed, survey.answer(request, PINS.with(remote: 58), %w[a b], %w[a c])
    assert_equal :arity, survey.answer(request, PINS, %w[a], %w[a c])
    assert_equal :inconsistent, survey.answer(request, PINS, %w[a b], %w[a b])
    assert_equal [1, 1, 0], [survey.pending, survey.in_flight, survey.buffered]
    assert_equal :accepted, survey.answer(request, PINS, %w[a b], %w[a c])
    assert_equal :unexpected, survey.answer(request, PINS, %w[a b], %w[a c])
    assert_equal Request.new(1, Node.new(1, 1)), survey.next_request
  end

  def test_requeue_resumes_lost_requests
    survey = started
    request = survey.next_request
    assert_nil survey.next_request
    assert_equal :stale, survey.requeue(request.with(session: 2))
    assert_equal :unexpected, survey.requeue(deeper(request))
    assert_equal [1, 1], [survey.pending, survey.in_flight]
    assert_equal :requeued, survey.requeue(request)
    assert_equal :unexpected, survey.requeue(request)
    assert_equal [1, 0], [survey.pending, survey.in_flight]
    assert_equal request, survey.next_request
    assert_equal 1, survey.requeue_all
    assert_equal 0, survey.requeue_all
    assert_equal request, survey.next_request
  end

  def test_bounded_buffers_hold_work_until_drained
    assert_equal [64, 64, 64], [PENDING_CAPACITY, OUTPUT_CAPACITY, MAX_FANOUT]
    shape = schema(fanout: 2, depth: 8)
    ours = Tree.new(shape, Array.new(256) { |i| "leaf-#{i}" })
    theirs = Tree.new(shape, Array.new(256) { |i| i.even? ? "other-#{i}" : "leaf-#{i}" })
    survey = Survey.new
    survey.begin(shape, shape, PINS, ours.digest(0, 0), theirs.digest(0, 0))
    found = []
    until survey.phase == :complete
      assert_operator survey.pending, :<=, PENDING_CAPACITY
      assert_operator survey.buffered, :<=, OUTPUT_CAPACITY
      request = survey.next_request
      request ? reply(survey, request, ours, theirs) : found.concat(survey.drain(7))
    end
    assert_equal (0...256).step(2).map { |i| i...(i + 1) }, found.sort_by(&:begin)
  end

  def test_strict_inputs_raise_before_mutation
    survey = started
    request = survey.next_request
    [1.0, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { survey.answer(at(request, index: bad), PINS, %w[a b], %w[a c]) }
      assert_raises(TypeError) { survey.answer(request.with(node: bad), PINS, %w[a b], %w[a c]) }
      assert_raises(TypeError) { survey.answer(request, PINS.with(local: bad), %w[a b], %w[a c]) }
      assert_raises(TypeError) { Survey.new.begin(schema(depth: bad), schema, PINS, 'a', 'b') }
      assert_raises(TypeError) { survey.drain(bad) }
    end
    assert_raises(TypeError) { survey.answer(request, PINS, [1, 2], %w[a c]) }
    assert_raises(TypeError) { survey.answer(request, PINS, 'ab', %w[a c]) }
    assert_raises(TypeError) { survey.answer([1, 0, 0], PINS, %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.answer(request, [40, 57], %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.requeue(nil) }
    assert_raises(TypeError) { Survey.new.begin(schema, schema, PINS, 1, 'b') }
    assert_raises(TypeError) { Survey.new.begin([4, 3, 1, 1], schema, PINS, 'a', 'b') }
    assert_raises(RangeError) { survey.answer(request.with(session: 2**64), PINS, %w[a b], %w[a c]) }
    assert_raises(RangeError) { survey.answer(at(request, index: -1), PINS, %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.requeue(request.with(node: [0, 0])) }
    assert_raises(RangeError) { Survey.new.begin(schema(fanout: 256), schema, PINS, 'a', 'b') }
    assert_raises(RangeError) { survey.drain(-1) }
    assert_raises(TypeError) { Survey.allocate }
    assert_equal [1, 1], [survey.pending, survey.in_flight]
    assert_equal :accepted, survey.answer(request, PINS, %w[a b], %w[a c])
  end

  # The binding never loses work: answers are built natively without running
  # Ruby code, so an `initialize` that raises cannot lose a node marked in flight.
  def test_answers_come_back_when_initialize_raises
    survey = started
    overridden = [Request, Node].each do |type|
      type.alias_method(:checked_initialize, :initialize)
      type.define_method(:initialize) { |**| raise 'initialize must not run' }
    end
    begin
      request = survey.next_request
    ensure
      overridden.each do |type|
        type.alias_method(:initialize, :checked_initialize)
        type.remove_method(:checked_initialize)
      end
    end
    assert_equal [1, 1], [survey.pending, survey.in_flight]
    assert_equal Request.new(1, Node::ROOT), request
    assert_predicate request, :frozen?
    assert_predicate request.node, :frozen?
    assert_equal :accepted, survey.answer(request, PINS, %w[a b], %w[a c])
  end

  def test_u64_limits_and_binary_digests
    top = Versions.new(local: (2**64) - 1, remote: (2**64) - 1)
    shape = Schema.new(fanout: 2, depth: 2, partitioning: (2**64) - 1, digest: (2**64) - 1)
    survey = Survey.new
    assert_equal :diverged, survey.begin(shape, shape, top, "\x00\xFF".b, "\x00\xFE".b)
    request = survey.next_request
    assert_equal :accepted, survey.answer(request, top, ["\xFF".b, "\x01".b], ["\xFF".b, "\x02".b])
    assert_equal :accepted, survey.answer(survey.next_request, top, ['', 'x'], ['', 'y'])
    assert_equal [3...4], survey.drain((2**64) - 1)
  end

  def test_freeze_guards
    survey = started
    request = survey.next_request
    survey.freeze
    assert_raises(FrozenError) { survey.begin(schema, schema, PINS, 'a', 'b') }
    assert_raises(FrozenError) { survey.next_request }
    assert_raises(FrozenError) { survey.answer(request, PINS, %w[a b], %w[a c]) }
    assert_raises(FrozenError) { survey.requeue(request) }
    assert_raises(FrozenError) { survey.requeue_all }
    assert_raises(FrozenError) { survey.drain(1) }
    assert_raises(FrozenError) { survey.pop_range }
    assert_equal [schema(fanout: 2, depth: 2), PINS], [survey.schema, survey.versions]
    assert_raises(FrozenError) { survey.abort }
    assert_equal [:surveying, 1], [survey.phase, survey.session]
    assert_equal [1, 1, 0], [survey.pending, survey.in_flight, survey.buffered]
  end
end
