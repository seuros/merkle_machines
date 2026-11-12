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

  def reply(survey, request, ours, theirs)
    survey.answer(request, PINS, ours.children(request.level, request.index),
                  theirs.children(request.level, request.index))
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
    assert_equal [5...9], survey.take_ranges(1)
    assert_equal [40...41, 63...64], survey.take_ranges
    assert_equal :complete, survey.phase
    assert_equal [], survey.take_ranges
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
    assert_equal [0...1], survey.take_ranges
  end

  def test_schema_refusals_open_no_session
    survey = Survey.new
    base = schema
    {
      fanout_mismatch: schema(fanout: 8), depth_mismatch: schema(depth: 2),
      partitioning_mismatch: schema(partitioning: 2), digest_mismatch: schema(digest: 2),
      invalid_schema: schema(fanout: 1)
    }.each do |refusal, other|
      assert_equal refusal, survey.begin(base, other, PINS, 'a', 'b')
    end
    assert_equal :invalid_schema, survey.begin(schema(fanout: 65), schema(fanout: 65), PINS, 'a', 'b')
    assert_equal :invalid_schema, survey.begin(schema(fanout: 2, depth: 64), base, PINS, 'a', 'b')
    assert_equal :idle, survey.phase
    assert_equal 0, survey.session
  end

  def test_busy_abort_and_stale_requests
    survey = started
    assert_equal :busy, survey.begin(schema, schema, PINS, 'a', 'b')
    request = survey.next_request
    assert_equal Request.new(session: 1, level: 0, index: 0), request
    assert survey.abort
    refute survey.abort
    assert_equal :idle, survey.phase
    assert_equal :stale, survey.answer(request, PINS, %w[a b], %w[a c])
    refute survey.requeue(request)
    started_again = schema(fanout: 2, depth: 2)
    assert_equal :diverged, survey.begin(started_again, started_again, PINS, 'a', 'b')
    assert_equal :stale, survey.answer(request, PINS, %w[a b], %w[a c])
  end

  def test_answer_refusals_change_nothing
    survey = started
    request = survey.next_request
    assert_equal :unexpected, survey.answer(request.with(level: 1), PINS, %w[a b], %w[a c])
    assert_equal :version_changed, survey.answer(request, PINS.with(remote: 58), %w[a b], %w[a c])
    assert_equal :arity, survey.answer(request, PINS, %w[a], %w[a c])
    assert_equal :inconsistent, survey.answer(request, PINS, %w[a b], %w[a b])
    assert_equal [1, 1, 0], [survey.pending, survey.in_flight, survey.buffered]
    assert_equal :accepted, survey.answer(request, PINS, %w[a b], %w[a c])
    assert_equal :unexpected, survey.answer(request, PINS, %w[a b], %w[a c])
    assert_equal Request.new(session: 1, level: 1, index: 1), survey.next_request
  end

  def test_requeue_resumes_lost_requests
    survey = started
    request = survey.next_request
    assert_nil survey.next_request
    assert survey.requeue(request)
    refute survey.requeue(request)
    assert_equal 0, survey.in_flight
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
      request ? reply(survey, request, ours, theirs) : found.concat(survey.take_ranges(7))
    end
    assert_equal (0...256).step(2).map { |i| i...(i + 1) }, found.sort_by(&:begin)
  end

  def test_strict_inputs_raise_before_mutation
    survey = started
    request = survey.next_request
    [1.0, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { survey.answer(request.with(index: bad), PINS, %w[a b], %w[a c]) }
      assert_raises(TypeError) { survey.answer(request, PINS.with(local: bad), %w[a b], %w[a c]) }
      assert_raises(TypeError) { Survey.new.begin(schema(depth: bad), schema, PINS, 'a', 'b') }
      assert_raises(TypeError) { survey.take_ranges(bad) }
    end
    assert_raises(TypeError) { survey.answer(request, PINS, [1, 2], %w[a c]) }
    assert_raises(TypeError) { survey.answer(request, PINS, 'ab', %w[a c]) }
    assert_raises(TypeError) { survey.answer([1, 0, 0], PINS, %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.answer(request, [40, 57], %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.requeue(nil) }
    assert_raises(TypeError) { Survey.new.begin(schema, schema, PINS, 1, 'b') }
    assert_raises(TypeError) { Survey.new.begin([4, 3, 1, 1], schema, PINS, 'a', 'b') }
    assert_raises(RangeError) { survey.answer(request.with(session: 2**64), PINS, %w[a b], %w[a c]) }
    assert_raises(RangeError) { survey.answer(request.with(index: -1), PINS, %w[a b], %w[a c]) }
    assert_raises(RangeError) { Survey.new.begin(schema(fanout: 256), schema, PINS, 'a', 'b') }
    assert_raises(RangeError) { survey.take_ranges(-1) }
    assert_raises(TypeError) { Survey.allocate }
    assert_equal [1, 1], [survey.pending, survey.in_flight]
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
    assert_equal [3...4], survey.take_ranges(2**64 - 1)
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
    assert_raises(FrozenError) { survey.take_ranges }
    assert_raises(FrozenError) { survey.abort }
    assert_equal [:surveying, 1], [survey.phase, survey.session]
    assert_equal [1, 1, 0], [survey.pending, survey.in_flight, survey.buffered]
  end
end
