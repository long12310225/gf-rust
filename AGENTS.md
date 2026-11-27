# Agent notes for gf-rust

This repository is a pure Rust port of the GoFrame framework. The Go source at
`D:\github\gf` (`github.com/gogf/gf/v2`, v2.10.5) is **read-only reference** and
is never modified from here (ADR-001).

Start with [`HANDOFF.md`](HANDOFF.md) for where the port stands, [`MAP.md`](MAP.md)
for the crate map and unit states, [`whole-app.md`](whole-app.md) for the
measured wave plan (320 units), and [`DECISIONS.md`](DECISIONS.md) for every
behavioural decision made so far.

## Working a unit

1. Pick the next unit from `HANDOFF.md` "Next steps" or the lowest `open` unit
   in `MAP.md`. Do not jump waves without an ADR.
2. Read the Go package and its `*_test.go` files completely before writing code.
3. Add one module per Go package to the family crate (`container/gpool` becomes
   `crates/gf-container/src/gpool.rs`), and register it in that crate's `lib.rs`.
   A package that imports a crate higher in the graph gets its own crate
   instead of a cycle: `util/gvalid` lives in `gf-valid` because it needs
   `os/gtime` and `net/gip*` (ADR-039).
4. Translate the Go tests into a `#[cfg(test)] mod tests` block in the same file.
   Every Go assertion must have an equivalent assertion; rename the Rust test
   `test_<go_test_name>` in `snake_case`. When a Go test only works because of
   sleep-based timing, replace the sleeps with channels/barriers and note that in
   a comment.
5. Run the gates below. A unit is `gated` only when all three are clean.
6. Update the documents in the same change: `MAP.md` (unit row plus evidence
   count), `HANDOFF.md` (state and next steps), `DECISIONS.md` (one ADR per
   divergence), `GLOSSARY.md` (new type names, if any).

## Gates

```powershell
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Performance-sensitive units also get a bench case in `crates/gf-bench` and a row
in `BENCH.md` (`cargo run --release -p gf-bench`, Go side in `bench/go`).

## Conventions

- Go names survive where users see them: types keep `GoCase` (`TPool`), and
  functions/methods are `snake_case` while constants are `UPPER_SNAKE`
  (ADR-005). Port-only helper types are marked as such in `GLOSSARY.md`
  (`Ttl`, `TreeComparator`, `TreeKey`, `FromValue`).
- Every public item has a doc comment. A module doc states what diverges from
  Go and points at the ADR.
- Errors are `gf_errors::gerror::Error`, built with
  `gerror::new_code(&gcode::CODE_*, &["message"])`; never strings, never
  `unwrap()` in library code (ADR-006). Tests use `expect("what")`.
- User callbacks never run while holding a lock (ADR-007). Snapshot, release,
  call.
- No `unsafe`. If it ever becomes necessary: minimal, isolated, `// SAFETY:`.
- `#[expect(lint, reason = "...")]` instead of `#[allow]`.
- Floats print through `gf_core::float` (`format_g` for Go's `'g'`, `format_f`
  for `'f'`), never through bare `format!("{}", f)` (ADR-037).

## rust-anything

The full workflow used for this port lives in
[`.kilo/skills/rust-anything/SKILL.md`](../.kilo/skills/rust-anything/SKILL.md)
and its whole-app companion `whole-app.md`. The CLI is installed
(`rust-anything 3.4.0`) and the per-unit harnesses live under `verify/<unit>`
(excluded from the workspace). The Go `cgo` lane needs `CGO_ENABLED=1` and
`C:\msys64\mingw64\bin` on `PATH`.

Bridged units are re-checked with:

```powershell
rust-anything record <crate>
rust-anything diff <crate>
rust-anything gate <crate>
```

## Environment notes (Windows)

- The tool shell is Windows PowerShell 5.1: rewriting source or Markdown files
  through `Set-Content`/`-replace` mangles non-ASCII bytes (it once truncated a
  `→` in `MAP.md`). Use the editor tools for file content, not shell redirection.
- `cargo` writes progress and warnings to stderr, so a `cargo ... | Select-Object`
  pipeline reports a non-zero exit code even on success. Confirm with
  `cmd /c "cargo test --workspace > nul 2>&1 & echo exit=%ERRORLEVEL%"`.
- Go 1.25 is installed at `C:\Program Files\Go\bin\go.exe`; the editor session
  may not have it on `PATH`.
