# Changelog

## [0.4.0](https://github.com/seuros/merkle_machines/compare/merkle-machines-v0.3.0...merkle-machines-v0.4.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. requeue returns Result<(), Refusal>; abort returns Aborting; Schema.new raises where the core has no schema.

### Features

* requeue and abort answer their real outcome, Ruby Request carries a Node and drain replaces take_ranges ([445d667](https://github.com/seuros/merkle_machines/commit/445d667f19c51a2bbf225af73d2897420a9a4d9a))

## [0.3.0](https://github.com/seuros/merkle_machines/compare/merkle-machines-v0.2.0...merkle-machines-v0.3.0) (2026-11-13)


### Features

* implement Default for LeafRange and name the drain buffer out ([d6e4c5e](https://github.com/seuros/merkle_machines/commit/d6e4c5ec9b67a705d52dcffea34d3a95fad059e7))

## [0.2.0](https://github.com/seuros/merkle_machines/compare/merkle-machines-v0.1.0...merkle-machines-v0.2.0) (2026-11-12)


### Features

* initial extraction ([43dd5e9](https://github.com/seuros/merkle_machines/commit/43dd5e92b117d600f3828fb106b9e4fa798f3f5e))
