# frozen_string_literal: true
require_relative 'lib/merkle_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'merkle_machines'
  spec.version = MerkleMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']
  spec.summary = 'Initial bounded digest-tree comparison with a native Rust core'
  spec.description = 'Unstable local survey of differing leaf ranges between two digest trees; no hashing or transport.'
  spec.homepage = 'https://github.com/seuros/merkle_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'
  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'merkle_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/merkle_machines_native/ffi/Cargo.toml'
  spec.files = Dir.glob(%w[
    lib/**/*.rb sig/**/*.rbs ext/**/*.{rs,rb,toml,md} Cargo.toml
    Cargo.lock README.md CHANGELOG.md LICENSE.txt
  ]).select { |p| File.file?(p) }
    .reject { |p| p.end_with?('/tests.rs') || p.include?('/tests/') }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/merkle_machines_native/extconf.rb']
  spec.add_dependency 'rb_sys', '~> 0.9', '>= 0.9.130'
end
