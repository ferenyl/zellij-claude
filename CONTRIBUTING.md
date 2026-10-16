# Contributing

## Setup

- Rust: the version in `rust-toolchain.toml`. With rustup it is installed automatically,
  including the `wasm32-wasip1` target, `rustfmt` and `clippy`. Without rustup, install
  a matching toolchain and the wasm target (on Arch: `rust` and `rust-wasm`).
- `jq` for the hook script and its tests.
- Optional: `shellcheck`, `cargo-deny`, Zellij 0.45.1 for manual and end-to-end testing.

## Layout

- `crates/core`: all logic, no Zellij host calls. Builds and tests natively.
- `crates/plugin`: the wasm `ZellijPlugin` shell. Keep it thin; put logic in `core`.
- `scripts/`: hook script and installer.
- `examples/`: configuration examples, validated by tests.
- `tests/hook/`, `tests/e2e/`: shell-based tests.

## Build

```sh
cargo build --release --target wasm32-wasip1 -p claude-sessions-plugin
# -> target/wasm32-wasip1/release/claude-sessions.wasm
```

Load a local build in a running Zellij session:

```sh
zellij action start-or-reload-plugin "file:$PWD/target/wasm32-wasip1/release/claude-sessions.wasm"
```

## Tests

CI runs formatting, clippy, `cargo test`, shellcheck and cargo-deny. The wasm plugin is
only built by the release workflow, so build it locally before opening a pull request.
Run the same locally before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked --target wasm32-wasip1 -p claude-sessions-plugin
shellcheck scripts/*.sh tests/*/*.sh
cargo deny check
```

`cargo test` includes:

- unit tests in each `core` module,
- snapshot tests of picker and bar rendering (`crates/core/tests/render_snapshots.rs`, using
  [insta](https://insta.rs)). After an intended rendering change, review and accept with
  `cargo insta review`, or `INSTA_UPDATE=always cargo test` and check the `.snap` diffs,
- validation of every file in `examples/` and of the README configuration table.

Run these locally; they are not part of CI:

```sh
sh tests/hook/test_hook.sh   # hook script with a stub zellij
sh tests/e2e/run.sh          # real, isolated Zellij; needs zellij 0.45.1 and `script`
```

## Code style

- `cargo fmt` formatting, no clippy warnings.
- Doc comments on public items. Where behavior depends on a Zellij API call, a short
  comment names the call.
- Tests use Arrange/Act/Assert and no mocks unless needed.
- New config keys: add them to `config::KEYS`, the parser, the README table and, when
  useful, `examples/`. Tests fail if the README table and `KEYS` differ.

## Dependencies

Dependabot opens one PR for Cargo and one for GitHub Actions every Wednesday 05:00
(Europe/Stockholm), each assigned to
and with review requested from the code owner (`.github/CODEOWNERS`).
`zellij-tile` only gets patch updates automatically: a minor bump targets a new Zellij
release and needs a code and compatibility review (update the README compatibility
table).

Dependabot does not update the Rust toolchain. Bump `rust-toolchain.toml` manually
every few months, and fix any new clippy warnings in the same PR.
