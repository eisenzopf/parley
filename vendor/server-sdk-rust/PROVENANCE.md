# Conference dependency snapshot

This is the Vapi SDK source snapshot already bundled with Parley, now consumed
directly by its Cargo path dependency. It corresponds to the local SDK checkout
at commit `02d61a635e3d40bbdb9ba64aad5515fc8b41d4be`, plus the existing local
Rustls ring-provider initialization in `src/client.rs` and its manifest dependency.
It is not claimed as an unmodified published crate or a new Vapi release.

The MIT license is preserved in `LICENSE`. Package metadata and original project
documentation are retained. The Parley root `Cargo.lock` governs builds of Parley;
the SDK's standalone lockfile is not used by that build.

Expected manifest, license, and Rust source hashes are recorded in
`config/conference-dependencies.json` at the Parley root. Verify them with
`node scripts/verify-conference-dependencies.mjs`. Changes to this snapshot must
be deliberate and reflected in that manifest and verification evidence.
