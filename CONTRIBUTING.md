# Contributing to memtree

memtree is a small, dependency-free tool with one contract: `README.md` describes the store
format, every finding code, each command's output, and the exit statuses. A change to any of
them updates `README.md`, the help text in `src/main.rs` when it is affected, and the tests, in
the same commit.

## Before opening an issue

- Confirm you are on an Apple Silicon Mac; other targets are unsupported.
- Reproduce against current `main` with the pinned Rust toolchain (`rust-toolchain.toml` selects
  it automatically).
- Remove private paths, prompts, and store contents from anything you share.

Include the command, its exit status, the expected and actual output, and your macOS and Rust
versions. Use a disposable store built in a temporary directory whenever possible.

## Development setup

```sh
git clone https://github.com/0xendale/memtree.git
cd memtree
cargo build
cargo test
```

One crate, no dependencies, runtime or dev. A commit that adds one explains why in its body.

## Change workflow

1. Write the failing test first; confirm it fails on an assertion, not a compile error.
2. Implement the smallest change that makes it pass.
3. Run the gate, with `LC_ALL=C TZ=UTC`:

   ```sh
   cargo test
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps
   cargo metadata --locked --format-version 1 > /dev/null
   ```

4. One atomic conventional commit (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `ci:`,
   `chore:`) with tests, code, and documentation together. Never commit a red tree.
5. A focused pull request explaining behavior, risks, and verification.

## Code requirements

- No unsafe code, no `unwrap`, `expect`, `panic`, `todo`, or `dbg!` in product code; test code
  may opt out with `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`.
- Output goes through `io::stdout()` and `io::stderr()` handles, never `println!` or `eprintln!`.
- Results to stdout, messages to stderr prefixed `memtree: `; a failure is never silent.
- Deterministic output: sorted, no timestamps, no colour, notes named by store-relative paths.
- `index` preserves by default. Anything that removes, rewrites, or reorganises content is
  explicit, opt-in, and reversible.
- `affected` uses only Git plumbing that never refreshes or rewrites the index. Probe Git
  behaviour in a scratch repository and pin it with a test before relying on it.
- Never mention an agent, assistant, or AI in commits, bodies, trailers, or pull requests.
