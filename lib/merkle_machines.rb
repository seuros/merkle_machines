# frozen_string_literal: true

require_relative 'merkle_machines/version'
require_relative 'merkle_machines/values'
# Native is required: no algorithm fallback. It resolves the value classes
# above once, at init.
require 'merkle_machines_native/merkle_machines_native'
require_relative 'merkle_machines/survey'
