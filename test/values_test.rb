# frozen_string_literal: true
require 'minitest/autorun'
require 'merkle_machines'

# Every value is held to the core's rule, whether the gem returned it or the
# caller built it by hand.
class ValuesTest < Minitest::Test
  include MerkleMachines

  def test_node_fields_are_the_cores_types
    assert_equal [255, (2**64) - 1], Node.new(255, (2**64) - 1).deconstruct
    [[256, 0], [-1, 0], [0, 2**64], [0, -1]].each { |fields| assert_raises(RangeError) { Node.new(*fields) } }
    [[1.0, 0], [0, nil], ['0', 0]].each { |fields| assert_raises(TypeError) { Node.new(*fields) } }
    assert_raises(RangeError) { Node::ROOT.with(level: 256) }
  end

  def test_versions_fields_are_the_cores_types
    assert_equal [0, (2**64) - 1], Versions.new(0, (2**64) - 1).deconstruct
    [[-1, 0], [0, 2**64]].each { |fields| assert_raises(RangeError) { Versions.new(*fields) } }
    [[1.0, 0], [0, nil], ['0', 0]].each { |fields| assert_raises(TypeError) { Versions.new(*fields) } }
  end

  # Sequence zero names no session in any scope, so no request carries it.
  def test_a_request_names_an_issuable_session
    assert_equal 7, Request.new((7 << 48) | 1, Node::ROOT).scope
    [0, 7 << 48, 2**64, -1].each { |session| assert_raises(RangeError) { Request.new(session, Node::ROOT) } }
    [1.0, nil, '1'].each { |session| assert_raises(TypeError) { Request.new(session, Node::ROOT) } }
    [nil, [0, 0], Object.new].each { |node| assert_raises(TypeError) { Request.new(1, node) } }
    assert_raises(RangeError) { Request.new(1, Node::ROOT).with(session: 0) }
  end

  # Sealing stops accidents, not reflection: Ruby binds a named subclass's
  # constant before `inherited` raises, so the class leaks. Both forms raise.
  def test_values_are_sealed_and_a_named_subclass_leaks_its_constant
    [Node, Schema, Versions, Request].each do |value|
      assert_raises(TypeError) { Class.new(value) }
      assert_raises(NoMethodError) { value.allocate }
    end
    assert_raises(TypeError) { self.class.module_eval('class LeakedNode < Node; end', __FILE__, __LINE__) }
    assert self.class.const_defined?(:LeakedNode, false), 'the constant is bound before inherited raises'
  end

  # A forged value never reaches the core: an instance of a leaked subclass
  # that skipped the check is a TypeError wherever it is read, and a Marshal
  # frame edited past the check is a RangeError from the core's constructor.
  def test_a_forged_value_is_refused_at_the_boundary
    assert_raises(TypeError) { self.class.module_eval('class ForgedRequest < Request; end', __FILE__, __LINE__) }
    ForgedRequest.class_eval do
      def initialize(**fields) = Data.instance_method(:initialize).bind_call(self, **fields)
    end
    forged = ForgedRequest.new(session: 0, node: Node::ROOT)
    shape = Schema.new(2, 2, 1, 1)
    survey = Survey.new
    assert_equal :diverged, survey.begin(shape, shape, Versions.new(1, 1), 'a', 'b')
    assert_raises(TypeError) { survey.answer(forged, Versions.new(1, 1), %w[a b], %w[a c]) }
    assert_raises(TypeError) { survey.requeue(forged) }

    deep = Marshal.load(Marshal.dump(Node.new(9, 0)).sub("i\x0E".b, "i\x02\xE7\x03".b))
    assert_equal 999, deep.level
    assert_raises(RangeError) { shape.span(deep) }
    assert_raises(RangeError) { Request.new(1, deep) }
    wide = Marshal.load(Marshal.dump(shape).sub("i\x07".b, "i\x02\xE7\x03".b))
    assert_raises(RangeError) { survey.begin(wide, shape, Versions.new(1, 1), 'a', 'b') }
    assert_equal [1, 0], [survey.pending, survey.in_flight]
  end

  # A frozen survey raises FrozenError before any argument is read.
  def test_a_frozen_survey_raises_before_reading_arguments
    survey = Survey.new.freeze
    assert_raises(FrozenError) { survey.begin(nil, nil, nil, nil, nil) }
    assert_raises(FrozenError) { survey.answer(nil, nil, nil, nil) }
    assert_raises(FrozenError) { survey.requeue(nil) }
  end

  # The classes are resolved at init and pinned, so a compaction moves none
  # of them: every path that builds or reads a value still works after it.
  def test_every_value_path_survives_gc_compact
    skip 'GC.compact is not available' unless GC.respond_to?(:compact)
    GC.compact
    shape = Schema.new(2, 2, 1, 1)
    survey = Survey.new
    assert_equal :diverged, survey.begin(shape, shape, Versions.new(1, 1), 'a', 'b')
    request = survey.next_request
    assert_equal Request.new(1, Node::ROOT), request
    assert_equal 0...4, shape.span(request.node)
    assert_equal :accepted, survey.answer(request, Versions.new(1, 1), %w[a b], %w[a c])
    assert_equal :requeued, survey.requeue(survey.next_request)
  end

  def test_hooks_are_private
    %i[check_node check_versions check_request request_scope fields].each do |hook|
      assert_raises(NoMethodError) { MerkleMachines.public_send(hook, 1, 1) }
    end
  end
end
