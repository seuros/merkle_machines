# frozen_string_literal: true

require_relative 'support'
require 'open3'
require 'rbconfig'

# The class of the survey the extension wraps is fixed when it loads. Nothing
# a caller does later, redefining the constant or `Object.const_get`, reaches
# a survey being built. Each case runs in a fresh process: the class is fixed
# once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'merkle_machines'
    M = MerkleMachines
    shape = M::Schema.new(fanout: 2, depth: 2, partitioning: 1, digest: 1)
    pins = M::Versions.new(local: 40, remote: 57)
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Survey constant redefined before the first survey made it an
  # instance of the new class, without any of the gem's methods.
  def test_a_redefined_survey_constant_does_not_change_the_survey_class
    assert_equal 'true true true diverged 5 2', isolated(<<~RUBY)
      original = M::Survey
      M.send(:remove_const, :Survey)
      M.const_set(:Survey, Class.new)
      surveys = [original.new, original.with_scope(5), original.resumed(5, 2)]
      fresh, scoped, resumed = surveys
      puts [*surveys.map { |s| s.instance_of?(original) }, fresh.begin(shape, shape, pins, 'a', 'b'),
            scoped.scope, resumed.issued].join(' ')
    RUBY
  end

  # Bug: building the first survey ran `Object.const_get`, a caller's code in
  # the middle of a native call.
  def test_building_a_survey_runs_no_const_get
    assert_equal '0 false Survey Survey Survey', isolated(<<~RUBY)
      hits = 0
      log = []
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        log.freeze
        super(*args)
      end
      surveys = [M::Survey.new, M::Survey.with_scope(5), M::Survey.resumed(5, 2)]
      puts [hits, log.frozen?, *surveys.map { |s| s.class.name.split('::').last }].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal 'diverged 5 2', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran mid-call' }
      fresh = M::Survey.new
      puts [fresh.begin(shape, shape, pins, 'a', 'b'), M::Survey.with_scope(5).scope,
            M::Survey.resumed(5, 2).issued].join(' ')
    RUBY
  end
end
