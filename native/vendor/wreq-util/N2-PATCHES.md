# Local dependency patch

Upstream: wreq-util 0.2.0 (Apache-2.0), crates.io release, Git revision
`de0dc6349bc2d4d1bd0bbbaa059b45ab1df3b0fd`.

The browser emulation source and license are unchanged. Both manifests raise
the optional TLS certificate compression dependency from zstd 0.13.3 to 0.14.3,
which incorporates the fixed zstd-safe 8.2 API. This removes the old compression
dependency while preserving all upstream profiles and algorithms. The locked
application build, transport regression tests and public search smoke checks
are the integration checks; they do not cover every emulated browser profile.

Remove this patch once an upstream compatible release includes the update.

Packaging uses the application's native/Cargo.lock. The upstream crate's
development Cargo.lock and the local registry cache marker are omitted.
