# Decision records

One section per decision, numbered in order, never renumbered. A changed
decision gets a new ADR that supersedes the old one.

## ADR-001: Target repository and read-only reference

Status: accepted (2026-11-24)

Context: The Go source is `D:\github\gf` (`github.com/gogf/gf/v2`, v2.10.5),
pinned at commit `1020b0afa19312373f6d7448768cdb5aa77a980d`
(`v2.10.5-22-g1020b0afa`, 2026-11-24). Moving the baseline requires updating this
ADR, `README.md` and `HANDOFF.md`.
The Rust port lives in a separate, currently empty repository, `D:\github\gf-rust`.
In-repo tooling (`.git`, `AGENTS.md`, Go toolchain rules) belongs to the Go
project; `rust-anything`'s whole-app workflow otherwise expects a `rust/` tree
inside the source repository.

Decision: The port is developed in `gf-rust` as a Cargo workspace at its root.
The Go repository is read-only reference and is never modified. Analysis
artifacts (`whole-app.md`, `research/`, `app-cases/`) live in `gf-rust`.

Consequences: `rust-anything app front-door/lift` (which scaffold inside the
source repo) are not used as-is; unit-level tooling (`bridge`, `record`, `diff`,
`gate`) is used with crates in this workspace once the Go/C toolchain exists.

## ADR-002: Scope is the whole framework, in base-first order

Status: accepted (2026-11-24)

Context: 31 packages, 1036 source files, ~127k lines, 320 units. The user chose
a full port, with the base layer first.

Decision: Port the core module (`github.com/gogf/gf/v2`) first in wave order,
then `cmd/gf`, then `contrib/*`. Within the core, follow `whole-app.md`'s waves:
helpers and encodings before containers, containers before `gconv`, `gconv`
before everything that coerces values, and `net/ghttp` after `os/gcfg`,
`os/glog`, `os/gres`, `os/gview`.

Consequences: The framework becomes usable incrementally (utilities → containers
→ config/log → HTTP). Database drivers and registries are last, which also
defers most third-party Rust dependency research.

## ADR-003: One crate per Go package family

Status: accepted (2026-11-24)

Context: Rust crates cannot have import cycles; Go packages form a DAG. Go
package families (`container/*`, `os/*`, ...) are large; a single crate would
serialize compilation and force whole-family edits per unit.

Decision: Group crates by Go top-level family (`gf-container`, `gf-os`,
`gf-encoding`, ...), with one module per Go package inside the crate
(`gf::container::gmap`). A crate is added when its first unit is ported. The
facade crate `gf` re-exports each family so users write the Go path:
`gf::encoding::ghash::bkdr`.

Consequences: A family-internal dependency cycle across two family crates would
force a split; none is expected because Go itself forbids the cycle. A single
Go package that must be split out (e.g. `gf-database` needing `gdb` before
`gredis`) becomes its own crate.

## ADR-004: One dynamic value type replaces `any`

Status: accepted (2026-11-24)

Context: `gconv`, `gvar`, the containers and `gjson` all pass `any` values and
preserve their dynamic type through copies. Rust generics cannot store mixed
types in one map.

Decision: [`gf_core::Value`](../crates/gf-core/src/value.rs) is the single
dynamic representation: `Null`, `Bool`, `Int(i64)`, `UInt(u64)`, `Float(f64)`,
`String`, `Bytes`, `List`, `Map(IndexMap<String, Value>)`. Maps keep insertion
order; `Display` mimics Go's `%v` for these variants and sorts map keys the way
`fmt` does. Numeric coercion rules are defined with `gconv` (wave 9), not here.

Consequences: Generic Go APIs are ported once over `Value` instead of being
generic over `T`. Typed helpers (`gvar::Var`) wrap `Value` and add accessors.
Known difference: large/small float formatting uses Rust's shortest form.

## ADR-005: Keep Go names, adapt the case

Status: accepted (2026-11-24)

Context: Users read GoFrame documentation; type and package names are part of
the mental model. The analyzer reports every unit by Go path.

Decision: Types keep Go names (`LocalCode`, `StackMode`, `Value`); functions and
methods use Rust `snake_case` (`parse_using_default_algorithm`,
`is_stack_mode_brief`); constants use `UPPER_SNAKE` (`CODE_INTERNAL_ERROR` where
Go has `CodeInternalError`). Modules mirror Go package names unchanged
(`gmap`, `gstr`, `gcode`). `GLOSSARY.md` records each mapping.

Consequences: Code searches by Go name still work on types and modules; function
searches need the snake_case form.

## ADR-006: Errors are typed, not strings

Status: accepted (2026-11-24)

Context: GoFrame's `gerror` carries a `gcode.Code`, a message, an optional
detail and a stack. The `rust-anything` bridge convention (`Err("Type: msg")`)
only applies to values crossing a language boundary; this port has no boundary.

Decision: Port `errors/gcode` as a `Code` trait plus `LocalCode`; port
`errors/gerror` as a `GError` struct implementing `std::error::Error`, holding
code, message, detail (`Option<Value>`) and a captured stack description.
Functions return `Result<T, GError>`; `panic!`/`unwrap` are only allowed for
states the Go original also treats as impossible, marked `// INFALLIBLE:`.

Consequences: Error text formatting must match `gerror`'s `String`/`Error`
output because tests assert on it. Stack traces are textual, not typed frames.

## ADR-007: Containers are always synchronized

Status: accepted (2026-11-24)

Context: Go containers take `safe ...bool` and default to unsynchronized. Rust
requires `Sync` for shared references, so an unsynchronized container type
cannot be shared across threads without `unsafe`; the port forbids `unsafe`.

Decision: Every ported container is internally synchronized (`RwLock` or
`Mutex`) regardless of the `safe` argument, which is accepted for signature
parity and ignored. Iteration returns snapshots or guards; no lock is held
across caller code.

Consequences: Behavior matches the `safe=true` Go container for all observable
results. Performance differs from Go's default (unsafe) containers; if a
benchmark makes this matter, add explicit `Unsafe*` variants with documented
single-threaded requirements instead of weakening safety.

## ADR-008: Equivalence proof is translated Go tests, then record/diff/gate

Status: accepted (2026-11-24)

Context: This machine has no Go toolchain and no C compiler yet, so
`rust-anything record/diff/gate` for Go units cannot run. The user is installing
Go; the framework also has a large test suite written with `gtest`.

Decision: Every ported unit translates its Go `*_test.go` cases into Rust tests
in the same module, including golden values, error paths and edge cases.
`rust-anything record/diff/gate` is added per unit once Go and a C compiler are
available; until then a unit is `gated`, not `recorded`.

Consequences: Test names cite the Go test they came from. A port is not called
complete on `cargo test` alone once recording is available. Tests that need
backing services (MySQL, Redis, etcd) stay out of the default suite and become
ignored tests with the service requirement documented.

## ADR-009: HTTP is axum-based and comes late

Status: accepted (2026-11-24)

Context: `net/ghttp` is 8.8k lines over Go's `net/http`, with middleware,
sessions, routing groups, OpenAPI (`net/goai`) and graceful restart. Go's
handler signature (`func(ctx context.Context, r *Request)`) has no direct axum
equivalent.

Decision: Port `ghttp` after the core value/config/log/view layer, on `axum` +
`tokio` + `tower`, preserving routes, middleware order, status codes, headers
and body formats. The `Request`/`Response` wrappers are ported as types over
`http` types rather than exposing axum types publicly.

Consequences: Go handler registration becomes a Rust builder/trait API; the
glossary records the exact mapping. HTTP-level equivalence is checked with
request/response cases, not Go unit tests alone.

## ADR-010: Rust dependency picks

Status: accepted (2026-11-24); refined per unit by `research/` notes

Context: `whole-app.md` maps Go modules to candidate crates, most marked
`unknown`. Heavy services (OTel, registries, DB drivers) are not needed until
their waves.

Decision: Start with these picks, each confirmed by a `research/` note before
the unit that uses it:

| Go | Rust |
| --- | --- |
| `sync`, `golang.org/x/sync` | `std::sync`, `tokio::sync` |
| `regexp` | `regex` (ASCII classes where Go's `\w` is ASCII) |
| `encoding/json` (ordered) | `serde_json` + `indexmap` |
| `net/http` server | `axum` + `hyper` + `tower` |
| HTTP client | `reqwest` |
| `database/sql` layer | `sqlx` (async) with driver crates per `contrib/*` |
| `time` | `chrono` for formatting, `time` where strict parsing is needed |
| `crypto/*` | `aes`, `des`, `rsa`, `sha1`, `sha2`, `md-5`, `crc32fast` |
| YAML/TOML/XML/INI/properties | `serde_yaml`/`serde_yml`, `toml`, `quick-xml`, `rust-ini`, `java-properties` |
| OTel | `opentelemetry*` crates (research pending) |

Consequences: `Value` stays independent of `serde_json::Value`; conversions
live in `gjson`/`gconv`. Crates that do not match semantics (e.g. `regex`
Unicode `\w`) get explicit wrappers and trap tests.

## ADR-011: Go-runtime helpers are replaced, not ported

Status: accepted (2026-11-24)

Context: Several `internal/` packages exist only because Go needs reflection
and pointer-safe locking at run time. Their APIs cannot be expressed in safe
Rust without inventing a runtime the port does not have:

- `internal/reflection` converts `reflect.Value`/`reflect.Type` and walks
  pointer chains.
- `internal/mutex`, `internal/rwmutex` are `sync.Mutex` wrappers with a `safe`
  switch; ADR-007 already settles locking.
- `internal/utils` predicates such as `IsStruct` and `CanCallIsNil` inspect Go
  kinds; a `Value` has no pointer, channel, function or struct kind.

Decision: Do not create Rust modules for `internal/reflection`,
`internal/mutex`, `internal/rwmutex`, `CanCallIsNil` and `IsStruct`. Port every
other function of `internal/utils` over the `Value` type, `&str` and `&[u8]`
(`is_int`, `is_uint`, `is_float`, `is_slice`, `is_map`, string helpers,
`ReadCloser`, list/map helpers, debug flags). `internal/fileinfo` is ported as a
plain struct with Go's `os.FileMode` bit layout and `SystemTime`.

Consequences: `MAP.md` marks these units `replaced`, not `open`, so they do not
block the waves. `IsStruct` returns once `gconv` fixes a struct representation
(likely a separate `Value::Struct` variant); until then struct-to-map conversion
is designed with `gconv`, not guessed here.

## ADR-012: `errors/gerror` needs its own error model before it is ported

Status: accepted (2026-11-24)

Context: Go's `gerror` wraps `error` values, discovers `ICause`/`IUnwrap`/`ICode`
with interface assertions, formats with `fmt.Sprintf` verbs, and captures
`runtime.Caller` stacks filtered by GoFrame and GOROOT paths. Rust has no
`error` interface, no runtime format strings and no Go stack layout.

Decision: Before porting `gerror`, write an ADR that fixes: the error chain type
(`Error` holding a wrapped `Box<dyn std::error::Error>` and/or inner `Error`
layers), the verb subset the port's formatter supports (`%v`, `%s`, `%d`, `%q`,
`%t`, `%f`, `%+v`, `%%` plus width/precision) and its divergence from Go for
unsupported verbs, and how `Stack()` renders (captured backtrace frames versus
Go's `function` / `file:line` lines). `errors.Is`/`As` map to source-chain
walks and `downcast_ref`.

Consequences: `errors/gerror` stays `open` until that decision is accepted and
recorded as an ADR. Units that only need error codes can depend on
`gf-errors::gcode` meanwhile.

Implementation (2026-11-24): `Error` stores `Option<Box<dyn std::error::Error +
Send + Sync>>` as its wrapped cause and implements `std::error::Error` with
`source()` returning it, so the chain is walkable by std tools. Text formatting
is `gfmt::sprintf` with `%v %s %d %f %t %q %x %c %%`, flags `- + 0`, width and
precision; unsupported verbs render `%!verb(type=value)`. `Stack()` renders
GoFrame-style numbered layers and frames from `std::backtrace::Backtrace`,
filtering `gf_*` frames in brief mode. Known divergences, all documented in
the module: `errors.Is`/`equal` compare code and current-level text instead of
Go identity; there is no nil receiver (the `wrap*` family returns
`Option<Error>`); assignment of `skip` only trims rendered frames.

## ADR-013: Option parsing is a hand-written scanner, proven against the Go regex

Status: accepted (2026-11-24)

Context: `internal/command` parses every process argument with
`^-{1,2}([0-9A-Za-z_?.\-]+)(=)?(.*)$`. The first port ran the `regex` crate on
every argument, which cost 2.1 µs per 7-argument parse — 15% slower than Go's
compiled `regexp` in the shared benchmark.

Decision: Replace the runtime regex with `match_option`, a byte scanner that
reproduces the pattern exactly, including the greedy-then-fallback dash match
(`--` parses as option name `-`), the ASCII name class, the optional `=`
separator and the rejection of arguments containing `\n`. `regex` stays a
dev-dependency, and a test compares the scanner with the pattern over a
600-input corpus (prefixes, bodies, tails, non-ASCII, newlines).

Consequences: Parsing is 2.16x faster than the Go original (0.92 µs vs 1.98 µs)
and `gf-internal` no longer builds `regex` for production. Any future change to
the parser must keep the differential test green; new patterns elsewhere in the
framework still use `regex` where its semantics are needed.

## ADR-014: JSON matches Go byte for byte, without an intermediate tree

Status: accepted (2026-11-24)

Context: `internal/json` wraps `encoding/json`, whose defaults are observable:
strings escape `<`, `>`, `&`, U+2028 and U+2029; map keys are sorted;
`[]byte` is encoded as BASE64; `RawMessage` preserves original text. `serde_json`
does none of those by default, and routing `Value` through
`serde_json::Value` would double the work.

Decision: Port `internal/json` in a new crate `gf-json` because `internal/json`
cannot live in `gf-internal` (that crate cannot depend on `gf-errors`).
`gf-core` implements `Serialize`/`Deserialize` for `Value` behind a `serde`
feature (a visitor maps JSON straight into `Value`). `gf-json` then serializes a
thin `GoValue` wrapper with a `serde_json::Formatter` that HTML-escapes string
fragments, sorts `Value::Map` keys, and writes `Value::Bytes` as a BASE64
string. `marshal_indent` uses `PrettyFormatter` plus prefix handling and the
same escaping pass. Errors are wrapped with Go's texts (`json.Marshal failed`,
`json.Unmarshal failed`, `json.MarshalIndent failed`).

Consequences: Output matches `encoding/json` for the values the port carries,
at half to a third of Go's cost in the shared benchmark. `unmarshal` keeps
document order in `Value::Map` (Go discards order); encoding sorts, so output
is stable. `UnmarshalUseNumber` is satisfied by `Value` keeping integers and
floats apart.

## ADR-015: URL and BASE64 decoding expose byte and UTF-8 forms

Status: accepted (2026-11-24)

Context: Go returns `string` from `url.QueryUnescape`, `url.PathUnescape` and
`base64` decoding even when the bytes are not valid UTF-8. A Rust `String`
cannot hold those bytes, so returning `String` alone would either panic or
silently replace data.

Decision: Every decoding API has two forms: the Go-shaped one returns
`Result<String, Error>` and rejects invalid UTF-8 with a clear message; the
`*_bytes` companion returns the exact byte sequence Go produces.
`gurl::parse_url` returns `HashMap<&'static str, Cow<'a, str>>`: keys are
static and values borrow the input unless percent-decoding was required, which
keeps parsing allocation-light. `build_query` takes a `BTreeMap` (Go sorts keys
in `Values.Encode`). BASE64 keeps Go's newline tolerance in `Decode`.

Consequences: Callers that need byte fidelity use the byte forms; callers with
textual URLs get a typed error instead of Go's silently-invalid string. The
shared benchmark shows URL parsing 1.4x faster and BASE64 encoding 1.9x faster
than Go ([BENCH.md](BENCH.md)).

## ADR-016: gregex rewrites Go's ASCII classes

Status: accepted (2026-11-24)

Context: Go's `regexp` uses ASCII character classes (`\w`, `\d`, `\s`) and
matches `.` against one UTF-8 rune; it supports `\Q...\E`; it has no
backreferences or lookaround. Rust's `regex` matches Unicode classes by default
and does not understand `\Q...\E`. Compiling with Unicode off fixes the classes
but makes `.` byte-oriented and rejects patterns that can match invalid UTF-8.

Decision: Compile with the normal Unicode engine and rewrite only what differs:
`\w \W \d \D \s \S` become explicit ASCII classes (positive forms also inside
classes), and `\Q...\E` is expanded with metacharacter escaping. The cache is
keyed by the original pattern, so the rewrite happens once. Byte APIs decode
their input as UTF-8 text (lossy), which matches how Go's engine consumes
`[]byte`; a complemented class inside a class such as `[\W]` keeps its Unicode
meaning and is a documented divergence.

Consequences: All Go cases pass, including `Split` with empty patterns and
group trimming. The shared benchmark shows matching about 5x faster than Go,
mostly because a cached compiled program needs no per-call lookup.

## ADR-017: Hash engines are ring for SHA and md-5 for MD5

Status: accepted (2026-11-24)

Context: Go's `crypto/md5` and `crypto/sha*` have assembly implementations that
use SHA-NI and vector instructions. The pure-Rust `sha2` crate with default
features was 1.8x slower than Go for 1 KiB, while `ring` uses the platform's
accelerated instructions.

Decision: Use `ring` for SHA-1 (legacy API), SHA-256 and SHA-512, and the
`md-5` crate for MD5. File hashing streams 64 KiB chunks with the same
`os.Open failed for name "..."` / `io.Copy failed` error texts. The Go
`Encrypt(any)` entry points wait for `gconv`; the port exposes
`encrypt_bytes`/`encrypt_string` until then, because Go's `gconv.Bytes` produces
platform-specific binary bytes for integers and `fmt` output for structs.

Consequences: SHA-256 is at parity with Go (3.4 µs vs 3.3 µs per KiB on this
machine); MD5 stays about 1.2x slower (2.0 µs vs 1.7 µs). Enabling `sha2`'s
`asm` feature would need nasm on Windows and is not used.

## ADR-018: ghtml covers the common named entities

Status: accepted (2026-11-24)

Context: Go's `html.UnescapeString` decodes roughly 2200 named entities. The
GoFrame tests exercise the five escaped characters plus numeric forms. A full
table is a large dependency for behavior that only matters when users feed
arbitrary HTML entities to `ghtml`.

Decision: Decode numeric entities (decimal/hex, with Go's U+FFFD replacement
rules) and a curated table of common named entities (the five basics,
ISO-8859-1 letters, punctuation, arrows, math and spacing). Unknown named
entities are left untouched. `SpecialCharsDecode` decodes only the five
entities, like Go's replace-based implementation. `StripTags` is a scanner that
drops tags and comments, drops script/style bodies, keeps textarea/title
content, and keeps a `<` that does not start markup.

Consequences: The Go fixtures pass, and `StripTags` is about 4x faster than
Go's `html/template`-derived state machine. Rare named entities pass through
unchanged; adding one is a single table row. Revisit if `gview` needs the full
HTML5 entity set.

## ADR-019: TOML and YAML go through serde crates

Status: accepted (2026-11-24)

Context: `gtoml` and `gyaml` wrap BurntSushi TOML and go.yaml. The port has one
dynamic type, `Value`, and already implements serde.

Decision: `gtoml` converts `Value` to `toml::Value` (and back) explicitly, so
datetimes become strings and integer width errors are reported. Top-level
scalars and arrays are rendered inline like BurntSushi does (`["a", "b"]`);
`Value::Null` panics like the Go encoder's reflection path; duplicate tables are
rejected by the parser. `gyaml` uses `serde_yaml` for encoding and decoding,
requires a mapping root like Go's `map[string]any` target, and reuses `gf-json`
for `ToJson`. Errors keep Go's `toml.Encoder.Encode failed`,
`toml.Unmarshal failed`, `yaml.Marshal failed` and `yaml.Unmarshal failed`
texts.

Consequences: TOML decoding is about 3x faster than Go and YAML about 2.6x.
`serde_yaml` is deprecated upstream; if it stops building, switch to a
maintained fork without changing this API.

## ADR-020: Randomness is a thread-local CSPRNG buffer

Status: accepted (2026-11-24)

Context: `util/grand` fills a 10 000-slot channel from `crypto/rand` in a
background goroutine and pulls one 4-byte slice per draw. The port has no
goroutine scheduler, and a shared lock would serialize every call.

Decision: Each thread keeps a 1 KiB buffer filled from `ring`'s `SystemRandom`
and consumes it four bytes at a time, mirroring Go's step of 4. All draw
functions (`intn`, `b`, `n`, `s`, `str`, `digits`, `letters`, `symbols`,
`perm`, `meet`, `meet_prob`) follow the Go arithmetic exactly. `d` takes two
`Duration`s (non-negative) and keeps Go's shared-factor-of-ten reduction; a
negative Go duration has no unsigned counterpart and is clamped.

Consequences: `Intn` is about 9.6x faster than Go on this machine because there
is no channel or shared lock. Distribution behavior matches Go (modulo bias
included, intentionally).

## ADR-021: Symmetric ciphers use RustCrypto with manual CBC loops

Status: accepted (2026-11-24)

Context: `gaes` and `gdes` must reproduce Go's byte-for-byte output, including
PKCS#5/#7 and zero padding, CFB feedback and default IV `I Love Go Frame!`.
The high-level RustCrypto CBC wrappers hide padding and error texts.

Decision: Use `aes`/`des` primitives and apply CBC as an explicit block loop
(XOR with the previous ciphertext block, then encrypt; inverse for decrypt).
CFB uses the `cfb-mode` crate's `AsyncStreamCipher`. Padding, unpadding and all
error texts are ported verbatim (`invalid unpadding`, `invalid padding`,
`unsupported padding type "%d"`, `aes.NewCipher failed for key "..."`). Tests
compare against the Go test vectors for AES-128/192/256-CBC, AES-CFB, DES/3DES
ECB and CBC.

Consequences: Output matches Go exactly. CBC encryption/decryption is about 2x
faster than Go (AES-NI through the `aes` crate) and padding errors keep their
original strings. ECB is kept for `gdes` because the Go API exposes it.

## ADR-022: Properties parsing is hand-written and reference-aware

Status: accepted (2026-11-24)

Context: `gproperties` wraps magiconair/properties, which supports comments,
`=|:`/whitespace separators, continuations, `\uXXXX`, `${name}` references and
reports malformed expressions, cycles and invalid escapes at load time.

Decision: Implement the parser directly: logical-line joining for trailing
backslashes, key/value splitting with escapes, `\u` validation, and a
`${name:default}` expander that detects cycles and malformed expressions.
Errors are wrapped with `Lib magiconair load Properties data failed.` like Go.
`decode` nests keys on `.` and lowercases the last segment; `encode` flattens
maps, lowercases leaf keys, sorts and writes `key = value\n` with escaped
control characters.

Consequences: The Go fixtures (including the `properties` value containing
comments and quotes) round-trip exactly, and decoding is ~1.8x faster than Go.
The full magiconair expression grammar (nested defaults) is not implemented;
`${name}` and `${name:default}` are.

## ADR-023: Charset conversion is bytes-first

Status: accepted (2026-11-24)

Context: `gcharset` converts between UTF-8 and GBK/GB18030/Big5/Shift_JIS and
the ISO/Windows pages. Go returns `string` values that may hold non-UTF-8
bytes; Rust `String` cannot.

Decision: `convert`, `utf8_to` and `to_utf8` work on `&[u8]` / `Vec<u8>`;
`convert_string` is the UTF-8-to-UTF-8 text convenience and errors when the
result is not UTF-8. `encoding_rs` supplies the tables, `GB2312` aliases to
HZ-GB-2312 like Go, and HZ is implemented on top of GBK (7-bit escapes, `~{`,
`~}`, `~~`, no trailing `~}`). UTF-16LE/BE and BOM-prefixed UTF-16 are encoded
manually because `encoding_rs` encoders only emit UTF-8. Error texts match Go
(`unsupported srcCharset "%s"`, `unsupported dstCharset "%s"`).

Consequences: Round trips match the Go vectors (Big5, GBK, GB18030, HZ, UTF-16
with and without BOM, ISO-8859-15, KOI8-R, EUC-JP). GBK-to-UTF-8 is about 15x
faster than Go because `x/text` streams through `transform.Reader`.

## ADR-024: XML follows mxj without its option surface

Status: accepted (2026-11-24)

Context: `gxml` wraps mxj: root-per-map, repeated children as lists, attributes
prefixed `-`, mixed text under `#text`, sorted output, an optional root tag,
and a global `XMLEscapeChars` switch. Its full option API (path selectors,
custom casters) is not used by the framework tests.

Decision: Parse with `quick-xml` into an owned tree and convert it to `Value`
with mxj's default conventions; encode from `Value` with sorted keys and the
global escape switch. The XML declaration is detected and, when its encoding is
not UTF-8, the remaining bytes are converted through `gcharset`. Errors are
wrapped with `mxj.NewMapXml failed` / `mxj.Map.Xml failed`.

Consequences: All Go fixtures pass (including entity round trips and the
GBK-declared document), decoding is about 2.3x faster and encoding 1.6x faster
than Go. mxj's path-selection helpers are not offered; add them only if a unit
needs them.

## ADR-025: RSA uses the `rsa` crate with PEM helpers

Status: accepted (2026-11-24)

Context: `grsa` parses PKCS#1 and PKCS#8/PKIX PEM keys, encrypts with PKCS#1
v1.5 or OAEP (SHA-256), generates keys in both formats, and offers base64
variants.

Decision: Use `rsa` 0.9 with `pem` for PEM framing and `rand::rngs::OsRng`.
Key parsing tries PKIX/PKCS#8 first and falls back to PKCS#1, mirroring Go's
auto-detection. Size limits (`key_size - 11`, `key_size - 2*32 - 2`) and all
error texts are ported. OAEP labels are not supported because the RustCrypto
`Oaep` type exposes only the default label; functions taking a label are
omitted rather than silently ignoring it. Key generation emits PKCS#1 PEM for
`generate_key_pair` and PKCS#8/PKIX PEM for `generate_key_pair_pkcs8`.

Consequences: Encrypt/decrypt round trips pass for both padding schemes and
both key formats, plus base64 forms and type detection. Tests generate 1024-bit
keys to stay fast; the public default remains 2048 bits.

## ADR-026: Mode detection and zip extraction safety

Status: accepted (2026-11-24)

Context: `util/gmode` auto-detects develop mode by checking that the caller's
source file exists (via `gdebug.CallerFilePath`). `gcompress` extracts zips
through `os.Root`, which refuses to follow symlinks out of the destination, and
rejects entry names that are not local (`filepath.IsLocal`).

Decision: `gmode` uses `cfg!(debug_assertions)` for the same intent (a
development build has sources) and still honours `gf.gmode` / `GF_GMODE`
through the command layer. Zip extraction normalises separators, rejects
entries whose components contain `..`, `:` or a leading slash, and after
creating each parent directory canonicalises it to prove it is still under the
destination root; escaping entries return `CodeInvalidParameter` with Go's
`zip file name "..." escapes the destination directory` text. `gdebug` is not
ported yet; when it is, it will use `Backtrace` frames for caller information.

Consequences: The Go zip-slip and symlink-escape tests are covered (traversal
rejected, writes through symlinks that leave the root rejected, inside-root
symlinks accepted), and `gmode` keeps the same public API with a documented
detection difference.

## ADR-027: gconv converts the port's value, not Go's memory

Status: accepted (2026-11-24)

Context: GoFrame's `gconv` converts any Go value, including structs and
platform memory layouts (`gconv.Bytes(int)` exposes the machine representation).
The port has one dynamic type and must stay portable.

Decision: `gconv` implements the scalar/list/map conversion rules of the Go
package over `Value`, plus a `FromValue` trait and `scan::<T>` for Rust structs
(through serde, so `#[serde(rename)]` replaces Go struct tags). `String` of a
list or map uses compact JSON with sorted keys, like the Go fallback.
`Bytes` of a number uses its decimal text instead of memory bytes; lists of
small integers map to their byte values; maps become JSON. `gjson` wraps
`Value`, exposes dotted-path access (`a.b.0.c`) with typed getters backed by
`gconv`, and detects content as JSON, YAML, TOML, INI (section headers) or
properties (requires `=`).

Consequences: All scalar/lookup semantics match the Go tests (including hex,
float truncation and bool words). The documented differences are numeric
`Bytes` and struct-tag mapping; both are revisited if a unit depends on Go's
exact byte layout. Benchmarks: `gconv.Int64` 2x and `gjson.New` 2.7x faster
than Go.

## ADR-028: gstr lands in waves; the core is owned-string based

Status: accepted (2026-11-24)

Context: `text/gstr` is the largest utility surface (~55 KB of Go over 21
files). Many functions have direct `std` equivalents; the conversion, case,
substring and parsing families are larger and used less by the base layers.

Decision: Port the core now: length/rune length, numeric check, repeat,
compare/equal, contains/contains_i/contains_any, pos/pos_i, array search and
prefix, slashes (add/strip/quote_meta), IsSubDomain, case helpers
(to_lower/to_upper/uc_first/lc_first/uc_words/letter checks), count family,
list2/list3, split/split_and_trim/join/join_any/explode/implode/chunk_split,
fields, trim family, bytes/chars. `split` returns owned `String` pieces like
Go's `[]string` API; `pos_i`/`contains_i` use a zero-allocation ASCII fast path.
The remaining families (`Convert`, `Case` conversions, `Sub`/`Parse`/
`Replace`/`Similar`/`Version`/`Domain` getters) follow as their units.

Consequences: The core matches Go behavior (including `IsSubDomain`'s wildcard
rules and `Split("")` per rune) and is 3x faster on rune length, case-insensitive
contains and UcWords. `split` + `join` is allocation-bound and 3x slower than Go
because Go's split shares the input; callers that only need pieces for joining
can keep using it, and a borrowing variant can be added if profiling asks.

Follow-up (2026-11-24): all remaining families were ported in the same wave —
position search with offsets, substrings, replacements, case conversions,
`Chr`/`Ord`/`OctStr`/`NumberFormat`/`Shuffle`/`HideStr`/`Nl2Br`/`WordWrap`,
`Parse`, Levenshtein/SimilarText/Soundex and version comparison. CaseSnake is
2.5x faster than Go (which uses regexp), SimilarText 2x and WordWrap 1.6x;
`Parse` is 2.4x slower because it builds owned nested `Value` keys.

## ADR-029: gdebug reports native frames

Status: accepted (2026-11-24)

Context: `debug/gdebug` reports the caller's function, file and line through
`runtime.Caller`, filters GOROOT frames, and exposes the goroutine id plus the
running binary's version.

Decision: `caller`, `caller_with_filter`, `stack` and the `caller_*` helpers
capture `std::backtrace::Backtrace`, parse function/file/line frames, and filter
`gf_debug` helper frames, toolchain paths and caller-supplied substrings.
Function names and paths are native symbols, so they differ in text from Go's
import paths (documented). `bin_version` uses the ported BKDR hash in base36;
`bin_version_md5` streams the executable through `gmd5`. `goroutine_id` returns
0 because threads are not goroutines.

Consequences: The API shape matches, and filtering matches behavior. Capturing
and symbolizing a backtrace costs ~0.33 ms per call (Go's `runtime.Caller` is
1.7 µs), so `gdebug` is debugging API and must stay out of hot paths, as Go's
own documentation warns for goroutine ids.

## ADR-030: Containers are generic maps and locked values

Status: accepted (2026-11-24)

Context: `container/gmap` ships nine concrete map types (string/string,
string/int, int/int, int/any, any/any, list variants, tree variants and the
generic KV map). `gvar.Var` wraps one value with conversion methods.

Decision: Port `gvar::Var` as a `RwLock<Value>` with the conversion surface
(`string`, `int64`, `uint64`, `float64`, `bool`, `bytes`, slices, `map`,
`marshal_json`, `unmarshal_json`) and `vars()`. Port `gmap` as one generic
`Map<K, V>` over an insertion-ordered map behind `RwLock`, with
`set/get/remove/contains_key/size/clear/keys/values/map/get_or_set/with_write/
with_read`; the Go type names become aliases (`StrAnyMap`, `StrStrMap`,
`StrIntMap`, `IntIntMap`, `IntStrMap`, `IntAnyMap`). All containers are always
synchronized (ADR-007), and `new_safe` accepts and ignores Go's flag.

Consequences: The common hash-map variants share one implementation and one test
set; `gmap.ListMap`, `gmap.TreeMap` and the KV/list variants need separate units
when their callers are ported. `any` keys cannot be hashed, so `AnyAnyMap`
becomes value-keyed only when `Value` gains hashing. Benchmarks: `gvar.Int()` is
1.6x and map set+get 1.3x faster than Go.

Follow-up (2026-11-24): `garray::Array<T>`, `gset::Set<T>`, `glist::List<T>`,
`gring::Ring<T>` and `gqueue::Queue<T>` were added with the same model —
one generic type behind a lock, Go names as aliases, index-based element access
in place of Go pointers, and condition variables for the blocking queue. The
sorted-array variants, `gtree`, `gtype` wrappers and `gpool` remain separate
units. Map set+get 1.3x, array append+At 1.8x, ring 2.4x and queue push/pop 14.6x
faster than Go; sets and lists are at parity.

## ADR-031: `gtype` uses atomics and locked `Option<Value>`

Status: accepted (2026-11-24)

Context: `container/gtype` ships 13 concurrent scalar wrappers built on Go's
`sync/atomic` and `atomic.Value`. `Interface`/`Any` stores any non-nil value
through `atomic.Value`, which panics on a nil store and has no Rust equivalent
for arbitrary types.

Decision: Numeric wrappers use `std::sync::atomic` with `Ordering::SeqCst`
(`set` returns the previous value, `add` the new value, `cas` a bool), floats
through `to_bits`/`from_bits` with a compare-and-swap loop. `String` and
`Bytes` use `RwLock`. `Interface` (with `pub type Any = Interface`) wraps
`RwLock<Option<Value>>`: `new(None)` is the empty object, `val()` returns
`Value::Null` when unset, and `set(Value::Null)` panics like Go's
`atomic.Value`. `Clone`/`DeepCopy` become `clone`/`deep_copy` returning the
type (there is no nil receiver). JSON goes through `gf_json` and `gconv`
(`Bytes` as BASE64, numbers as their decimal text).

Consequences: Observable behavior matches the Go tests (set/cas/add order,
JSON round trips, copy independence). Two documented differences: there is no
nil receiver, and `set(Null)` keeps Go's panic rather than becoming a `Result`.

## ADR-032: `gutil` starts with the comparator family

Status: accepted (2026-11-24)

Context: `util/gutil` is a large grab bag (copy, dump, list/map/slice/struct
helpers, reflection). The new ordered containers only need the comparators.

Decision: Port `gutil_comparator.go` in `gf-util::gutil`: `comparator_string`,
the signed/unsigned/float/byte/rune comparators over `&Value` through `gconv`,
`comparator_t<T: Ord>`, and `comparator_t_str<T: ToString>`
(`pub type Comparator = fn(&Value, &Value) -> i32`). NaN ordering matches Go's
`cmp.Compare` (NaN sorts first; NaN equals NaN). `ComparatorTime` waits for
`os/gtime` and is recorded as pending. The remaining `gutil` families are
separate units.

Consequences: The sorted arrays, trees and tree maps share one comparator
implementation. Integer comparators keep Go's narrowing (`comparator_uint32`
wraps a negative input), and float comparisons reproduce Go's NaN ordering.

## ADR-033: Ordered containers and their Go-visible shapes

Status: accepted (2026-11-24)

Context: Go's `garray` sorted variants, `gtree` (AVL, red-black, B-tree and
their KV forms) and `gmap`'s list/tree maps are separate types with custom
comparators. Rust has no `any` keys, and `gf_core::Value` implements neither
`Eq` nor `Hash` (it holds `f64`), so an `IndexMap<Value, _>` is impossible.

Decision:

- `garray::SortedArray<T>` stores a `Vec<T>` behind `RwLock`, a comparator
  `Arc<dyn Fn(&T, &T) -> i32 + Send + Sync>` (default: `comparator_t_str`) and
  an optional custom sorter. `SortedIntArray`/`SortedStrArray`/
  `SortedAnyArray` and `SortedTArray<T>` are aliases. Binary search, insertion
  order for equal values, range/sub-slice rules, unique, string and JSON
  output follow Go.
- `gtree::AVLKVTree<K, V>` and `RedBlackKVTree<K, V>` share one implementation
  over `std::collections::BTreeMap` keyed by a comparator-ordered wrapper, so
  iteration is comparator-sorted. `AVLTree`/`RedBlackTree` are the any/any
  aliases. `BKVTree` is a faithful port of the gods B-tree (node/entry layout,
  splits, removal, `Height`, indentation and `String`) because GoFrame's tests
  pin `Height() == 6` for 100 keys at order 3 and the exact `String` output.
- `gmap::ListKVMap<K, V>` keeps insertion order in a `Vec<(K, V)>`, not an
  `IndexMap`, because `Value` keys cannot be hashed; lookups are linear and
  positions are kept on update, like Go's linked list. `ListMap` is
  `ListKVMap<Value, Value>`.
- `gmap::KVMap<K, V>` is the existing hash `Map<K, V>` plus Go's extra surface
  (`search`, `pop`/`pops`, `set_if_not_exist*`, `get_or_set_func*`,
  `*WithError`, `filter_empty`/`filter_nil`, `flip`, `merge`, `is_sub_of`,
  `diff`, JSON). `TreeMap`/`TreeKVMap` are aliases of the red-black tree and
  `new_tree_map*` constructors.
- `*FuncLock` variants of `gmap` run the callback under the write lock to keep
  Go's guarantee that the callback runs once in a race; the callback must not
  call back into the same container. The tree `*FuncLock` names are aliases of
  the non-lock variants (the callback runs before the lock), a documented
  divergence. All other callbacks (iterators, `filter`, `walk`, `lock_func`)
  run on a snapshot and never under the lock (ADR-007).

Consequences: All container tests pass, including the B-tree structural
assertions. Documented divergences: `Value`-keyed trees expose `entries()`
rather than `map()` (no `Hash`), `search` returns `Option<V>` instead of
`(value, found)`, AVL/red-black `string()` is an in-order listing rather than
gods' node printout, and Go's unspecified map iteration order is replaced by
deterministic order in list maps.

## ADR-034: `intlog` is internal, context-free and preformatted

Status: accepted (2026-11-24)

Context: `internal/intlog` writes `time [INTE] file:line content` when debug is
enabled, adds OpenTelemetry trace ids from `context.Context`, and renders the
caller stack through `debug/gdebug`. Taking `tracing` or `gdebug` would create
cycles (`gf-debug` -> `gf-encoding` -> `gf-util` -> `gf-internal`), and the
port has no `context.Context`.

Decision: `gf_internal::intlog` keeps the output shape
(`%Y-%m-%d %H:%M:%S%.3f [INTE] file:line content`), reads the debug flag from
`utils::is_debug_enabled`, and parses `std::backtrace::Backtrace`'s `Display`
output for the caller and the stack (the `frames()` API is unstable). It
filters its own frames and writes `Caller Stack:\n` plus GoFrame's numbered
format on error variants. There is no context argument and no trace id;
`print`/`printf`/`error`/`errorf` receive text the caller already formatted.

Consequences: Modules above `gf-internal` (notably `os/gbuild`) can log without
new dependencies. Caller text is a native symbol and file path, which differs
from Go's import path (same as ADR-029). Stack capture costs a backtrace per
error call, so `intlog` stays a debugging path.

## ADR-035: `gtime` keeps Go's two states and bridges PHP formats itself

Status: accepted (2026-11-24)

Context: `os/gtime` is 3.9k lines: a `*Time` pointer with nil-receiver checks,
Go's zero `time.Time`, PHP-style `Format` (d/i/s/...), Go-layout `Layout`,
lenient `StrToTime` through three regexes, zones, StartOf/EndOf and SQL/JSON
bridges. Rust has no nil pointer and `chrono` has no PHP format language.

Decision: One struct `Time` holds `inner: Option<DateTime<FixedOffset>>`,
`layout` and the `Location` kind that produced the offset:

- `inner: None` is Go's nil `*Time`; `Some(zero instant)` is Go's zero
  `time.Time`; both report `is_zero`, only the first `is_nil`.
- `Location` is `Local`, `Utc`, a named IANA zone (`chrono-tz`) or a fixed
  zone with an optional name, so `zone()`, `location()`, `Format("T")` and
  `to_zone` keep Go's rendering.
- `format` translates the PHP specifiers to `chrono` format items on each call
  and post-processes the no-leading-zero specifiers (`j`, `G`, `u`); the Go
  sentinels (`=j=`, `=G=`, `=u=.`) are not needed. `layout` renders a Go
  layout; `format_to`/`layout_new`/`layout_to` convert between the two.
- `str_to_time` ports Go's three regex patterns and the layout fallback list;
  failures are `CodeInvalidParameter` errors instead of Go's nil return, and
  `new_from_str*`/`format_new` return `Option` where Go documents nil.
- Range boundaries follow Go exactly: week starts Monday, quarter/half are
  calendar based, `end_of_*` subtracts one nanosecond unless
  `nano_precision` is set.
- JSON marshals `Y-m-d H:i:s`; SQL `scan`/`value` accept the same shapes Go
  does.

Consequences: 94 translated tests (the six Go test files plus the Example
functions) pass. Documented divergences: nil is `is_nil()` rather than a nil
receiver, parse failures return errors, a `Local` zone has no portable
abbreviation so `T` renders the numeric offset, a parsed `MST` token does not
resolve an IANA zone, and `format` is ~36x slower than Go's direct layout walk
because it rebuilds and re-parses the format on every call (BENCH.md).

## ADR-036: The base layer stays self-contained

Status: accepted (2026-11-24)

Context: the new units sit low in the graph (`gf-util` -> `gf-container`,
`gf-internal` below everything, `gf-os` on top). Pulling `gdebug`, `ghash`,
`gstr`, `gipv4` or `gbase64` into them would invert or cycle those edges.

Decision:

- `util/guid` collects MAC addresses with `network-interface`, hashes with a
  local SDBM implementation, formats base 36 locally and keeps its sequence in
  an `AtomicU32`. Only `grand` is reused.
- `util/gpage` escapes HTML with a local five-character scanner, matching
  Go's `html.EscapeString` byte for byte.
- `internal/intlog` renders callers from `std::backtrace` (ADR-034).
- `os/gbuild` reads the injected payload from `option_env!("GF_BUILD_DATA")`
  at compile time, decodes it with `gf-encoding` and reports `rustc
  <rust-version>` where Go reports `runtime.Version()`.
- `os/genv` is the one place that mutates the process environment. Edition 2024
  marks `std::env::set_var`/`remove_var` unsafe, so both calls sit behind one
  process-wide `Mutex`, each in a small `unsafe` block with a `// SAFETY:`
  comment; keys are validated and failures return `Error` (Go panics nowhere
  here but returns `os.Setenv`'s error).
- `VERSION` moves to `gf-core` so `gf-os` can read it without depending on the
  facade; `gf::VERSION` re-exports it.
- `verify/<unit>` holds the `rust-anything` harness crates and is excluded from
  the workspace and the facade.

Consequences: No cycles, and the base layer compiles without the facade or
`gf-debug`. The two `unsafe` blocks in `genv` are the only unsafe code in the
workspace, they are narrow, documented and covered by the crate tests.

## ADR-037: Float text follows Go's two formats

Status: accepted (2026-11-24). Supersedes the float consequence noted in ADR-004.

Context: Go prints floats with `strconv.FormatFloat` in two shapes:
`gtype.Float32/Float64.String` and their JSON encoding use `'g', -1` (shortest
digits, exponential when the decimal exponent is below -4 or at least 6), while
`gconv.String` uses `'f', -1` (never exponential: `1e300` becomes 301 digits).
Rust's `Display` for floats is the `'f'` shape, so the port previously printed
`1e+21` as the 22 digit expansion and `Value`'s `%v` rendering disagreed with
Go for extreme exponents. `gconv.String` also appended `.0` to integral
floats, which Go does not.

Decision: `gf_core::float` exposes `format_g`, `format_g32` and `format_f`.

- `format_g*` derive Go's `'g'` text from Rust's shortest flat text: split the
  digits and the decimal exponent, then choose the exponential form when the
  exponent is below `-4` or at least `6`, writing the exponent with a sign and
  at least two digits. `format_g32` uses `f32`'s shortest digits, i.e. Go's
  32-bit precision.
- `format_f` is Rust's flat `Display` with Go's `NaN`/`+Inf`/`-Inf` spellings.
- `gtype`'s `string`/`marshal_json` use `format_g`/`format_g32`;
  `gconv::string` and `Value`'s `Display` use `format_g`/`format_f` as Go does.

Consequences: `Value::Float(1e21).to_string()` is `1e+21`,
`gtype::new_float64(Some(1e21)).string()` is `1e+21`, and
`gconv::string(&Value::Float(123.0))` is `123`, all matching Go 1.25. The
vectors in `gf-core/src/float.rs` were produced by Go's
`strconv.FormatFloat`. JSON encoding through `gf-json` still hands floats to
`serde_json`, whose exponent spelling (`1e300`) differs from Go's
(`1e+300`); that is a `gf-json` unit gap, not a `gtype` one.

## ADR-038: `gpool` uses one shared sweeper; `gmutex` hands out guards

Status: accepted (2026-11-24)

Context: `container/gpool` builds on `os/gtimer`, which is not ported (and would
pull a timer hierarchy into the base layer); `Pool.Close` in Go stops a
process-wide timer entry, so closing one pool silently stops expiry for every
other pool. `os/gmutex` embeds `sync.Mutex`/`sync.RWMutex`, which Rust replaces
with guards, and exists mainly for the `LockFunc`/`RLockFunc` helpers.

Decision:

- `gf-container::gpool` keeps one process-wide sweeper thread started on the
  first pool. Pools register a `Weak` reference and are visited every second;
  freed pools drop out of the registry at the next tick. `get` also expires the
  stale head lazily, so correctness never depends on the tick.
- `Ttl` (new name) models Go's signed `time.Duration`: `Never` (ttl 0),
  `Immediate` (ttl < 0) and `After(Duration)`, with `From<Duration>` for the
  common case.
- `close` releases the idle values immediately instead of on the next tick,
  and a closed pool never stops another pool's expiry.
- `gf-os::gmutex` wraps `std::sync::Mutex`/`RwLock`: `lock`/`rlock` return
  guards, `lock_func`/`rlock_func` keep Go's callback shape, and
  `try_lock_func`/`try_rlock_func` return `Option<R>` instead of `bool` so the
  callback's result survives. Mutexes do not poison, matching Go's `sync`.

Consequences: One extra thread for the whole process instead of one `gtimer`
entry per pool; `Ttl` has no Go counterpart (Go passes a signed
`time.Duration`), so it is documented in `GLOSSARY.md` as a port-only type.
`gpool::get` cannot return the zero value beside the error the way Go does;
callers use `Result`. 13 translated tests cover the two units, including a
sweeper test that waits for one real tick.

## ADR-039: `gvalid` is its own crate, rules over `Value`, no struct tags

Status: accepted (2026-11-24)

Context: `util/gvalid` is ~1.5k lines plus ~2.7k lines of 67 builtin rules. It
imports `os/gtime` and `net/gipv4`/`gipv6`, both of which sit above `gf-util`
in the crate graph, so the package cannot live in `gf-util` without a cycle.
It also validates structs through reflection (`gstructs`, `gmeta`, struct tags),
which Rust does not have, and translates messages through `i18n/gi18n`, which
is not ported.

Decision:

- New crate `gf-valid` with `gvalid` and the private `gvalid::builtin` module.
  The facade exposes it at Go's path through a `gf::util` module that glob
  re-exports `gf-util` and adds `gvalid`.
- Rules are one static table of `(name, default message, check)` with a linear
  lookup; `builtin::RunInput` carries the rule name/pattern, the value, the
  whole data map, the chosen message, the `ci` flag and an optional type id.
- The validation lanes are `Rules::Text` (one tag chain), `Rules::Map`
  (`field -> rule`) and `Rules::Sequence` (tags that keep the error order).
  `CheckStruct` has no counterpart: Rust has no reflection, so callers pass a
  rules map instead (the same treatment `internal/reflection` got in ADR-011).
  A derive macro can later build that map from attributes.
- `RuleFuncInput` and `builtin::RunInput` use `gvar::Var` like Go, but carry no
  `reflect.Type`. The `enums` rule takes `Option<&str>` as the Go type id and
  reports Go's "value type cannot be empty" error when it is absent.
- No i18n: custom messages and the builtin English messages are used; the
  `gf.gvalid.rule.*` translation lookup is not performed.
- `ValidationError` is one concrete type with Go's accessors (`maps`, `map`,
  `items`, `first_item`, `first_rule`, `first_error`, `strings`) returning
  borrows, because `gerror::Error` is not `Clone`. The error map is returned by
  value like Go's `Error` interface, hence the crate-level
  `clippy::result_large_err` expectation.
- `util/gutil` gains `map_possible_item_by_key` and
  `map_contains_possible_key`, which the glossary had listed but the crate did
  not implement; the builtin rules need them for cross-field comparisons.

Consequences: 67 builtin rules and the value/map/sequence lanes are gated by 25
new tests (24 in `gf-valid`, 1 in `gf-util`). Documented gaps: no struct-tag
lane; no i18n; `enums` needs an explicit type id; `gregex::match_string` drops
trailing empty capture groups where Go keeps them, so `parse_tag_value` reads
the optional message group by position. The struct lane is the one remaining
piece of `gvalid`, and it depends on a derive/trait design rather than
reflection.

## ADR-040: `gfile` over `std::fs`, without file-flag integers

Status: accepted (2026-11-24)

Context: `os/gfile` is a friendly layer over Go's `os` and `path/filepath`: 13
files, ~1.9k lines, including a globstar matcher the package added on top of
`filepath.Match`. Two of its files depend on units that are not ported:
`gfile_cache.go` needs `gcache` and `gfsnotify`, and `gfile_source.go`'s
`MainPkgPath` walks Go runtime frames looking for `package main` source files.
Go's `OpenFile(path, flag int, perm FileMode)` has no direct Rust counterpart
either: `std::fs::OpenOptions` is a builder, not an integer bitmask.

Decision:

- The package is one module `gf-os::gfile` plus four submodules mirroring the
  Go file split (`copy`, `scan`, `replace`, `search`).
- `open`, `create`, `open_append` and `open_with(path, &OpenOptions)` replace
  `Open`/`Create`/`OpenFile`/`OpenWithFlag*`. Permissions come from the process
  umask; [`gfile::chmod`] sets them explicitly.
- `MatchGlob` keeps Go's globstar semantics (valid `**` components, memoized
  matching), and `path_match` ports `path.Match` byte for byte, including
  character classes, negation and escaping. `glob` is implemented on top of
  them because `std` has no glob.
- `DefaultReadBuffer` becomes a `const`, `Separator` becomes
  `SEPARATOR`/`separator()`, and `Join` keeps GoFrame's concatenation semantics
  (it does not reset on an absolute piece, unlike `Path::join`).
- `MainPkgPath` and `Search`'s use of it are not ported (ADR-011 applies:
  there is no Go runtime frame); `Search` still walks the priority paths, the
  working directory and the binary directory, and reports the searched paths.
- `GetContentsWithCache`/`GetBytesWithCache` are deferred until `gcache` and
  `gfsnotify` land (waves 16-17).
- `Home` resolves `HOME`, `USERPROFILE`, then `HOMEDRIVE` + `HOMEPATH`, instead
  of the platform user database.
- Byte-offset helpers take `impl Read + Seek` where Go takes `io.ReaderAt`.
- Windows path handling follows `filepath`: both `/` and `\` are separators.

Consequences: 21 tests cover path helpers, file operations, offsets and line
reading, sizes, globbing and matching (including the globstar cases from Go's
own doc comment), sorting, mtimes, home and the copy, scan, replace and search
lanes. Documented gaps: no cache lane, no `MainPkgPath`, no `os.FileMode` type
(modes are `u32`), and `chmod` only sets the read-only bit off unix.

## ADR-041: `gspath` keeps the direct search lane only

Status: accepted (2026-11-24)

Context: `os/gspath` keeps an ordered list of search roots and two lanes: a
direct `stat` per root, and an indexed cache that `gfsnotify` keeps current by
watching every added directory. `gfsnotify` wraps the OS event watcher the Go
module `fsnotify` provides, which has no ported counterpart yet; its Rust
option (`notify`) is not vendored on this machine and the cache lane also needs
`gfile::GetContentsWithCache`.

Decision: `gf-os::gspath` ports the ordered roots and the direct lane, including
Go's security check that a hit must stay under the root it came from, the
`index_files` fallback, and `set`/`add` resolution order (absolute, then under
the existing roots, then under the working directory). Managers are shared per
root through a process-wide registry, like Go's `pathsMap`. `SPath::
new_with_cache` accepts Go's `cache` flag but documents that it behaves like
`new`, and `all_paths` stays empty.

Consequences: `gcfg`'s file adapter can search config files; the cache lane
(`SearchWithCache`, `AllPaths`, `Remove` cache effects, and the `gfsnotify`
updates) lands together with `gfsnotify`. Three tests cover the direct lane,
root replacement and the shared registry.

## ADR-042: `gfsnotify` runs on the `notify` crate

Status: accepted (2026-11-24)

Context: `os/gfsnotify` wraps `github.com/fsnotify/fsnotify` with a callback
registry, recursive directory watching, a one-millisecond burst filter and an
`Exit()` panic that removes the calling callback. Rust has no fsnotify in
`std`; the `notify` crate (8.2.2, fetched from the configured rsproxy sparse
mirror) is the ecosystem equivalent and compiles on this toolchain.

Decision: `gf-os::gfsnotify` keeps GoFrame's API and semantics on top of
`notify`:

- `Op` stays a bit mask (`CREATE`, `WRITE`, `REMOVE`, `RENAME`, `CHMOD`) with
  the same `is_*` helpers and the `CREATE|WRITE` display form.
- One shared default watcher, `Add`/`AddOnce`/`Remove`/`RemoveCallback`, and
  per-path callback lists whose lookup follows Go: the path itself, its direct
  parent, then recursive ancestors.
- Recursive watching uses the OS recursive mode instead of adding one watch per
  sub-directory; removing a callback unwatches its root when it was the last.
- Callbacks run on their own thread, like Go's `go w.doCallback`. `exit()`
  records a thread-local request that is honoured right after the callback
  returns, replacing Go's panic-and-recover sentinel.
- The burst filter drops identical `op path` events inside one millisecond.
- `nofity`'s event kinds map as: `Create(_)` -> `CREATE`,
  `Modify(Name(_))` -> `RENAME`, other `Modify(_)` -> `WRITE`,
  `Remove(_)` -> `REMOVE`; access events are ignored.

Consequences: `notify` and its tree join the dependency set (ADR-010 style
pick, recorded here); `gfile`'s and `gspath`'s cache lanes are now unblocked.
Five tests cover create/write/remove delivery, argument validation, `AddOnce`
deduplication, callback removal, `exit()` self-removal and the `Op` rendering.
Known difference: delivery is asynchronous on worker threads, so callers poll
or wait rather than assume the callback has run when the operation returns.

## ADR-043: `gfile`'s cache lane uses an inline TTL map

Status: accepted (2026-11-24)

Context: `gfile_cache.go` caches file contents in `os/gcache` and evicts an
entry from a `gfsnotify` callback. `gcache` is not ported (wave 16), and the
lane only needs get-or-set with a TTL, one entry per path.

Decision: `gfile::cache` keeps a process-wide `Mutex<HashMap<String,
CacheEntry>>` (content, insertion time, optional TTL) and uses `gfsnotify` for
eviction, exactly like Go: the first read installs one `add_once` watch per
cache key, and the callback drops the entry and calls [`gfsnotify::exit`].
`CacheDuration` (a port-only enum) names Go's signed duration cases:
`Default` (the `gf.gfile.cache` setting, one minute), `None` (negative: read
through), `Forever` (zero), `After(Duration)`. Expiry is checked lazily on read;
Go also drops the watch after the TTL with a timer, which the port leaves to
the lazy path. A malformed `gf.gfile.cache` value falls back to the default
where Go panics.

Consequences: `get_contents_with_cache`/`get_bytes_with_cache` no longer wait
for `gcache`, and the watch+eviction path is covered by four tests (eviction,
read-through, duration parsing, missing files). `gspath`'s indexed lane can now
reuse the same shape.

## ADR-044: `gres` extracts eagerly and carries bytes, not strings

Status: accepted (2026-11-24)

Context: `os/gres` packs a directory into a zip archive, gzips it, and either
embeds it base64/hex or keeps the raw gzip bytes. Go's `Resource` stores
`*zip.File` values from a live `zip.Reader`; `Add(string(data))` passes the
packed bytes through a Go string, which can hold arbitrary bytes. Rust strings
are UTF-8, and the `zip` crate's entries borrow the archive, so neither shape
works directly.

Decision: `gf-os::gres` keeps GoFrame's behavior with three adjustments:

- `unpack_content` takes `&[u8]` and detects hex, base64 or raw gzip, so
  binary archives are lossless; `Resource::add` and `gres::add` take `&[u8]`
  too. Text inputs can pass `text.as_bytes()`.
- Every zip entry is extracted into an owned [`File`] (content, size,
  directory flag, modification time) when the archive is unpacked. Zip marks
  directories with a trailing slash; the resource keys drop it, because
  GoFrame's `Get` trims trailing slashes and its own archives store
  slash-free directory keys.
- `PacketOption` is named `PackOption` (`Option` is the standard enum), and
  `PackToGoFile`/`PackToGoFileWithOption` are not ported: they emit a Go source
  file calling `gres.Add`, which has no Rust meaning.
- Packing writes the zip through the `zip` crate and gzips at level 9, adding
  the same directory entries (the prefix chain) as Go's `doZipPathWriter`.
  Packing with the `/` prefix produces absolute entry names, like Go.

Consequences: seven tests cover pack/add/get/index lookup, scanning (including
Go's `/i18n` vs `/i18n-dir` guard), export with and without prefix stripping,
hex/base64/raw unpacking, pack-to-file + load, JSON encoding and the reader.
`util/gstr` gained `trim_left_str`, which `Export` needs and the port was
missing. `gcfg`'s file adapter now has its resource lookup.

## ADR-045: `gcfg` is context-free and merges the watcher interface

Status: accepted (2026-11-24)

Context: `os/gcfg` threads `context.Context` through every adapter call, and
stores operation details (file name, path, type, key, value, content) as
context values read back by watchers. It also splits watch support into a
second `WatcherAdapter` interface found by type assertion. The port has no
context (ADR-034) and no runtime type assertion on trait objects.

Decision: `gf-os::gcfg` keeps GoFrame's behaviour with these adjustments:

- [`OperationContext`] replaces the context values: a typed struct with the
  file name, file path, file type, operation, key, value and content. Watchers
  are `Fn(&OperationContext)`.
- One `Adapter` trait carries `available`/`get`/`data` plus the watcher
  methods with default no-op bodies, so an adapter that cannot watch needs no
  extra code and `Loader` never type-asserts.
- `Config::get` returns `Ok(None)` where Go returns a nil `*gvar.Var`;
  `get_or` covers the `def` argument. `must_*` still panic like Go.
- `AdapterFile` keeps the file-name cache, the resource-manager and local
  search paths, `SetPath`/`AddPath` validation messages, custom content
  (`SetContent`/`RemoveContent`/`ClearContent`) and one `gfsnotify` watch per
  configuration file that invalidates the cache and notifies watchers. Go's
  re-arm retry loop (100 attempts, 1ms apart) is not ported; a single watch is
  installed and the cache invalidation still happens on the first event.
- `gfile::MainPkgPath` does not exist, so the automatic main-package search
  path is skipped.
- `Loader<T>` requires `T: Default + DeserializeOwned` and scans through
  `gconv`; `SetReuseTargetStruct` is accepted but has no effect because the
  port always swaps in a freshly scanned value.

Consequences: the `gjson` port gained the pieces `gcfg` needs —
`content_type`, `is_valid_content_type`, `load_content`, `load_content_type`,
`Json::map`, `Json::dump`, a recorded `set_violence_check` flag, and UTF-8 BOM
stripping. Nineteen tests cover the content adapter, `Config` wrapping and
priority fallbacks, the file adapter (reads, paths, file path errors, custom
content, `Set`, BOM, change-triggered reload, content notifications) and the
loader (whole document, property key, defaults, reload, converter, stop, error
handler). `glog` and `gcmd` can now build on `gcfg`.

## ADR-046: `glog` drops context, pools and the `*f` variants

Status: accepted (2026-11-24)

Context: `os/glog` is the largest `os/` package (about 3.9k lines). It threads
`context.Context` through every call (for trace ids and context keys), writes
files through `os/gfpool` under `os/gmlock`, schedules rotation checks with
`os/gtimer`, formats with Go's `fmt.Sprintf`, colors with `fatih/color`, and
exposes a `log/slog` bridge.

Decision: `gf-os::glog` keeps the pipeline and the observable text layout:

- Levels, flags, colors, `Config`, `HandlerInput`, the middleware `Handler`
  chain, the package default handler, `Logger` chaining (`with_path`,
  `with_cat`, `with_file`, `with_level`, `with_stdout`, `with_header`,
  `with_line`, `with_async`, `with_writer`, `with_stack*`) and the package API
  (`debug`/`info`/... plus `instance`) are ported, including the built-in
  `handler_json` and `handler_structure` handlers.
- No `context.Context` (ADR-034), so `TraceId`, `CtxStr` and `CtxKeys` are
  omitted; caller information comes from `gf-debug`.
- `LoggerClone` becomes `Clone`: the port copies the configuration, so chained
  setters never touch the source logger (Go shares the level-prefix map).
- Go's `fmt.Sprintf` variants (`Infof`, `Errorf`, ...) are not ported; Rust
  formats at the call site.
- Files are opened per write with a per-path lock registry instead of
  `gfpool` + `gmlock`; rotation checks run lazily on the first write and then
  when `rotate_check_interval` elapsed, instead of through `gtimer`.
- `io.Writer` becomes a `Writer` closure; color codes are emitted inline.
- The `log/slog` bridge (`glog_slog.go`) is not ported: Rust has no `slog`
  equivalent, and the `log` crate façade is a separate decision.

Consequences: twelve tests cover the header layout, level filtering, the
level-string mapping (including the `LEVEL_PROD` set), prefix/header toggles,
custom level prefixes, the JSON and structure handlers, file output with size
rotation and backups, `Cat` chaining, shared instances, stack capture and file
path formatting. `os/gcmd` is the last `os/*` package.

## ADR-047: `gcmd` drops context and the struct-object lane

Status: accepted (2026-11-24)

Context: `os/gcmd` has three layers — the package-level parsed arguments
(`Init`/`GetOpt` over `internal/command`), the [`Parser`] with supported
options and aliases, and the [`Command`] tree with sub-command dispatch and
help rendering. Its struct-object lane (`NewFromObject`, `AddObject`,
`gcmd_command_object.go`) reads method names and struct tags through
`os/gstructs`, `util/gmeta` and `util/gtag`. Go threads
`context.Context` through every callback and stores the parser/command in
context values.

Decision: `gf-os::gcmd` ports the three core layers with these adjustments:

- no `context.Context` (ADR-034): `CtxKey*` and the context-carried parser and
  command are gone; callbacks are `Fn(&Parser) -> Result<_, Error>` and
  `Fn(&Parser) -> Result<Value, Error>` (`Function`/`FuncWithValue`, both
  `Send + Sync`), and the command receives no context argument;
- `Command` sub-commands are `Arc<Command>` values with a stored parent path
  instead of a parent pointer, so a tree can be built from `&self`;
- `RunWithSpecificArgs` keeps Go's flow: parse, skip the binary name, search
  the command tree, `-h/--help` help printing, per-command re-parse with its
  own options and `Config`-node fallback through `gcfg`;
- the struct-object lane is not ported: it needs `os/gstructs` + `util/gmeta`
  and the struct-tag decision that ADR-039 left open;
- callback panics are not recovered into `CodeInternalPanic` (Go's `recover`);
  Rust callbacks should return `Err`;
- `PrintTo(io.Writer)` becomes [`Command::help_text`] returning the text.

Consequences: nine tests cover parser aliases/multi-options/defaults, the
package-level state, `BuildOptions`, environment fallback, sub-command
dispatch with per-command options, `CodeNotFound` for unknown commands,
`AddCommand` validation, help sections and word wrapping. All `os/*`
foundations are in place; the remaining `os/` packages (`gcache`, `gcron`,
`gctx`, `gfpool`, `gmlock`, `gmetric`, `gproc`, `grpool`, `gsession`,
`gstructs`, `gtimer`, `gview`) build on them.

## ADR-048: `gcache` names its TTL and types its keys

Status: accepted (2026-11-25)

Context: `os/gcache` is a concurrent in-memory cache plus a `Cache` wrapper
over an adapter. Go keys are `any` (hashable, so `1` and `"1"` differ), and
every call carries a `context.Context` and a signed `time.Duration` where `0`
means "never" and a negative value means "delete the key". The port has no
context, Rust `Duration` cannot be negative, and `Value` implements neither
`Hash` nor `Eq`.

Decision: `gf-os::gcache` keeps the cache semantics with these shapes:

- no `context.Context`;
- [`Ttl`] names Go's signed duration: `Never` (0), `Remove` (negative),
  `After(Duration)` (positive), with `From<Duration>` so `Duration::ZERO`
  means `Never`;
- [`Key`] is a canonical typed key built from any `Value` (`int:1` vs
  `string:1`) that also keeps the original value, so `Keys`/`Data` return the
  typed keys; public methods take `impl Into<Key>` (`1_i64`, `"name"`,
  `Value`, `Key`);
- `GetExpire` returns `Option<Duration>` (`None` = missing, `Some(ZERO)` =
  never) instead of Go's `-1` sentinel;
- `Data` returns `Vec<(Value, Value)>` because `Value` cannot be a map key;
- `*FuncLock` variants are aliases of the plain `*Func` ones: the port never
  runs user callbacks under the data lock (ADR-007); duplicate computation is
  possible under a race, which Go prevents with its write lock;
- the memory adapter keeps a per-adapter cleanup thread (one second, like
  Go's `gtimer` loop) and an LRU `VecDeque` whose eviction removes the entry
  from the data map; the Redis adapter (`gcache_adapter_redis.go`) is not
  ported because it needs a Redis client.

Consequences: fifteen tests cover set/get/contains, TTL expiry and cleanup,
`Update` keeping the expiry, `UpdateExpire`, keys/values, LRU eviction and
missing-key non-eviction, the `SetIfNotExist*`/`GetOrSet*` families,
`SetMap`, `Clear`, `Removes`, concurrent writers, the `Must*` wrappers and
typed-key separation. `gfile`'s cache lane can later be rebased on `gcache`.

## ADR-049: `gmlock` returns RAII guards

Status: accepted (2026-11-25)

Context: `os/gmlock` is a keyed lock manager where Go callers write
`gmlock.Lock(key)` / `gmlock.Unlock(key)` pairs. Rust already ties unlocking to
a guard's lifetime, and a manual `Unlock(key)` has no guard to release.

Decision: `gf-os::gmlock` keeps the keyed read/write semantics with owned
guards:

- `Locker::lock(key)` and `Locker::rlock(key)` return [`WriteGuard`] /
  [`ReadGuard`]; dropping the guard releases the key, so `Unlock`/`RUnlock` are
  not ported;
- `try_lock` / `try_rlock` return `Option<Guard>`;
- `lock_func`, `rlock_func`, `try_lock_func`, `try_rlock_func`, `remove`,
  `clear` and the package-level functions are ported;
- the read/write state is a per-key `Mutex` + `Condvar` with writer
  preference, implemented locally so guards are `'static` and need no external
  crate;
- there is no cache expiry for keys, like Go: keys stay until `remove`/`clear`.

Consequences: eight tests cover write exclusivity, independent keys, shared
readers, the writer-preference hand-off, the four `*_func` variants, blocking
acquisition across threads, and the package default locker.

## ADR-050: `grpool` has no context and a thread supervisor

Status: accepted (2026-11-25)

Context: `os/grpool` is a reusable worker pool: jobs are queued, workers fork
up to a limit, the pool can be paused/resumed and the limit can be changed by a
callback. Go schedules its supervisor with `os/gtimer` and passes
`context.Context` to every job.

Decision: `gf-os::grpool` keeps the pool behaviour with these shapes:

- no `context.Context`: jobs are `Func = Arc<dyn Fn() + Send + Sync>`, and
  `LimitChangerFunc`/`RecoverFunc` take the atomic limit / an [`Error`];
- `Pool::add` queues a job and forks a worker when the limit allows; the
  worker pops jobs LIFO like Go, exits when the queue is empty, and a
  `CountGuard` performs the single worker-count decrement;
- one supervisor thread per pool wakes every 500ms and forks workers when the
  limit changed or the pool is idle with pending jobs (Go uses a randomised
  500–1500ms `gtimer` entry);
- `add_with_recover` catches the panic with `catch_unwind` and converts the
  payload into an error; plain `add` lets the panic kill the worker, like Go;
- `Pool` is `Clone` over an `Arc` state so callers can share it.

Consequences: eight tests cover job execution, the concurrency limit,
jobs/size accounting, `clear_jobs`, pause/resume, adding to a closed pool,
panic recovery, `set_cap` and the package default pool.

## ADR-051: `gfpool` keys on options, not flag bits

Status: accepted (2026-11-25)

Context: `os/gfpool` pools open file handles keyed by `path&flag&ttl&perm`
where `flag` is Go's `os.O_*` bit set. It also installs a `gfsnotify` watch so
that a rename/remove recreates the pool, and its `Close(close ...bool)` either
returns the handle to the pool or closes it for real. Rust's `OpenOptions`
cannot be hashed, and integer flag masks are not idiomatic.

Decision: `gf-os::gfpool` keeps the pooling behaviour with these shapes:

- [`FileOptions`] is a hashable set of booleans (`read`, `write`, `append`,
  `create`, `truncate`) with `read_only`, `append_create` and `write_truncate`
  presets, replacing the Go flag bits;
- the ttl reuses `gcache::Ttl` (`Never`/`Remove`/`After`);
- [`File::close`] returns the handle to its pool, [`File::close_now`] closes
  the operating-system handle (Go's `Close(true)`); dropping a handle without
  closing simply drops the operating-system handle;
- the `gfsnotify` watch is not ported: [`Pool::file`] compares the open
  handle's metadata with the path and reopens when they differ, which covers
  rotation without a watch;
- the process-wide pool registry is keyed by `path + options + ttl`.

Consequences: eight tests cover append writes, handle reuse, `close_now`,
immediate ttl, rotation (a replaced path is reopened), truncate, `get` on an
unknown pool and read-only handles.

## ADR-052: context- and reflection-dependent packages are not ported

Status: accepted (2026-11-25)

Context: `os/gctx` wraps `context.Context` with OpenTelemetry span ids;
`os/gstructs` and `util/gmeta` read struct tags and field metadata at runtime.
The port has no `context.Context` (ADR-034), no OpenTelemetry client, and Rust
has no runtime reflection or struct tags (ADR-039 left the derive-macro
decision open).

Decision: these packages are recorded as **not ported** instead of being
stubbed:

- `os/gctx` — `New`/`WithSpan`/`CtxId`/`GetInitCtx` need a context value and a
  trace id; the port passes explicit parameters or typed structs instead;
- `os/gstructs` + `util/gmeta` — struct field/tag introspection has no Rust
  counterpart; the `gcmd` struct-object lane and the `gvalid` struct-tag lane
  already depend on this decision (ADR-039, ADR-047).

Consequences: the `os/*` package count for the port drops these two entries;
the remaining packages (`gcron`, `gsession`, `gview`, `gmetric`, `gproc`) do
not depend on them. If a derive-based struct metadata crate is ever added, the
deferred lanes can be reconsidered.

## ADR-053: `gtimer` replaces the panic sentinel with a thread-local entry

Status: accepted (2026-11-25)

Context: `os/gtimer` keeps timing jobs in a priority queue ordered by the tick
they must run at. One loop thread advances the tick counter; due jobs run on
their own goroutines. `Exit()` is implemented by panicking with an internal
sentinel inside the job, and `JobFunc` receives a `context.Context`.

Decision: `gf-os::gtimer` keeps the tick/queue semantics with these shapes:

- no `context.Context`: jobs are `JobFunc = Arc<dyn Fn() + Send + Sync>`;
- [`Entry`] is a cheap handle over `Arc<EntryState>`; the timer's queue keeps
  the entry alive even when the caller drops the handle;
- the priority queue is a `BinaryHeap` with a reversed `Ord` (earliest tick
  first) and an atomic `next_priority` cache, like Go;
- [`exit`] marks the thread-local current entry closed instead of panicking;
- a panicking job is contained (`catch_unwind`), the entry returns to `Ready`
  and the panic is logged; Go lets the job goroutine die and re-panics;
- `TimerOptions::default()` reads `gf.gtimer.interval` (milliseconds, 100 in
  default) like Go's `getDefaultInterval`.

Consequences: ten tests cover `AddOnce`, repeating `Add`, `AddTimes`, entry
stop/start, singleton non-overlap, `DelayAdd*`, `Exit`, timer stop/start, the
quick option and the package default timer.

## ADR-054: `gcron` schedules on `gtimer` with `chrono` dates

Status: accepted (2026-11-25)

Context: `os/gcron` parses six-part crontab patterns (with `#` for
five-part), supports month/week names, `@every 1s` and predefined
`@yearly`-style patterns, and checks every second whether a job is due. Go
uses `time.Time` arithmetic and a `context.Context`; it also starts entries in
the stopped state so a job cannot run before the entry is registered.

Decision: `gf-os::gcron` keeps the scheduling semantics with these shapes:

- no `context.Context`: jobs are `JobFunc = Arc<dyn Fn() + Send + Sync>`;
- date arithmetic uses `chrono::{DateTime<Local>, Datelike, Timelike}`; the
  schedule check is a port of `checkMeetAndUpdateLastSeconds` (including the
  latency fix-up) and `Next` keeps Go's month/day/hour/minute/second loops and
  the five-year cutoff;
- every entry is a `gtimer` job with a one-second interval, added in the
  stopped state and started after registration, like Go;
- `StopGracefullyNonBlocking` returns a [`StopWaiter`] with `wait()` instead of
  a cancelled context; running jobs are counted with a mutex + condvar;
- a panicking job is contained and logged through the optional `glog::Logger`
  (falling back to the package logger), instead of Go's recover-and-re-panic;
- the entry's job name is the job pointer (`{:p}`), because Rust has no
  function-name reflection.

Consequences: thirteen tests cover `@every`/predefined patterns, slash ranges
and their zero-interval rejection, a subset of Go's `Next` vectors, add /
search / remove, duplicate names, invalid patterns, `AddOnce`, `AddTimes`,
entry stop/start, graceful stop and entry ordering.

## ADR-055: `gsession` drops context, Redis and file encryption

Status: accepted (2026-11-25)

Context: `os/gsession` is a session manager over a pluggable storage: the
memory storage keeps `*gmap.StrAnyMap` values in `os/gcache`, the file storage
writes `timestamp(8 bytes) + JSON` per session id with path-escape and symlink
checks, and the Redis storage needs a Redis client. Go passes a
`context.Context` through every call, optionally encrypts files with AES, and
batches TTL updates every ten seconds.

Decision: `gf-os::gsession` keeps the manager/session/storage structure:

- no `context.Context`; storage methods take only their data arguments;
- session maps are `IndexMap<String, Value>`; the memory storage stores
  `Value::Map` in `gcache`;
- the [`Storage`] trait provides default implementations returning
  [`error_disabled`] (`CodeNotSupported`), mirroring `StorageBase`; a storage
  overrides only what it supports and `Session` falls back to its in-memory map
  exactly like Go;
- the Redis storage is not ported (needs a Redis client), and the file
  storage's optional AES encryption is not ported (off by default in Go);
- the file storage writes atomically (temporary file + rename), validates ids
  against path traversal and symbolic links, stores the millisecond timestamp
  in the first eight bytes, updates the TTL by rewriting that timestamp, and
  cleans expired files from an hourly background thread;
- `RegenerateId` keeps the data, writes it under the new id and optionally
  deletes the old one.

Consequences: ten tests cover the memory round trip, remove/clear, id
management (`SetId`, `SetIdFunc` and their started-session errors),
`RegenerateId` data retention, memory TTL expiry, the file round trip, path
traversal rejection, timestamp-based expiry, unique ids and the storage base
fallbacks.

## ADR-056: `gmetric` keeps the OTel-shaped API without a backend

Status: accepted (2026-11-25)

Context: `os/gmetric` mirrors the OpenTelemetry metrics API with its own
interfaces: a global `Provider` supplies `MeterPerformer`s, a `Meter` creates
instruments, and every instrument falls back to a no-op performer until a
provider initializes it. Go threads `context.Context` through every
measurement and names its measure-option type `Option`.

Decision: `gf-os::gmetric` ports the whole API surface with these shapes:

- no `context.Context`: measurement methods take only the value/option;
- Go's `Option` (measure attributes) is named [`ObservationOption`]; `Option`
  is the standard enum;
- `Attribute` is a plain struct (`key`, `value`) with `new_attribute`;
  `Attributes` is `Vec<Attribute>`;
- `MetricInfo`/`InstrumentInfo` are structs with accessors, including Go's
  `Key()` formatting (`name[@version]:metric`);
- instruments (`Counter`, `UpDownCounter`, `Histogram`, observable variants)
  hold their info and a performer; without a global provider they use the
  no-op performers and still expose their info;
- `Meter`/`Provider`/`MeterPerformer`/`Observer`/`MetricObserver` are traits;
  `get_global_provider().meter(option)` returns a [`LocalMeter`];
- global attributes are stored in a process-wide list and matched with the
  same instrument/pattern/version rules as Go, and `get_registered_callbacks`
  drains the queue like Go.

Consequences: seven tests cover counter info/key, histogram buckets, invalid
and empty metric names, observable metrics plus callback queueing/draining,
the global-attribute matching matrix, common attributes and provider
enablement.

## ADR-057: `gproc` ports only the shell lane

Status: accepted (2026-11-25)

Context: `os/gproc` contains two very different halves. The shell lane runs
commands through the platform shell (`cmd.exe /c` on Windows, `bash -c`/`sh -c`
elsewhere) and exposes process identity helpers; the other half implements
goroutine "processes" with mailboxes (`gproc_comm_*`), signal handling
(`gproc_signal*`) and a process manager/supervisor built on Go's goroutine ids
and `os.Signal`.

Decision: `gf-os::gproc` ports the shell lane and process helpers only:

- [`shell`], [`shell_run`] and [`shell_exec`] run commands synchronously
  (stdin piped or inherited, stdout/stderr captured or inherited); a non-zero
  exit becomes an [`Error`] carrying the captured output;
- `pid`, `is_child`, `ppid`, `set_ppid`, `start_time`, `uptime` and the binary
  search helpers (`search_binary`, `search_binary_path`) are ported;
- `ppid_os` is implemented for Linux through `/proc/self/stat` and returns `0`
  elsewhere (`std` exposes no portable parent-pid call);
- no `context.Context` and no OpenTelemetry environment injection;
- the goroutine communication, signal and manager/supervisor layers are not
  ported: Rust threads have no process-wide registry with mailboxes, and
  `std` has no signal API (that would need an extra crate).

Consequences: seven tests cover command output, combined stderr plus failure
status, stdin piping, `shell_run` success/failure, process identity, the custom
parent-pid round trip and binary search.

## ADR-058: `gview` ships its own Go-template subset engine

Status: accepted (2026-11-25)

Context: `os/gview` is a wrapper over Go's `text/template` and
`html/template`: it resolves template files across search paths and the
resource manager, caches them by mtime, and exposes built-in functions.
Faithfully porting Go's template language means reimplementing its lexer,
parser (pipelines, variables, `define`/`template`/`block`, method calls) and
`html/template`'s context-aware escaper. No Rust crate consumes Go template
syntax.

Decision: `gf-os::gview` implements a documented subset directly over
[`Value`] data:

- text, `{{...}}` actions, `{{/* comments */}}`, `{{- ... -}}` trim markers and
  configurable delimiters;
- `.`, `.field.sub`, `$x`, `$x.field`, quoted strings, numbers, booleans, `nil`;
- pipelines with Go's "previous result appended as the last argument" rule;
- `if`/`else if`/`else`/`end`, `range` over lists/maps/integers with optional
  `$k, $v :=`, `with`;
- built-ins: `eq ne lt le gt ge text html htmlencode encode htmldecode decode
  url urlencode urldecode date substr strlimit concat replace compare hidestr
  highlight toupper tolower nl2br include dump map maps json xml ini yaml yamli
  toml plus minus times divide`;
- custom functions registered through `bind_func`/`bind_func_map` with the
  signature `Fn(&[Value]) -> Result<Value, Error>`;
- file search across paths, `template/`/`resource/template` sub-folders and the
  resource manager; `include` recursion depth is capped at 32; the file cache
  refreshes on mtime changes (hot reload);
- `auto_encode` escapes interpolated plain values and `include` output is
  emitted raw, matching the Go behaviour tested by `Test_XSS` and
  `Test_Encode_Parse`.

Not ported: `define`/`template`/`block` definitions, method calls on Go
structs (there are no Go structs), `printf`-style formatting, i18n translation
(no `gi18n` port), and `html/template` context-aware escaping (plain
`AutoEncode` escaping instead).

Consequences: thirteen tests cover interpolation with custom functions,
comparisons, codecs/pipelines, `nl2br`, date/substr/strlimit/replace/highlight,
`include` with missing files, path validation, hot reload, `AutoEncode`,
syntax errors, custom delimiters/config maps, `map`/`maps`, and
`range`/`with`.

## ADR-059: `gtrace` keeps the context-free tracing primitives

Status: accepted (2026-11-25)

Context: `net/gtrace` is a convenience layer over OpenTelemetry: trace/span
ids live in `context.Context`, spans come from a global `TracerProvider`, and
baggage uses OTel's baggage API. This port has no `context.Context` (ADR-034)
and deliberately no OTel dependency, so the span/provider/propagator half
cannot be ported faithfully.

Decision: `gf-net::gtrace` ports the pieces that carry no context or OTel
state:

- [`Carrier`] — the text-map carrier (`new`, `get`, `set`, `keys`,
  `must_marshal`, JSON string, `unmarshal_json`). Go's `UnmarshalJSON` parses
  into a temporary map and drops it; that bug is not reproduced;
- [`TraceContext`] — an explicit trace/span holder replacing the context
  values, with GoFrame's 32-hex validation in `with_trace_id`/`with_uuid` and
  `get_trace_id`/`get_span_id`. An all-zero id is accepted by the setter but
  the getter returns empty, like OTel's `IsValid`;
- [`Baggage`] — an explicit key-value holder (`set_value`, `set_map`,
  `get_map`, `get_var`);
- `safe_content`, `safe_content_for_http` (gzip handling plus rune-based
  truncation with `...`), `max_content_log_size`, `is_tracing_internal`,
  `common_labels`, `is_using_default_provider`.

Not ported: span creation/ending, the default `TracerProvider`, the composite
text-map propagator and the HTTP client/server instrumentation; every one of
them is defined in terms of `context.Context` and OTel interfaces.

Consequences: six tests cover the carrier round trip and JSON handling,
trace-id/UUID validation and normalisation, `SafeContent` truncation with and
without gzip, baggage access and the tracing configuration getters.

## ADR-060: `gudp` runs on `std::net::UdpSocket` with relative deadlines

Status: accepted (2026-11-25)

Context: `net/gudp` wraps `net.UDPConn` with retry options, read/write
deadlines, named servers and free-port helpers. Go deadlines are absolute
`time.Time` values; `std::net::UdpSocket` exposes only read/write timeouts.

Decision: `gf-net::gudp` keeps the Go API shape over `std::net::UdpSocket`:

- [`ClientConn`] (`new`, `send`, `recv`, `send_recv`, `remote_addr`,
  `local_addr`, `set_deadline`, `set_deadline_recv`, `set_deadline_send`) and
  [`ServerConn`] (`recv`, `send`, deadlines, `is_closed`) — deadlines are
  relative `Duration`s;
- [`Retry`] holds `count` + `interval` by value; a zero interval falls back to
  100 ms like Go;
- [`Server`] (`new`, `set_address`, `set_handler`, `run`, `close`,
  `listened_address`, `listened_port`, `local_addr`) invokes the handler on the
  calling thread like Go, but sets a 200 ms read timeout so `close` unblocks a
  looping handler promptly; `ServerConn::is_closed` exposes the flag to
  handlers;
- `":0"` (and an empty address) binds `0.0.0.0:0` so the returned
  `listened_address` is reachable over IPv4; `":port"` is normalised the same
  way. (`UdpSocket::bind(":0")` would otherwise pick a link-local IPv6
  wildcard, which an IPv4 client cannot reach — the tests caught this);
- package helpers `send`, `send_recv`, `get_free_port(s)` and the named server
  registry (`get_server`, `Server::new(..., name)`).

Consequences: six tests cover the echo round trip (client + package helpers),
whole-package buffering, local-address binding and invalid ports, free ports,
the named server registry and the missing-handler error.

## ADR-061: `gtcp` ports connections, package protocol and server (no TLS/pool)

Status: accepted (2026-11-25)

Context: `net/gtcp` is GoFrame's TCP layer: a `Conn` over `net.Conn` with
`Send`/`Recv` (once/all/fixed), `RecvLine`, `RecvTill`, timeout variants, a
simple length-prefixed package protocol, a connection pool, TLS helpers and a
server with a named registry.

Decision: `gf-net::gtcp` ports the connection, package and server lanes:

- [`Conn`] on `std::net::TcpStream`: `send`, `recv` (`>0` exact, `0` once,
  `<0` until the buffer wait), `recv_line`, `recv_till`, `recv_with_timeout`,
  `send_with_timeout`, `send_recv(_with_timeout)`, deadline setters (relative
  `Duration`s) and `set_buffer_wait_recv`;
- the package protocol: `PkgOption { header_size, max_data_size, retry }`
  (default header 2, max 4, big-endian length prefix), `send_pkg`,
  `recv_pkg`, timeout variants and `get_pkg_option` validation;
- [`Server`]: `new`, `set_address`, `address`, `set_handler`, `run`, `close`,
  `listened_address`, `listened_port`, `local_addr`, the named registry
  (`get_server`) and the package helpers (`new_conn`, `send`, `send_recv`,
  `send_pkg`, `send_recv_pkg` with timeouts, `get_free_port(s)`);
- `run` uses a non-blocking listener with a 5 ms poll (std listeners have no
  cancellation) and resets accepted streams to blocking mode — a non-blocking
  listener hands out non-blocking streams on Windows, which made the first
  echo test fail with `WSAEWOULDBLOCK`; that fix is part of the port.

Not ported: TLS (`NewConnTLS`, `NewConnKeyCrt`, `NewServerTLS`,
`NewServerKeyCrt`, `LoadKeyCrt`) — it needs a TLS stack — and
`ConnPool`/`Pool` — the pool needs a borrow/return policy pass. Recorded for a
later unit.

Consequences: ten tests cover the echo round trip, `RecvLine`/`RecvTill`,
timeouts, package round trips across header sizes 1/2/3/4 (including the
65536-byte rejection and 65500-byte acceptance), package timeout behaviour,
invalid package options, free ports, the server registry and the package
helpers.

## ADR-062: `gsvc`/`gsel` are context-free interfaces plus local implementations

Status: accepted (2026-11-25)

Context: `net/gsvc` defines the service-registry/discovery interfaces with a
`LocalService` implementation, and `net/gsel` builds load-balancing selectors
over those services. Both are `context.Context`-centric in Go: registry calls
take a context (wrapped in a five-second timeout), selectors take a context in
`Pick`/`Update`, and the done callback takes one as well.

Decision: `gf-net::gsvc` and `gf-net::gsel` keep the interface layout without
contexts:

- `gsvc`: `Endpoint`/`Endpoints`/`Metadata`, the `Service` trait and
  `LocalService` (`new_service_with_name`, `new_service_with_kv`,
  `autoFillDefaultAttributes` reading `GF_GSVC_*`), the
  `Registrar`/`Discovery`/`Watcher`/`Registry` traits, `SearchInput`, and the
  process-wide default registry (`set_registry`, `get_registry`, `register`,
  `deregister`, the latter two returning `CodeNotImplemented` when no registry
  is set, like Go);
- `NewEndpoint`/`NewEndpoints` return `Result` instead of panicking
  (`Endpoint::must_new` is the panicking variant);
- `gsel`: `Node`, `DoneInfo`, `DoneFunc`, the `Selector` trait
  (`pick`/`update`), the four selectors (`SelectorRoundRobin`,
  `SelectorRandom`, `SelectorWeight`, `SelectorLeastConnection`), the builders
  (names `BalancerRoundRobin`/`BalancerRandom`/`BalancerWeight`/
  `BalancerLeastConnection`) and the process-wide default builder;
- a selector without nodes returns `Ok(None)`; the weight selector expands
  nodes by their `weight` metadata and drops zero/missing weights;
- the least-connection selector hands back a done callback that decrements the
  in-flight counter for the picked node.

Consequences: five `gsvc` tests cover endpoint parsing/display, the local
service key/prefix/value, service-from-key-value parsing, metadata helpers and
the default registry round trip; six `gsel` tests cover round-robin cycling,
random membership, weight distribution and zero-weight dropping,
least-connection in-flight tracking, builder names/default and unweighted nodes
not being selectable.

## ADR-063: `ghttp/internal/response` defines its own writer trait

Status: accepted (2026-11-25)

Context: Go's `response` package wraps `http.ResponseWriter`, `http.Flusher`
and `http.Hijacker`. Rust has no standard HTTP server API, and the future
`ghttp` port will serve over raw TCP, so the Go interfaces cannot be mapped
directly.

Decision: `gf-net::ghttp::response` defines a minimal
[`ResponseWriter`] trait (`write_header`, `write`, `flush`) and ports the two
wrappers over it:

- [`Writer`] — forwards only the first `write_header`, counts written bytes and
  remembers the header/hijack flags (`mark_hijacked` replaces `Hijack`, since
  the connection type is server-specific);
- [`BufferWriter`] — buffers body writes (`write`, `write_string`, `buffer`,
  `buffer_string`, `buffer_length`, `set_buffer`, `clear_buffer`), records the
  status, and `flush` writes the status header once, emits the status text when
  the body is empty and nothing was written, then writes and flushes the
  buffered body; a hijacked writer flushes nothing;
- [`status_text`] maps the common HTTP status codes (unknown codes return
  `Unknown Status`).

The `ghttp` server will implement `ResponseWriter` over its connection type.

Consequences: six tests cover header/byte tracking with a single forwarded
header, buffered write/flush, the default status body, buffer
replacement/clearing, hijack suppression and the status-text table.

## ADR-064: the `gtcp` pool keeps typed connections per address

Status: accepted (2026-11-25)

Context: Go's `PoolConn` borrows a connection from a per-address
`gpool.Pool`; `gpool` stores untyped `any` values and expires them by a timer
inside the pool. The Rust `gpool` stores `Value`, so a `Conn` cannot be placed
in it directly.

Decision: `gf-net::gtcp::pool` implements the pool inside `gtcp`:

- [`PoolConn`] wraps a [`Conn`], its per-address pool and a status
  (`CONN_STATUS_UNKNOWN`/`ACTIVE`/`ERROR`);
- a process-wide map holds one `PoolState` per address; the state keeps a
  `VecDeque<(Conn, Instant)>` and prunes entries older than
  [`DEFAULT_POOL_EXPIRE`] (ten seconds, like Go) on `get`;
- `close` consumes the connection: active connections return to the pool
  (callers must not reuse them), others are closed;
- the retry path of `send`/`send_pkg` re-acquires a pooled connection when the
  status is `unknown` and retries once, like Go;
- all methods take `&mut self` because the retry path can replace the
  underlying connection;
- the port keeps the package-protocol and timeout methods on `PoolConn`
  (`send_recv_pkg_with_timeout`, `recv_with_timeout`, ...), mapped onto the
  [`Conn`] implementations.

Consequences: five tests cover connection reuse through the pool (the local
port is preserved after `close` + re-borrow), line/till reads, package round
trips and package timeouts, send/recv timeouts and the pool constants plus the
connection-failure error path. TLS remains unported (ADR-061).

## ADR-065: `goai` keeps the OpenAPI model, builds schemas from `Value`

Status: accepted (2026-11-25)

Context: `net/goai` builds an OpenAPI 3.0 document from Go structs and route
functions with runtime reflection (`gmeta`, `gstructs`, struct tags). Rust has
no runtime struct reflection, so Go's `Add`/`addSchema`/`addPath` cannot be
translated as-is.

Decision: `gf-net::goai` ports the document model and a dynamic-data schema
builder:

- the full OpenAPI model: `OpenApiV3`, `Info`/`Contact`/`License`,
  `Paths`/`Path`/`Operation`, `Parameters`/`Parameter`, `RequestBody`,
  `Responses`/`Response`, `Headers`/`Header`, `Content`/`MediaType`/`Encoding`,
  `Links`/`Link`, `Examples`/`Example`, `Callbacks`, `Schemas`/`Schema`,
  `Discriminator`, `Components`, security schemes/requirements, servers, tags
  and external docs, with `x-` extensions flattened into the parent object;
- references use one generic [`RefOr<T>`] (`reference` + boxed `value`) that
  serializes as `{"$ref": ...}` when a reference is set, replacing Go's one
  `XxxRef` type per model; boxing keeps recursive schemas finite;
- arbitrary JSON payloads (`example`, `default`, `enum`, link parameters, ...)
  use `serde_json::Value`; the model derives `Serialize` (camelCase names,
  `omitempty` equivalents via `skip_serializing_if`);
- `OpenApiV3::new()` defaults `openapi` to `3.0.0`; `add_schema`, `add_path`,
  `path_mut`, `to_json`, `to_pretty_json` and `Path::set_operation`/
  `operation` replace the builder entry points that do not need reflection;
- `schema_from_value` maps a [`Value`] to a schema (object/properties,
  array/items, integer/number/boolean/string/binary, nullable for null).

Not ported yet: the reflection-driven `Add`/`addSchema`/`addPath` (Go struct
tags and function signatures), the struct-tag metadata conversions and the
config-driven content-type negotiation.

Consequences: eight tests cover document defaults and JSON output,
`$ref`-or-value serialization, `Value`-based schemas (object/array/item
types), operation/parameter/request-body output, security schemes and servers,
flattened extensions, path accessors and pretty-print round trips.

## ADR-066: `ghttp` starts with its own HTTP/1.1 message model

Status: accepted (2026-11-25)

Context: GoFrame's `ghttp` wraps `net/http` for requests, responses, cookies
and headers. Rust has no standard HTTP server, and the port already serves TCP
through `net/gtcp`, so the HTTP layer must be built on strings/bytes.

Decision: `gf-net::ghttp::message` defines the message primitives the future
server/router will use:

- [`Header`] — insertion-ordered, case-insensitive multi-value headers with
  canonical key casing (`textproto.CanonicalMIMEHeaderKey`);
- [`Cookie`]/[`Cookies`] — `Cookie` and `Set-Cookie` parsing/formatting;
- [`Query`] — ordered multi-value query/form data with `+`/`%XX` decoding and
  form encoding (`url_decode`/`url_encode`);
- [`Request`] — request line, headers, body by `Content-Length`, parsed query
  and cookies, plus `form()` for `application/x-www-form-urlencoded`;
- [`Response`] — status/headers/body with `set_text`/`set_json`/`set_body`,
  `Content-Length` filling and HTTP/1.1 serialization via `write_to`.

Not ported yet: multipart forms, chunked transfer encoding, trailers,
keep-alive reuse policy, TLS and the server/router layers.

Consequences: seven tests cover case-insensitive ordered headers, cookie
parsing/formatting (including attributes), query parsing/encoding plus decode
errors, full request parsing with query/cookies/body/form, malformed request
rejection, response serialization (JSON and 404 text) and write-to-writer.

## ADR-067: `ghttp` router is a flat route table; the server serves one
## connection at a time

Status: accepted (2026-11-25)

Context: GoFrame's router is a per-domain segment tree that merges
registration-order middleware lists with hook handlers, and its server is
`net/http.Server` with keep-alive, graceful restart and TLS. The Rust port
already owns its HTTP messages (ADR-066) and its TCP layer (`net/gtcp`).

Decision: `gf-net::ghttp` adds a router and a server:

- [`Router`] is a flat route table with static, `:param` and `*wildcard`
  segments, an optional method prefix (`GET:/path`, `ALL:/path`, comma lists),
  path normalization and scoring that prefers more static segments; the last
  registered route wins ties;
- route-level middlewares are stored with the route; the server prepends its
  global middlewares;
- [`Context`] carries request/response/params and the handler chain;
  middleware calls [`Context::proceed`] (named to avoid clashing with
  `Iterator::next`), a middleware that does not proceed short-circuits;
- [`Group`] prefixes paths and appends its middlewares to nested groups;
- [`Server`] supports `bind_handler`, `bind_middleware`, `group`,
  `serve(Request) -> Response` (network-free, used by tests and by `run`),
  `run` (non-blocking accept loop with a 5 ms poll, one thread per connection,
  one request per connection) and `close`;
- unmatched routes return `404 page not found\n`; handler errors return 500
  with the error text.

Not ported yet: keep-alive reuse, TLS, graceful restart/shutdown, per-domain
routing, route hooks (before/after serve), static file serving, session/cookie
integration, admin and swagger endpoints.

Consequences: eleven tests cover static/param/wildcard matching, static
priority and method prefixes, middleware ordering and short-circuiting, group
prefixes with nested middleware, path normalization, 404/500 responses,
middleware header propagation and a full TCP round trip through `run`.

## ADR-068: `ghttp` static serving is path-mapped with explicit escape checks

Status: accepted (2026-11-25)

Context: GoFrame serves static files through `http.FileServer`/`os.Root`: it
searches static path mappings, then search paths, then `gres`; directories
serve an index file or a listing; paths are resolved through an `os.Root` so
symlinks escaping the root are rejected.

Decision: `gf-net::ghttp::file_server` (the module cannot be called `static`,
a Rust keyword) ports the disk lane:

- [`StaticConfig`] with `set_server_root`, `add_search_path`,
  `add_static_path(prefix, path)`, `index_files` (default `index.html`),
  `index_folder` and `enabled`; static path mappings are sorted longest-prefix
  first, like Go;
- search order: mappings, then server root/search paths; percent-decoded URIs
  are rejected when any segment is `..`, and every existing path component
  must canonicalize under the canonical root (symlink escapes included);
- directories serve the first existing index file, otherwise a listing when
  `index_folder` is on, otherwise 403;
- listings mirror GoFrame's shape (`Index of <escaped path>`, directory-first
  ordering, `..` row) and escape names with Go's `html.EscapeString` while
  hrefs use Go's `url.URL{Path}` escaping (verified against Go: `&` stays,
  space→`%20`, `#`→`%23`, `(`/`)`→`%28`/`%29`);
- content types come from a small extension map;
- [`super::server::Server`] exposes `set_server_root`, `add_search_path`,
  `add_static_path`, `set_index_files`, `set_index_folder`,
  `set_file_server_enabled`, and falls back to static serving when no route
  matches.

Not ported: `gres` static resources, `ServeContent` range requests and
Last-Modified handling.

Consequences: seven module tests cover file serving with content types,
directory 403/listing, listing escaping, traversal rejection (raw and
percent-encoded), static path prefix mapping, disabled/missing roots and the
escaping helpers; one server test verifies the route-first/static-fallback
order over `serve`.

## ADR-069: `gclient` speaks HTTP/1.1 over `gtcp`

Status: accepted (2026-11-25)

Context: `net/gclient` wraps `net/http` with a prefix, default headers,
cookies, timeouts, retries and middleware. Rust has no HTTP client in `std`,
and this port already owns HTTP messages (ADR-066) and a TCP connection
(`net/gtcp`).

Decision: `gf-net::gclient` builds requests and parses responses by hand:

- [`Client`] keeps `prefix`, default headers, cookies, a timeout and a retry
  count; `set_prefix`/`set_header`/`add_header`/`set_cookie`/`set_timeout`/
  `set_retries` return a modified clone, like GoFrame's fluent setters;
- `get`/`get_content`/`post`/`post_form`/`post_json`/`put`/`delete`/`head`/
  `patch` and `request` build `METHOD target HTTP/1.1` with `Host`,
  `Content-Length`, `Connection: close`, optional `Cookie` and the default
  headers, then parse the response with `parse_response`;
- `build_url` merges prefix/path/query parameters (query `c=3` from the path
  is kept and parameters are appended, encoded as forms);
- [`Content::from_response`] parses JSON bodies when the content type says so,
  otherwise returns the text;
- only `http://` URLs are accepted; `https` returns `CodeNotSupported`.

Not ported: TLS, redirect following, chunked responses, compression, proxies,
the cookie jar across responses, middleware and the rich response helpers.

Consequences: seven tests cover GET with query parameters, JSON content and
404 status handling, form/JSON POST bodies, default headers and cookies,
URL building and splitting, response parsing and errors, and response header
visibility.

## ADR-070: `gredis` is a minimal RESP2 client over `gtcp`

Status: accepted (2026-11-25)

Context: `database/gredis` wraps `go-redis`, which provides cluster/sentinel
discovery, RESP3, pipelining, transactions, pub/sub and a large option surface.
This port has no external Redis crate and already owns a TCP connection.

Decision: `gf-database::gredis` (new `gf-database` crate) implements RESP2
directly:

- [`Reply`] decodes simple strings, errors, integers, bulk strings (nil
  included) and nested/nil arrays; helpers convert to string/int/strings/pairs;
- [`encode_command`] builds a RESP array of bulk strings;
- [`Config`] carries address, password, database index and timeout;
- [`Client`] runs `AUTH` and `SELECT` during construction, then exposes
  `command` plus PING, string (GET/SET/SETEX/DEL/EXISTS/EXPIRE/TTL/INCR/DECR),
  hash (HGET/HSET/HGETALL/HDEL), list (LPUSH/RPUSH/LPOP/RPOP/LLEN/LRANGE), set
  (SADD/SMEMBERS/SREM/SCARD), sorted-set (ZADD/ZSCORE/ZRANGE/ZREM/ZCARD) and
  PUBLISH commands; error replies become `Err` with the server message;
- one connection guarded by a `Mutex`; `close` drops it.

Not ported: cluster/sentinel, RESP3, pipelining, transactions, subscriptions,
scripting, connection pooling and TLS.

Consequences: eight tests cover RESP encoding/decoding (including nested
arrays, nil bulks and malformed/incomplete input), reply helpers, the
AUTH/SELECT handshake, string commands, hash/list/set/sorted-set commands,
error replies, connection failures and nil bulk results. The tests use a fake
RESP server over `std::net`, so no live Redis is required.

## ADR-071: `gdb` starts with its value layer, no reflection conversions

Status: accepted (2026-11-25)

Context: `database/gdb` is a reflection-heavy ORM: `Record` is
`map[string]*gvar.Var`, conversions (`Struct`/`Structs`/`ScanList`) bind rows
to Go structs by tag, and the model/driver layers build SQL per dialect. Rust
has no runtime struct reflection, and the ORM is far too large for one unit.

Decision: `gf-database::gdb` ports the value layer first:

- [`Record`] — an ordered field map over [`gf_core::Value`] with
  `insert`/`get`/`string`/`int`/`uint`/`map`/`json`/`xml`/`is_empty`/`len`
  and `Deref` to the underlying map;
- [`Result`] — a record list with `is_empty`/`len`/`size`/`chunk`/`json`/
  `xml`/`list`/`array`/`map_key_value`/`map_key_str`/`map_key_int`/
  `map_key_uint`/`record_key_str`/`record_key_int`/`record_key_uint`;
  duplicate `map_key_value` keys produce a list value, like Go;
- [`Map`]/[`List`]/[`Array`] aliases matching the Go types;
- [`SqlResult`] with `rows_affected`/`last_insert_id`/`insert_id` and the
  `must_*` forms.

Not ported yet: `Struct`/`Structs`/`ScanList` (runtime reflection), the SQL
builder, dialects, drivers, `Model`, transactions and caching.

Consequences: six tests cover record conversions (map/JSON/XML/scalars),
result basics and JSON, chunking, array projection (explicit and default
field), all key projections including multi-value grouping, and `SqlResult`
accessors plus the missing-insert-id error.

## ADR-072: `gdb` SQL builder renders dialects without a driver

Status: accepted (2026-11-25)

Context: GoFrame's builder accumulates `WhereHolder`s holding `any` values
(strings, maps, structs), then the active driver formats them (placeholder
style, quoting, nil handling). Rust has no `any` and no database driver in
`std`.

Decision: `gf-database::gdb_builder` ports the clause model and renders SQL
for a small dialect enum:

- [`WhereOperator`] covers `=`, `!=`, `<`, `<=`, `>`, `>=`, `IN`, `NOT IN`,
  `LIKE`, `NOT LIKE`, `BETWEEN`, `NOT BETWEEN`, `IS NULL`, `IS NOT NULL` and
  raw fragments; [`WhereKind`] is the AND/OR connector;
- [`Builder`] holds table/fields/where/order/group/having/limit/offset and
  renders `select_sql`/`insert_sql`/`update_sql`/`delete_sql` with positional
  arguments;
- [`Dialect::MySql`] quotes with backticks and `?`, [`Dialect::Postgres`] with
  double quotes and `$n`; identifiers are quoted per dotted segment while
  expressions (containing `(`/space/`*`) pass through;
- nil comparisons become `IS NULL`/`IS NOT NULL`, `IN`/`NOT IN` with an empty
  list become `0=1`/`1=1`, and `omit_nil`/`omit_empty` can skip holders;
- `page(page, size)` mirrors Go's 1-based pagination.

Not ported yet: driver execution (no `sql` crate), `Model` chaining, where
parameters as maps/structs (callers pass [`Value`]s), sub-queries, joins,
upserts and dialect-specific pagination.

Consequences: eight tests cover equality/comparison and nil handling,
`IN`/`LIKE`/`BETWEEN` including empty lists, nil omission, OR/raw/prefix
conditions, order/group/having/limit/page/random, insert/update/delete
rendering and the PostgreSQL placeholder numbering.

## ADR-073: `ghttp` server keeps connections alive, tracks status and runs
## status handlers

Status: accepted (2026-11-25)

Context: GoFrame's server relies on `net/http.Server` for keep-alive,
`Shutdown` and status handlers (`BindStatusHandler` for 404/502 overrides).
The port's server previously served exactly one request per connection, had no
status, and returned a fixed 404.

Decision: `gf-net::ghttp::Server` gains the missing lifecycle pieces:

- `serve_stream` loops over HTTP/1.1 requests on the same connection until the
  request carries `Connection: close`, the client disconnects or the server
  stops; the read timeout is 30 s per request;
- `status()` returns `SERVER_STATUS_STOPPED`/`SERVER_STATUS_RUNNING`; `run`
  sets running after binding and stopped when the accept loop ends;
- `in_flight` counts requests being served; `shutdown(timeout)` marks the
  server closed and waits (bounded) for in-flight requests;
- `bind_status_handler(status, handler)`/`bind_status_handler_by_map` register
  handlers that run after the response status is known (route, static or 404),
  with the response carried in the context, so handlers can replace the body or
  status like `WriteStatusExit`; multiple handlers run in registration order.

Not ported yet: TLS, graceful restart/fd passing, idle-timeout tuning and
per-domain status handlers.

Consequences: three new tests cover two keep-alive requests plus a closing
third on one connection, 404/502 status handler overrides (including two
handlers appending bodies), and the status transitions plus `shutdown`.

## ADR-074: `ghttp` response cookies and session wiring use `gsession`

Status: accepted (2026-11-25)

Context: GoFrame's server exposes `Response.SetCookie`, parses request
cookies, and binds a session manager (`SetSessionStorage`) whose id travels in
a cookie. The port already had cookie parsing and the full `os/gsession`
implementation (ADR-055).

Decision: `gf-net` now depends on `gf-os` (an acyclic direction) and adds:

- [`Cookie`] builder methods (`path`, `domain`, `max_age`, `secure`,
  `http_only`, `same_site`) in the message layer;
- `Response::add_cookie`/`set_cookie`/`cookies` — `Set-Cookie` headers are
  appended and re-parsable;
- `Server::set_session_manager(Arc<gsession::Manager>)` and
  `set_session_cookie_name` (default `gfsessionid`);
- for matched routes, `serve` opens a session from the request cookie, attaches
  it to [`Context`] (`session_mut`), runs the chain, closes the session and
  writes a `Set-Cookie` (`Path=/`, `HttpOnly`) only when the id is new or
  changed.

Not ported yet: config-driven session options (cookie domain/secure/TTL),
lazily created sessions only for handlers that touch them, session id
regeneration on login, and per-domain session storage.

Consequences: one message test covers cookie attributes, `Set-Cookie`
appending and re-parsing; one server test covers a full login → cookie →
second-request read round trip through a memory session storage. The ghttp
suite is 37 tests.

## ADR-075: `frame/g` is a thin facade over the ported crates

Status: accepted (2026-11-25)

Context: Go's `frame/g` re-exports frequently used types and provides
singleton accessors (`g.Server()`, `g.Log()`, `g.View()`, `g.DB()`, ...). Many
of its aliases (`Ctx`, `Meta`, `map[any]any`) depend on context or runtime
reflection, and the accessors that need unported subsystems (DB/Redis/i18n)
cannot be provided yet.

Decision: `gf::frame::g` provides:

- type aliases `Var`, `Map` (`IndexMap<String, Value>`), `List`, `Slice`,
  `Array`, `SliceStr`, `SliceInt`, `ArrayStr`, `ArrayInt`, plus `new_var`;
- `client()` (new HTTP client, like `g.Client`);
- `server(name)`/`tcp_server(name)`/`udp_server(name)` over the named
  registries (a new `ghttp::server::get_server` registry was added, matching
  the gtcp/gudp one);
- `log(name)` → `glog::instance`, `cfg(name)` → `gcfg::instance`,
  `validator()` → `gvalid::Validator::new`, `view(name)` with a facade-level
  named view registry (default view when unnamed);
- `set_debug(enabled)` → `gf_internal::utils::set_debug_enabled`.

Not provided: `Ctx`/`Meta` aliases (no context/reflection), `map[any]any`
aliases, `DB`/`Model`/`ModelRaw` (no driver execution), `Redis`
(no config-driven client), `I18n`, `Resource`, `Go`/`Try`/`Dump` helpers.

Consequences: four tests cover the aliases and `new_var`, singleton identity
for HTTP/TCP/UDP servers, named views/loggers, and the client prefix plus the
validator flow.

## ADR-076: `gdb` gets a real execution layer over bundled SQLite

Status: accepted (2026-11-25)

Context: the builder (ADR-072) renders SQL but nothing executed it; Go's
`gdb` runs through `database/sql` drivers. Rust has no `database/sql`, and a
real driver choice is needed to make `gdb` usable end to end.

Decision: add `rusqlite` (with the `bundled` feature, so no system SQLite is
required) and a first driver in `gf-database::gdb_exec`:

- [`Dialect::Sqlite`] was added to the builder (double quotes, `?`
  placeholders, `RANDOM()`);
- [`Db`] opens a file or in-memory database and exposes `execute`, `query`,
  `insert` (returns affected + last insert id), `update`, `delete`, `select`,
  `find` (first row) and `scalar`;
- `Value` parameters map to SQLite (bool → 0/1, u64 overflow is an error,
  lists/maps → JSON text), and rows map back to `Value` by column type;
- `update`/`delete`/`select` consume a [`crate::gdb_builder::Builder`], so the
  where/order/group/limit clauses are exercised end to end.

Not ported: MySQL/PostgreSQL drivers, pooling, transactions, `Model` chaining
and struct scanning. The bundled SQLite increases build time, accepted for a
self-contained, testable database lane.

Consequences: eight tests cover insert/select with typed columns (int, real,
blob, null), builder where/order/limit/in/like, `find`, update/delete with
affected counts and scalar reads, JSON parameters, u64 overflow errors, invalid
SQL errors and the SQLite dialect properties.

## ADR-077: `gdb.Model` chains the builder onto the SQLite driver

Status: accepted (2026-11-25)

Context: Go's `Model` is the ORM entry point: it accumulates
`Fields`/`Where`/`Order`/`Limit`/`Group` state and executes through the driver,
with `All`/`One`/`Count`/`Value`/`Insert`/`Update`/`Delete` terminals. The
port has the builder (ADR-072) and the SQLite driver (ADR-076) but no
chainable layer.

Decision: `gf-database::gdb_model` adds `Model<'a>` borrowing a `Db`:

- builders: `new`, `fields`, `where_eq`, `or_where_eq`, `where_op`,
  `where_in`, `where_not_in`, `where_like`, `where_between`, `where_null`,
  `where_not_null`, `order_asc`/`order_desc`/`order_random`, `group`,
  `having`, `limit`, `page`;
- reads: `all`, `one`/`find`, `count` (ignores order/limit/offset),
  `value(field)`, `exists`;
- writes: `insert`/`update`/`delete` taking `&[(String, Value)]`;
- helpers `record_data(record)`/`map_data(map)` to build write payloads.

Not ported: joins, sub-queries, transactions, struct scanning, models built
from struct tags, caching and soft deletes.

Consequences: seven tests cover where/order chains, fields projection, first
row/`find`, count/value/exists, limit/page/null, insert/update/delete with
affected counts and insert ids, between/OR conditions and the data helpers.

## ADR-078: `i18n/gi18n` keeps the file lane, drops context and gres

Status: accepted (2026-11-25)

Context: Go's `gi18n` loads translations from a directory or `gres` resource,
keys them by language, translates `{#key}` placeholders and picks the language
from `context.Context` when set. The port has no `gres` lane for i18n and the
ADR-034 rule drops `context.Context`.

Decision: new `gf-i18n` crate with `gi18n`:

- `Options { path, language, delimiters }` (defaults `en` and `{#`/`}`) and
  `Manager` with lazy loading, `set_path` (error for missing directories,
  reloads), `set_language`, `set_delimiters`, `translate`,
  `translate_with_language`, `translate_format`, `get_content`,
  `get_content_with_language`, `content`;
- file discovery: recursive scan of the path, language = first path segment
  (nested `zh/x.json`) or the file stem (flat `en.json`); files are parsed by
  content with the `gf-encoding` decode chain, and nested maps are flattened
  into dotted keys (`resourceUsage.workflow`), matching the Go YAML case;
- placeholder replacement honours the configured delimiters, leaves unknown
  keys untouched and replaces an exact whole-content key first;
- `translate_format` supports `%s`/`%v`/`%d`/`%%` (the Go `fmt` surface is not
  generalised);
- named instances (`instance`/`set_instance`), `default_manager` and the
  package-level wrappers; `frame/g::i18n(name)` returns the named manager.

Not ported: `gres` resources, context language values (`*_with_language`
methods replace them), hot reload on file changes, `gres` packed content.

Consequences: seven tests cover basic translation (including unknown keys),
custom delimiters, format verbs, explicit language/content lookups, nested
language directories plus YAML flattening, `set_path` errors and reload, and
named instance identity.

## ADR-079: `gdb` transactions use explicit BEGIN/COMMIT/ROLLBACK

Status: accepted (2026-11-25)

Context: Go's `gdb` offers `Transaction` with nested savepoints. The SQLite
driver in this port holds a single `rusqlite::Connection`; its
`transaction()` API needs `&mut Connection`, while all `Db` methods take
`&self` to keep one handle usable from a chain.

Decision: `gdb_exec::Db` gets explicit transaction helpers on `&self`:

- `begin`/`commit`/`rollback` execute the SQL statements;
- `is_transaction()` reflects `Connection::is_autocommit`;
- `transaction(closure)` runs `BEGIN`, the closure, then `COMMIT` (or
  `ROLLBACK` and the original error);
- `gdb_model::Model::transaction(closure)` passes a model with the same
  builder state into the same closure contract, and `Model` is `Clone` so a
  transaction body can build writes from a cloned chain.

Not ported: nested transactions/savepoints (SQLite rejects a second `BEGIN`),
transaction options (`TxOptions`), read-only transactions and distributed
transactions.

Consequences: two execution tests cover the commit/rollback contract and the
manual helper sequence; one model test covers a committed insert and a rolled
back one.

## ADR-080: `ghttp` serves a supplied OpenAPI document with a CDN Swagger UI

Status: accepted (2026-11-25)

Context: GoFrame builds the OpenAPI document from handler/struct metadata and
serves an embedded Swagger UI at `/swagger` with the document at `/api.json`.
The port has the `goai` document model (ADR-065) but no metadata-driven
generation, and no embedded UI assets.

Decision: `gf-net::ghttp::openapi` serves a caller-supplied document:

- `OpenApiConfig { document: goai::OpenApiV3, openapi_path, swagger_path }`
  with defaults `/api.json` and `/swagger`; `new` fills `openapi: "3.0.0"`
  when empty and the paths can be overridden;
- `document_json` serializes through `gf_json`, `ui_html` renders a Swagger UI
  page that loads `swagger-ui-dist@5` from unpkg and points at the document
  path;
- `Server::set_openapi(config)` stores it; `serve` checks the two paths before
  routing (GET only) and the response still flows through status handlers.

Not ported: metadata/struct-driven document generation, embedded UI assets
(the CDN is a browser-time dependency), per-domain documents and the
`SetOpenApiPath` config surface.

Consequences: three module tests cover the document JSON, the UI page and
custom paths/GET-only matching; one server test serves both paths and keeps
the rest of the router untouched.

## ADR-081: `frame/gins` owns the named instances behind `frame/g`

Status: accepted (2026-11-25)

Context: Go's `gins` is the singleton container behind the `g` facade; `g` is
a thin set of wrappers over it. The port grew ad-hoc registries: `ghttp` had a
server registry, `frame/g` had its own view registry and called `gi18n`
directly.

Decision: `gf::frame::gins` centralises the container:

- `server(name)`/`set_server(name, server)` over the `ghttp` registry
  (`ghttp::server::set_server` was added);
- `log(name)` → `glog::instance`, `config(name)` → `gcfg::instance`,
  `i18n(name)`/`set_i18n(name, manager)` → the `gi18n` registry;
- `view(name)`/`set_view(name, view)` over a container-level registry, with the
  default view for an unnamed lookup;
- `FRAME_CORE_COMPONENT_NAME_*` constants mirroring the Go names.

`frame/g` now delegates `server`, `view` and `i18n` to `gins`, matching the Go
layering and removing its duplicate view registry.

Not provided: `database`, `redis` and `resource` accessors — the database
needs a configured driver instance (the SQLite driver opens per handle), Redis
needs a config-driven client and the `gres` lane is not wired into the
container.

Consequences: three tests cover server singleton identity and replacement, the
named view and i18n registries, and logger/config instances plus the component
name constants. The facade suite is 9 tests.

## ADR-082: `gclient` follows redirects with Go's method semantics

Status: accepted (2026-11-25)

Context: GoFrame's client relies on `net/http.Client` redirect handling
(limit 10, `Location` resolution, POST→GET on 301/302/303 unless 307/308).
The port returned the 3xx response untouched.

Decision: `gclient::Client` gains:

- `DEFAULT_MAX_REDIRECTS = 10`, a `max_redirects` field,
  `max_redirects()`/`set_max_redirects(n)` (zero disables following and
  returns the 3xx response);
- `request` loops over 301/302/303/307/308 with a `Location` header,
  resolving it through `resolve_url` (absolute, protocol-relative,
  root-relative, path-relative and query-only forms), and errors with
  `too many HTTP redirects` beyond the limit;
- method semantics: 303 always becomes a bodyless GET; 301/302 do so for POST;
  307/308 preserve method and body;
- retries apply per hop (each hop uses the existing retry loop).

Not ported: cross-host cookie/authorization stripping, redirect history
exposure, 300/305 handling and HTTPS (no TLS stack yet).

Consequences: five tests cover relative redirect following, 303 POST→GET
dropping the body, 307 preserving method and body, the redirect limit and the
zero-limit mode, plus the URL-resolution helper cases.

## ADR-083: `gclient` HTTPS uses rustls with Mozilla plus custom roots

Status: accepted (2026-11-25)

Context: Go's client gets TLS from `crypto/tls`; the port had only plain
`http://`. Implementing HTTPS requires a TLS stack, and rustls (ring backend)
is the minimal audited option that fits the no-`unsafe` rule.

Decision: add `rustls 0.23` (ring, tls12), `rustls-pemfile` and
`webpki-roots` to `gf-net` (plus `rcgen` as a dev-dependency for tests):

- `split_url` accepts `https://` and defaults to port 443;
- `send_with_retries` picks `try_request_tls` for `https://` URLs; it builds a
  `RootCertStore` from the Mozilla roots plus certificates added through
  `add_root_certificate_pem`/`add_root_certificate_der`, then performs a
  `ClientConnection` over `TcpStream` with the client timeout and parses the
  response bytes with the same `parse_response`;
- missing `close_notify` at EOF is tolerated because the body is framed by
  `Content-Length`;
- redirects work across the scheme boundary (the per-hop loop re-derives TLS).

Not ported: TLS server side for `gtcp`/`ghttp`, system certificate stores
beyond the Mozilla roots, client certificates, SNI/ALPN customisation, HTTP/2
and certificate-pinning APIs.

Consequences: two tests start a self-signed TLS server (`rcgen`) and cover a
successful HTTPS GET trusting the certificate via DER and PEM roots, and the
failure when the certificate is not trusted.

## ADR-084: `ghttp` serves HTTPS with a rustls server config

Status: accepted (2026-11-25)

Context: after the client TLS work (ADR-083) the server still spoke plain
HTTP. GoFrame's server enables TLS from `certificate`/`privateKey` config.
The port's server owns the accept loop, so TLS can be added there without
changing the handler model.

Decision: `ghttp::Server` gains:

- `set_tls_pem(certificate_pem, private_key_pem)` parses a PEM chain and key
  with `rustls-pemfile`, builds a `rustls::ServerConfig` (no client auth) and
  stores it; `is_tls()` reports the mode;
- `run` snapshots the config per accepted connection: with TLS it spawns
  `serve_stream_tls`, otherwise the plain `serve_stream`;
- `serve_stream_tls` wraps `rustls::StreamOwned` in a `BufReader` (reads parsed
  requests, writes the response through the buffered reader) and keeps the same
  keep-alive loop, in-flight counter and status handling.

Not ported: `gtcp` TLS, client certificates/mTLS, ALPN/HTTP/2, certificate hot
reload, per-domain certificates and automatic HTTP→HTTPS redirects.

Consequences: two tests generate a self-signed certificate with `rcgen`, serve
it with `set_tls_pem`, and cover a successful `gclient` HTTPS GET trusting the
certificate and the failure of a plain `http://` client against the TLS
server.

## ADR-085: `goai` gets manual operation/server/tag registration

Status: accepted (2026-11-25)

Context: GoFrame's `goai.Add` reflects over handler functions and structs to
build operations, schemas and paths. Rust has no runtime reflection, and the
port already carries the full OpenAPI model (ADR-065) plus `add_schema`/
`add_path`/`path_mut` helpers.

Decision: `goai::OpenApiV3` gains manual helpers that mirror what `Add`
populates without reflection:

- `add_operation(path, method, operation)` → `Path::set_operation`, erroring on
  an unsupported method;
- `find_operation(path, method)` for lookups;
- `add_server(server)` and `add_tag(tag)` appending to the optional arrays.

Not ported: reflection-driven `Add`, schema inference from handler
signatures, automatic parameter discovery and `Config` read/write content-type
filtering.

Consequences: two tests cover multi-method registration on one path with
lookup and JSON output (including the invalid-method error), and server/tag
appending.

## ADR-086: `gtcp` TLS is a separate `TlsConn`, not a `Conn` variant

Status: accepted (2026-11-25)

Context: Go's `gtcp.Conn` wraps `tls.Conn` when TLS is configured, so the same
type flows through handlers. The Rust port's [`crate::gtcp::Conn`] owns a
`TcpStream` and its `send`/`recv` take `&self`; a TLS stream needs `&mut self`
for rustls and cannot be a transparent field without changing that surface and
every caller (gredis, gclient, ghttp).

Decision: `gtcp::tls` adds a sibling connection type:

- [`TlsClientConfig`] with the Mozilla roots plus `add_root_certificate_pem`/
  `add_root_certificate_der`;
- [`TlsConn`] built with `new(address, server_name, config, timeout)`, with
  `send`/`recv` mirroring [`crate::gtcp::Conn`]'s length/retry/buffer-wait
  semantics, plus `set_deadline`, `close`, `local_addr`/`remote_addr`;
- rustls `ClientConnection` over a connected `TcpStream` with the client
  timeout applied to read/write.

Not ported: TLS for `gtcp.Server` (its handler receives a plain `Conn`), server
certificates, client certificates, ALPN and the pool integration.

Consequences: four tests start a TLS echo server (`rcgen`) and cover the
round trip, an untrusted-root handshake failure, PEM roots plus the empty-PEM
error, and the `recv(0)` short-read mode.

## ADR-087: `gtcp` TLS server accepts TLS connections with a `Fn(TlsConn)`

Status: accepted (2026-11-25)

Context: the plain `gtcp.Server` handler receives a [`crate::gtcp::Conn`],
which cannot be constructed from a TLS stream without changing `Conn`'s
`&self` contract. Go's server instead swaps the connection type transparently.

Decision: `TlsConn` now holds either the client or the server rustls stream
(`TlsInner::{Client, Server}`), and `gtcp::tls::TlsServer` provides a TLS
sibling of `gtcp.Server`:

- `new(address)`, `set_address`/`address`, `set_tls_pem(cert, key)`,
  `set_handler(Arc<dyn Fn(TlsConn) + Send + Sync>)`, `close`, `run`,
  `listened_port`/`listened_address`/`local_addr`;
- `run` binds a non-blocking listener and, per accepted stream, wraps it with
  `TlsConn::from_accepted` and invokes the handler on its own thread;
- `TlsConn::from_accepted` applies read/write timeouts and builds the
  `ServerConnection`.

Not ported: pool integration, client certificates/mTLS, ALPN, certificate hot
reload and the config-driven `SetTlsConfig` surface.

Consequences: five tests cover the client echo round trip, the `TlsServer`
lifecycle and handler round trip, untrusted-root failure, PEM roots plus the
empty-PEM error, and `recv(0)` short reads.

## ADR-088: `gview` translates rendered results through `gi18n`

Status: accepted (2026-11-25)

Context: GoFrame's `gview` binds an i18n manager (`SetI18n`) and translates
the whole rendered result at the end of parsing, using the `I18nLanguage`
template variable (or the context language) to pick the language. The port's
gview deferred i18n (ADR-058) and the `gi18n` crate did not exist then.

Decision: `gf-os` now depends on `gf-i18n` (acyclic: gi18n has no `os`
dependency) and `gview::View` gains:

- `set_i18n(Arc<gi18n::Manager>)` and `i18n()` on the view state;
- `apply_i18n(rendered, params)` after every render in `parse_with_options`
  (content, orphan and file lanes): an `I18nLanguage` param selects the
  language through `translate_with_language`, otherwise the manager language
  is used;
- without a bound manager the rendered text is unchanged.

Not ported: context-derived language (no `context.Context`, ADR-034),
per-template i18n variables and `gview`'s config-driven default manager.

Consequences: two tests bind a manager with `en`/`zh-CN` files and cover result
translation (including the unchanged case before binding) and the
`I18nLanguage` param override. The gview suite is 15 tests.

## ADR-089: `test/gtest` keeps the assertions, drops the `T`/`C` bookkeeping

Status: accepted (2026-11-25)

Context: Go's `gtest` wraps `*testing.T`: assertion failures panic, `C` recovers
the panic and marks the test failed, and package-level `Assert*` panic
directly. Rust's built-in test harness already treats a panic as a failure, so
the `testing.T` wrapper and the `C` recovery step have no counterpart.

Decision: new `gf-test` crate with `gtest` assertion functions that panic with
GoFrame-style `[ASSERT] EXPECT ...` messages:

- `assert_eq`/`assert_ne` (generic `PartialEq + Debug`), plus `assert_eq_str`
  for Go's string-coerced `AssertEQ`;
- `assert_gt`/`assert_ge`/`assert_lt`/`assert_le` via `partial_cmp` (incomparable
  values fail the assertion);
- `assert_in`/`assert_ni` over slices;
- `assert_nil`/`assert_not_nil` over `Option<&T>`;
- `error`/`fatal` for explicit failures.

Not ported: `T`/`C` (the Rust harness is the case runner), map deep
comparison, `AssertIN` over `Value` coercions and the `gdebug` stack filter.

Consequences: seven tests cover the passing assertions, equality/ordering/
membership/nil failure panics (`#[should_panic]`) and the explicit error path.
The facade re-exports the crate as `gf::test::gtest`.

## ADR-090: `ghttp` applies a subset of `SetConfigWithMap`

Status: accepted (2026-11-25)

Context: GoFrame's server is configured from a map (`SetConfigWithMap`) with
keys such as `address`, `serverRoot`, `indexFiles`, `indexFolder`,
`staticPaths`, `openapiPath` and `swaggerPath`. The port had individual setters
but no config-map entry point.

Decision: `Server::set_config_with_map(&Value)`:

- requires a `Value::Map` and extracts the subset above, matching keys
  case-insensitively; unknown keys are ignored like Go;
- `staticPaths` is a nested map of URI prefix → directory;
- `openapiPath`/`swaggerPath` are applied through the new
  `set_openapi_paths` (they update the stored document config when one is
  registered);
- `set_openapi_paths` is also public for direct use.

Not ported: the full GoFrame server-config key set (timeouts, TLS, session,
logger, dump-router-map, max-header-bytes), struct-based `SetConfig` and
per-domain server configs.

Consequences: two tests cover a config map setting the address and static
root (served file) plus the non-map error, and a config map overriding the
OpenAPI document/UI paths after registration.

## ADR-091: `gspath`'s indexed cache lane closes ADR-041

Status: accepted (2026-11-25)

Context: ADR-041 deferred `gspath`'s cache lane until `gfsnotify` existed;
`new_with_cache` ignored the flag and `all_paths` was always empty. `gfsnotify`
is now ported, so the deferral can be closed.

Decision: `SPath` implements both lanes:

- `new_with_cache(path, true)` enables a `HashMap` index (cache key → path and
  directory flag); `set`/`add` rescan the root recursively and register a
  `gfsnotify` watcher on it;
- `search` uses the cache when enabled: cache key lookup (no filesystem
  fallback, like Go), index-file lookup for directory hits (`/` handled) and
  cache misses returning an empty result;
- watcher events add created entries recursively and remove removed/renamed-away
  entries; the callbacks are kept in `SPath` so the watch stays alive;
- `remove(path)` deletes one entry; `all_paths()` returns the sorted cache keys.

Not ported: Go's string-encoded cache values (`path_D_`/`path_F_`), separate
monitor bookkeeping per root (the port re-registers the root watcher on
`set`/`add`) and the resource-manager lane.

Consequences: two tests cover a cached search (directory, file, index file,
root, `AllPaths` ordering, cache-miss empty result) and the watcher adding a
created file plus removal through `remove`.

## ADR-092: `gvalid`'s struct lane is a derive macro, not reflection

Status: accepted (2026-11-25)

Context: ADR-039 left `util/gvalid`'s struct-tag lane open because Rust has no
runtime reflection. Go reads `v:"required|length:1,10"` struct tags and
validates the struct value. A decision was recorded as either a derive macro
emitting `Rules::Map` or a hand-written `Validatable` trait.

Decision: both halves:

- `gf-valid` exposes `Validatable { validation_rules() -> Rules; validation_data() -> Value }`
  and `FieldValue`, with `FieldValue` implemented for `Value`, `String`, `str`,
  bool, the integer/unsigned/float primitives, `Option<T>` and `Vec<String>`;
  `Validator::check_struct(&value)` clones the validator, sets the derived data
  and rules, and runs the existing pipeline;
- a new proc-macro crate `gf-valid-derive` provides
  `#[derive(Validate)]`, reading `#[gf(rule = "...")]` (and optional
  `#[gf(name = "...")]`) per field, preserving declaration order; unannotated
  fields are still included in the data map without a rule.

Not ported: Go's `v` tag name (Rust needs a declared attribute), runtime
reading of arbitrary structs without the derive, nested-struct rule
expansion and tag-value `gconv` coercion beyond `FieldValue`.

Consequences: five integration tests cover a valid struct, the required rule
(field reported), numeric/format rules, an `Option` field rule and the rule
ordering/data map output.

## ADR-093: `gregex` keeps unmatched trailing capture groups like Go

Status: accepted (2026-11-25)

Context: Go's `FindStringSubmatch`/`FindAllStringSubmatch` always return one
entry per capture group, using `""` for groups that did not participate. The
port's `capture_strings` truncated the vector at the last participating group,
so `(a)(b)?` on `"ac"` yielded `["a", "a"]` instead of Go's `["a", "a", ""]`.
`gvalid::parse_tag_value` had a workaround for the difference.

Decision: `capture_strings` maps every capture to a string, using `""` for
unmatched groups (the previous truncation is removed). `match_string`,
`match_bytes`, `match_all_string` and `match_all_bytes` all go through it, so
the whole surface changes together.

Not ported: Go's `SubexpIndex` naming lane and regexp fallback semantics of
`gregex` (POSIX/backtracking) remain outside the port.

Consequences: the match test now asserts trailing empty groups for
`match_string` (including an empty middle group) and for `match_all_string`,
matching Go's `FindStringSubmatch` output.

## ADR-094: `garray` sorted pop methods get a Go-example parity pass

Status: accepted (2026-11-25)

Context: HANDOFF recorded that `container/garray`'s sorted variants had no
`PopLeft`/`PopRight` parity pass against the Go examples, although the methods
themselves existed.

Decision: no API change; the sorted `pop_left`/`pop_right`/`pop_lefts`/
`pop_rights`/`pop_rand`/`pop_rands` implementations already follow Go's
`SortedTArray` delegation. The parity pass is a test pass translating:

- `TestSortedStrArray_PopLeft`/`PopRight` (value, remaining length, removal
  from `contains`) and the empty-array cases (no value, empty bulk pops);
- `TestSortedStrArray_PopLefts`/`PopRights` including the "size larger than
  the array returns everything" rule;
- `ExampleSortedTArray_PopLefts`/`PopRights` value and remainder ordering.

Not ported: Go's `SortedIntArray`/`SortedArray`/`SortedStrArray` type wrappers
are unified in the generic `SortedArray<T>`; their pop semantics are covered
through `SortedStrArray` plus the existing generic tests.

Consequences: two tests (four cases) now pin the sorted pop surface to the Go
examples; the garray suite is 68 tests.

## ADR-095: `gf-json` writes floats with Go's `'g'` text

Status: accepted (2026-11-25)

Context: ADR-037 recorded that `gf-json` handed floats to serde_json, which
writes `1e300` where Go's `encoding/json` writes `1e+300` (Go uses
`strconv.FormatFloat(v, 'g', -1, 64)`: exponent sign plus two padded digits).
`gf_core::float::format_g` already reproduces Go's text for `gtype`.

Decision: `GoValue`'s float arm formats through `gf_core::float::format_g`,
then serializes the text as a `serde_json::value::RawValue` (JSON allows
`e+300`/`e-07`), falling back to `serialize_f64` when the text is not valid
JSON (NaN/Inf, which Go's encoder also rejects). Because this happens in the
`Serialize` implementation rather than the formatter, both `marshal` and
`marshal_indent` produce Go-style floats.

Not ported: `json.Number` / `UseNumber` for float strings, float32-specific
formatting (`format_g32`) in the JSON lane, and Go's exact error text for
non-finite floats.

Consequences: one test covers large/small exponent forms, plain decimals,
zero, negatives, floats nested in a list, and the indented path.

## ADR-096: `gdb` gets a blocking SQLite connection pool

Status: accepted (2026-11-25)

Context: GoFrame's `gdb` keeps an internal link pool (`MaxIdle`/`MaxOpen`/
`MaxLifetime`). The port's `Db` owns one `rusqlite::Connection`, so concurrent
use would otherwise need one handle per thread with no reuse.

Decision: `gf-database::gdb_pool` adds:

- `DbPool::open(path)` (four connections) and `open_with_size(path, size)`
  (zero size is an error; each slot opens the file);
- `get()` blocks on a `Condvar` until a connection is free and returns
  `PooledDb`, which dereferences to `Db` and returns the connection on `Drop`;
- `try_get()` for the non-blocking form; `size`/`idle`/`path` state accessors;
- connections to the same file share the SQLite database, while an in-memory
  pool gives each slot its own database (documented).

Not ported: max-idle/lifetime reaping, health checks, async acquisition,
per-connection configuration and MySQL/PostgreSQL backends.

Consequences: five tests cover hand-out/reclaim with two slots, blocking
acquisition while one slot is held, shared file state across slots, the
zero-size/missing-directory errors and the drop-return path.

## ADR-097: `gredis` adds pipelines and `MULTI`/`EXEC` transactions

Status: accepted (2026-11-25)

Context: the RESP2 core (ADR-070) sent one command per round trip. GoFrame's
`gredis` also exposes pipelines and transactions (`TxPipelined`, `Watch`).

Decision: `gredis::Client` gains:

- `pipeline(commands)` writes every encoded command in one send and decodes
  exactly `count` replies with a shared buffer (`read_replies`);
- `watch(keys)`/`unwatch()` issue the corresponding commands on the client
  connection;
- `transaction(commands)` runs `MULTI`, queues every command, then `EXEC`:
  queueing errors abort with `transaction queue failed: ...` after draining
  `EXEC` for state reset; a nil `EXEC` array (watched key changed) is an error;
  otherwise the `EXEC` array is returned;
- `watch_transaction(keys, commands)` performs `WATCH` and the transaction
  under one connection lock, so no other client command can interleave.

Not ported: `TxPipelined` batch wrappers, optimistic-lock retry helpers,
connection pooling, pub/sub and RESP3.

Consequences: five tests cover a two-command pipeline, a committed
transaction (with command order), a queueing error, a watched-key abort
(including the `WATCH`/`MULTI`/`SET`/`EXEC` order) and `watch`/`unwatch`.
The gredis suite is 13 tests.

## ADR-098: `gredis` pub/sub uses a dedicated buffered connection

Status: accepted (2026-11-25)

Context: a subscribed Redis connection stops replying in command/reply order
and instead streams `message`/`pmessage` arrays. The client's shared `Mutex`
connection is therefore unusable for subscriptions, and the push stream can
carry an ack plus a message in one TCP read.

Decision: `Client::subscribe(channels)` and `psubscribe(patterns)` open a
dedicated connection (through the same `AUTH`/`SELECT` helper as the client),
send the subscribe command, read the ack, and return a `Subscription`:

- `Subscription` owns its connection plus a decode buffer (`buffer` + `drain`)
  so ack-plus-message reads are not lost;
- `next_message()` skips subscribe/unsubscribe acks and returns a
  `SubscribeMessage { pattern, channel, payload }` for `message`/`pmessage`;
- `unsubscribe()` sends `UNSUBSCRIBE` and reads its ack; `close()` closes the
  dedicated connection.

Not ported: reconnect/backoff on subscription connections, channel health
checks, `PING` inside subscription mode and RESP3 push types.

Consequences: three tests cover `SUBSCRIBE` (ack skipped, message fields),
`PSUBSCRIBE` (pattern/channel/payload) and a subscribe error; the gredis suite
is 16 tests.

## ADR-099: `gredis` scripting runs `EVAL`/`EVALSHA` and `SCRIPT` subcommands

Status: accepted (2026-11-25)

Context: the RESP2 core covered data commands, pipelines, transactions and
pub/sub; Lua scripting (`Eval`, `ScriptLoad`, ...) was still missing from the
GoFrame client surface.

Decision: `gredis::Client` gains:

- `eval(script, keys, args)`/`eval_sha(sha, keys, args)` building
  `EVAL`/`EVALSHA` with the `numkeys` prefix and returned replies untouched
  (the reply shape is script-defined);
- `script_load(script)` (SHA1 string), `script_exists(shas)` (array of `1`/`0`
  mapped to bools) and `script_flush()`.

Not ported: script caching/retry (`EvalSha` fallback to `Eval`), `FCALL`
functions, Redis-side script timeout handling and RESP3 reply shapes.

Consequences: three tests cover `EVAL` command encoding (script, `numkeys`,
keys, args) with integer/bulk replies, `EVALSHA`, and `SCRIPT LOAD/EXISTS/FLUSH`
including command order. The gredis suite is 19 tests.

## ADR-100: `gdb` struct scanning is a derive, not reflection

Status: accepted (2026-11-25)

Context: GoFrame's `Scan`/`Structs` map rows to struct fields by `orm`/`json`
tags using reflection (already deferred in ADR-071). The port now has typed
fields in the value layer, so a derive can supply the mapping.

Decision:

- `gdb::FieldFromValue` converts an optional column `Value` into a typed field
  (`String`, bool, integer/unsigned/float primitives, `Option<T>`,
  `Vec<String>`, `Value`) with per-value `gconv` coercion and defaults;
- `gdb::FromRecord { from_record(&Record) }` is the row-to-struct contract;
- new proc-macro crate `gf-database-derive` provides `#[derive(FromRecord)]`,
  mapping fields by name, `#[gdb(column = "user_name")]` renames a column and
  `#[gdb(skip)]` fills the field with `Default::default()`;
- `Model::scan::<T>()`/`scan_one::<T>()` build on `all`/`one`;
- the crate declares `extern crate self as gf_database` so the expansion paths
  resolve inside its own tests.

Not ported: nested structs/slices of structs, relation/association scanning,
`orm` tag grammar (priority/omitempty), custom converters and scanning into
maps without a derive.

Consequences: four integration tests cover column/type mapping (including
`Option`/float/bool), `scan_one`, missing rows, `FieldFromValue` defaults and
the `#[gdb(skip)]` path.

## ADR-101: `gi18n`'s gres lane is a trait, implemented by `gres::Resource`

Status: accepted (2026-11-25)

Context: ADR-078 deferred `gi18n`'s packed-resource lane. `gf-os` already
depends on `gf-i18n` (gview i18n, ADR-088), so `gf-i18n` cannot depend on
`gf-os` without a cycle.

Decision: `gf-i18n` defines a small source abstraction:

- `Resource` trait (`contains(path)` and `files(path) -> Vec<ResourceFile>`)
  plus `ResourceFile { name, content }`, where names are relative to the i18n
  root;
- `Options.resource`/`Options::with_resource`, resource auto-discovery in
  `Manager::new` (resource first, then disk) and `set_path` accepting a
  resource path;
- `load_data` reads from the resource when it contains the path, otherwise
  from the filesystem;
- `gf-os` implements the trait for `gres::Resource` (scanning packed files and
  stripping the root prefix), with a manual `Debug` impl on `Resource`.

Not ported: gres monitoring/hot reload on the i18n lane and resource-prefixed
language names via `gres` directory nodes beyond relative stripping.

Consequences: one fake-resource test in `gf-i18n` covers loading/`set_path`,
and one `gres` test packs a real directory, checks the relative file names and
translates through the manager.

## ADR-102: `ghttp` CORS lives on `Context`, with `exit_all` for preflight

Status: accepted (2026-11-25)

Context: GoFrame's `CORSOptions`/`CORS` are methods on `Response` reading the
paired `Request`; the port's `Response` is standalone, while `Context` owns
both. `MiddlewareCORS` applies the defaults and then `Next`, stopping the
chain for `OPTIONS` via `ExitAll`.

Decision:

- `ghttp::cors` defines `CorOptions` (domain list, origin, credentials,
  expose-headers, max-age, methods, headers), `DEFAULT_ALLOW_HEADERS`,
  `DEFAULT_ALLOW_METHODS`, `DEFAULT_MAX_AGE`;
- `default_options(&Request)` derives the origin from `Origin` then `Referer`
  (`scheme://host`) and appends client `Access-Control-Request-Headers`;
- `allowed_origin` implements subdomain matching for `allow_domain`;
- `Context::cors_default()`/`Context::cors(&options)` set the response headers;
  an `OPTIONS` request stops the chain (`Context::exit_all`, added to the
  router) and keeps status 200;
- `middleware_cors` is the default middleware function for
  `Server::bind_middleware`.

Not ported: `Access-Control-Request-Method` echo checks, cookie/credential
validation beyond the configured values and per-route CORS config.

Consequences: four tests cover default origin/request headers, referer
extraction, custom domain filtering (allowed and rejected origins) and the
`OPTIONS` early-exit with headers applied.

## ADR-103: `ghttp` gzip compression runs after the handler chain

Status: accepted (2026-11-25)

Context: GoFrame's `MiddlewareGzip` wraps the response writer and compresses
the buffered body after `Next` returns. The port's handlers write into
`Context::response` and there is no writer wrapper, so the middleware cannot
compress at call time.

Decision: `ghttp::gzip` uses a deferred step:

- `middleware_gzip` only arms `Context::gzip` when `Accept-Encoding` contains
  `gzip`;
- `Context::proceed` calls `gzip::finish` once the handler chain ends; it
  skips responses that are already `Content-Encoding`-set, bodies below
  [`GZIP_MIN_LENGTH`] (1 KiB) and compression failures, otherwise it
  gzip-compresses the body, updates `Content-Length` through `set_body` and
  sets `Content-Encoding: gzip`.

Not ported: streaming/flushed responses (compression requires the buffered
body), `Vary: Accept-Encoding`, per-route disable lists and other encodings
(br/deflate).

Consequences: three tests cover compression of a large body with round-trip
decompression and length rewrite, the small-body/missing-header skips, and the
already-encoded skip.

## ADR-104: `ghttp` JSON-body and handler-response middleware are deferred

Status: accepted (2026-11-25)

Context: GoFrame's `MiddlewareHandlerResponse` runs its envelope logic after
`Next`, and `MiddlewareJsonBody` rejects invalid JSON before it. The port's
handler list is flat: a middleware function runs before the route handler and
has no "after" hook (the same constraint as gzip, ADR-103).

Decision: `ghttp::middleware`:

- `middleware_json_body` validates a non-empty request body with
  `gf_json::valid`; invalid bodies get a 400 with `need json body` and the
  chain stops (`Context::exit_all`);
- `middleware_handler_response` arms `Context::handler_response`;
  `Context::proceed` calls `middleware::finish` after the chain (before gzip),
  which fills an empty response with `DefaultHandlerResponse { code, message,
  data: null }` (200 → OK/0, 404 → Not Found/65, 403 → Not Authorized/61,
  other 4xx/5xx → Unknown Error/64), skips bodies and GoFrame's streaming
  content types, and preserves non-content headers.

Not ported: handler-returned result objects (`GetHandlerResponse`), error
objects (`SetError`) and `DefaultHandlerResponse`'s Go struct tags (the Rust
value is built explicitly).

Consequences: five tests cover invalid/valid JSON bodies, the empty-response
envelope (status/content type/JSON shape), untouched bodies and stream
content types, and the non-OK status mapping.

## ADR-105: `ghttp` tracing keeps W3C `traceparent`, no OTel SDK

Status: accepted (2026-11-25)

Context: GoFrame's `internalMiddlewareServerTracing` builds spans through the
OpenTelemetry SDK, propagating via `traceparent`/`tracestate` and emitting
request/response events. The port has no OTel dependency and `gtrace` is a
value-level subset (ids, carrier, baggage) without a provider.

Decision: `ghttp::tracing` implements the propagation core:

- `parse_traceparent` validates `version-traceid-spanid-flags` (2/32/16/2 hex
  digits, non-zero ids) and returns `(trace_id, parent_span_id, sampled)`;
- `ServerSpan { trace_id, span_id, parent_span_id, sampled }` is stored on
  `Context` (new `span` field plus `Context::span()`);
- `middleware_tracing` inherits the incoming trace/parent or creates a fresh
  32/16-hex id pair, is idempotent when the span is already set;
- `traceparent_header` formats a span for outbound propagation.

Not ported: span export/`TracerProvider`, `tracestate` and baggage propagation,
request/response span events and route-name span renaming.

Consequences: four tests cover traceparent parsing (valid and invalid forms),
parent inheritance, root span creation (with header formatting) and idempotent
application.

## ADR-106: `gview` supports `define`/`template`/`block`

Status: accepted (2026-11-25)

Context: ADR-058 left Go's template definitions (`{{define}}`,
`{{template}}`, `{{block}}`) unported; the engine rejected them. These are the
standard composition primitives used by every real template set.

Decision: `gview` gains them through source preprocessing plus one AST node:

- `split_definitions` scans the content before tokenization: `{{define
  "name"}}...{{end}}` bodies are removed into a definition map, and
  `{{block "name" pipeline}}body{{end}}` is stored as a definition and replaced
  by `{{template "name" pipeline}}`; `{{end}}` matching counts
  `if`/`range`/`with`/`block`/`define` openers;
- `Node::Template { name, pipeline }` is parsed from `template "name"
  [pipeline]`; at render time the definition body is parsed and rendered with
  a child context (data = pipeline value or the current dot, fresh variables,
  depth + 1 with a 100-level guard), so `$` inside the definition is its
  argument;
- a missing definition reports `no template "name" associated with this
  engine`; unclosed definitions report the missing `end`.

Not ported: `define` bodies loaded from separate files into one template set,
`{{template}}` dynamic names, method calls on typed structs and
`html/template` context-aware escaping.

Consequences: four tests cover define + pipeline, template without a pipeline
plus `block` default, nested `if/else` inside a definition body, and the
missing-template/unclosed-definition errors. The gview suite is 19 tests.

## ADR-107: `gview` adds Go template's `printf`/`len`/`index`/`slice` and logic builtins

Status: accepted (2026-11-25)

Context: the port's builtin set covered GoFrame helpers and comparisons but
not the standard Go `text/template` functions that real templates rely on
(`printf`, `len`, `index`, `slice`, `and`, `or`, `not`).

Decision: `call_builtin` gains:

- `printf` with a small Go verb subset (`%v %s %d %f %.Nf %q %t %%`);
- `len` over strings (runes), bytes, lists and maps;
- `index` over lists, maps and strings (out-of-range is an error);
- `slice` over lists and strings with optional end (out-of-range is an error);
- `and`/`or` returning the first decisive operand (Go semantics) and `not`.

Not ported: the full `fmt` verb/flag grammar, `printf` width/precision on
strings, `call`, `html/template` safe values and variadic `slice`.

Consequences: three tests cover `printf` verbs/precision, `len`/`index`/`slice`
(including an out-of-range error) and the logical builtins; the gview suite is
22 tests.

## ADR-108: `gcmd`'s object lane is a derive, not reflection

Status: accepted (2026-11-25)

Context: GoFrame's `NewFromObject` builds a command from a struct via
reflection: fields with `short`/`brief`/`d` tags become options and exported
methods become sub-commands (ADR-047 left this unported because the port has no
`gstructs`/`gmeta`).

Decision: the port covers the field lane with a derive:

- `gcmd::FieldFromParser` converts an optional `Var` into a typed field
  (`String`, `Value`, bool, integer/unsigned/float primitives, `Option<T>`,
  `Vec<String>`); an orphan option stored with an empty value counts as `true`;
- `gcmd::CommandObject` (`command_name`, `command_meta`, `from_parser`,
  `execute`) plus `CommandMeta` describe one command; `new_from_object::<T>()`
  builds the `Command` and installs a run callback that parses the struct and
  calls `execute`;
- the new proc-macro crate `gf-cmd-derive` provides
  `#[derive(CommandObject)]`: struct `#[cmd(name/usage/brief/description/
  examples)]` and field `#[cmd(short/brief/default/arg/orphan/rename/skip)]`,
  mapping each field to an `Argument` and reading it back through
  `Parser::get_opt` with its default.

Not ported: methods as automatic sub-commands (needs reflection), `gmeta`/
`gstructs` tag scanning, nested command struct fields, validation tags and
`AddObject`-style merging of multiple objects.

Consequences: two integration tests cover the derived metadata and the
parse/run path (explicit options, orphan flag, renamed option and defaults).

## ADR-109: `gdb` table fields come from `PRAGMA table_info`

Status: accepted (2026-11-25)

Context: GoFrame's `TableFields` reads the driver's column metadata and the
Model exposes it (`Fields` with no arguments). The port had no introspection,
so features that depend on knowing column types/nullability (time fields, soft
delete, field selection validation) could not be built.

Decision: the SQLite lane introspects through `PRAGMA table_info`:

- `gdb_exec::FieldInfo { name, type_name, nullable, primary, default }`;
- `Db::table_fields(table)` quotes the table name (embedded `"` doubled),
  returns fields in declaration order and an empty vector for unknown tables;
  `nullable` is `notnull == 0` and `primary == 0` because SQLite reports
  `INTEGER PRIMARY KEY` as nullable;
- `Model::table_fields()` delegates using the model's table (the existing
  `Model::fields(&str)` remains the select-list setter, mirroring Go's dual
  `Fields` API).

Not ported: MySQL/PostgreSQL metadata, index/foreign-key introspection,
`TableFields` caching and the `TableInfo` aggregate.

Consequences: two tests cover the pragma mapping (primary/nullable/default/
order/unknown table/quoted names) and the model delegation.

## ADR-110: `gdb` maintains time fields and soft-deletes via introspection

Status: accepted (2026-11-25)

Context: GoFrame's Model fills `created_at`/`updated_at` automatically and
turns `Delete` into a `deleted_at` update when the table has those fields
(configurable through time-field options). After ADR-109 the port can detect
the fields, so the behaviour can be mirrored.

Decision: `gdb_model` gains `TimeOption { created_at, updated_at, deleted_at,
maintain_disabled, soft_delete_disabled }` (default names, all enabled) and
`Model::{with_time_option, unscoped}`:

- `insert` appends `created_at`/`updated_at` with `gtime::datetime()` when the
  table has those fields and the data does not provide them;
- `update` appends `updated_at` the same way;
- `delete` becomes an update of `deleted_at` when the field exists and soft
  delete is enabled;
- `all`/`one`/`count`/`value`/`exists`/`scan` add `deleted_at IS NULL` to the
  builder; `unscoped()` disables that filter (and soft writes);
- `maintain_disabled` turns the automatic timestamps off entirely.

Not ported: per-model custom time formats/unix timestamps, `deleted_at`
values other than a datetime string, `Restore`, deleted-row uniqueness checks
and bypassing the filter for `Update`/`Delete` when the model is scoped.

Consequences: three tests cover insert/update timestamps (including an
explicit `created_at`), soft-delete filtering with `unscoped`, and disabling
maintenance/soft delete (hard delete, null timestamps).

## ADR-111: `gredis` decodes RESP3 replies and negotiates with `HELLO`

Status: accepted (2026-11-25)

Context: ADR-070's client was RESP2-only; ADR-098 deferred RESP3. RESP3 is
required for the modern `HELLO 3` handshake and for typed replies (maps, sets,
doubles, booleans, big numbers, pushes), which Redis 6+ servers return once
negotiated.

Decision:

- `Reply` gains `Map(Vec<(Reply, Reply)>)`, `Set(Option<Vec<Reply>>)`,
  `Double(f64)`, `Boolean(bool)`, `BigNumber(String)`, `Push(Vec<Reply>)` and
  `Null`; `string`/`bulk`/`int`/`strings`/`pairs` handle them (`pairs` returns
  map entries directly);
- the decoder handles `%` (map), `~` (set), `,` (double), `#` (boolean), `(`
  (big number), `>` (push), `_` (null), `!` (blob error → `Error`) and `=`
  (verbatim string → `Bulk`);
- `Client::hello(protocol)` sends `HELLO <n>` and `Client::resp3()` is the
  `HELLO 3` shorthand; protocol state is not tracked on the client, so callers
  keep sending RESP2-style commands supported by Redis 6+.

Not ported: automatic RESP3 negotiation at connect time, `HELLO` option
fields (`AUTH`/`SETNAME`), attribute replies (`|`) and client-side protocol
switching state.

Consequences: two tests cover every RESP3 reply type (including consumed byte
counts and conversions) and a fake-server `HELLO 3` handshake returning a map.

## ADR-112: `ghttp` groups expose verb helpers and multi-method binding

Status: accepted (2026-11-25)

Context: `Router::Group` carried a prefix and middlewares and could register
routes through `bind_handler("METHOD:/path", handler)`, but GoFrame's group
API is verb-oriented (`group.GET`, `POST`, `ALL`, `Bind`, `BindHandler`), and
callers had to hand-write the method prefix for every route.

Decision: `ghttp::router::Group` gains:

- `bind(method, pattern, handler)` (method upper-cased) and the verb helpers
  `get`/`post`/`put`/`delete`/`patch`/`options`/`head`;
- `all(pattern, handler)` for every method and `bind_methods(&[&str], ...)`
  for several methods (joined with `,`, matching the router's method-prefix
  grammar);
- `prefix()` reports the normalized group path, nested `group()` composes
  prefixes and inherits middlewares as before.

Not ported: GoFrame's handler-object binding (`Bind` over a controller
struct), `ALLMap`/`BindMap` route tables, domain-based groups and per-group
`Hook`s.

Consequences: one router test covers every verb helper, `ALL`, explicit
`bind`, multi-method binding, negative method matches and nested prefixes.

## ADR-113: `ghttp` static files send validators and honour conditional GET

Status: accepted (2026-11-25)

Context: ADR-068's static file server always sent a full 200 body and
explicitly deferred modification-time headers. Go's `FileServer` and
GoFrame's static handler send `Last-Modified` (and commonly `ETag`) and answer
conditional requests with 304, which browsers and proxies rely on.

Decision:

- `ServeResult::File` carries `modified: Option<u64>` (Unix seconds) from the
  file metadata; `apply` sets `Last-Modified` (IMF-fixdate) and a weak
  `ETag` (`W/"<type-len>-<body-len>-<mtime-hex>"`);
- `apply_conditional(result, request_headers)` returns `304` with an empty
  body and no `Content-Length` when `If-None-Match` matches the ETag (comma
  lists supported) or `If-Modified-Since` parses and is not older than the
  file mtime;
- `http_date`/`parse_http_date` implement IMF-fixdate conversion with the
  civil-date algorithms, and the server's static branch now calls
  `apply_conditional`.

Not ported: strong ETags/content hashing, `If-Match`/`If-Unmodified-Since`,
`If-Range`/range requests, `Cache-Control`/`Expires` policy, and `304`
handling for directory listings.

Consequences: two tests cover the date round-trip (including invalid input)
and 304 for both validators plus the older-date and mismatched-ETag 200 paths.

## ADR-114: `gredis` resolves Sentinel masters before connecting

Status: accepted (2026-11-25)

Context: GoFrame's `Config` supports Sentinel mode (`MasterName` plus
sentinel `Address` and `SentinelUser`/`SentinelPass`); the port's `Config`
only held one server address.

Decision:

- `Config` gains `master_name`, `sentinel_user`, `sentinel_pass` and the
  builders `with_master_name`/`with_sentinel_auth`; in Sentinel mode
  `address` holds comma-separated sentinels;
- `Client::new` calls `resolve_sentinel_master` when `master_name` is set:
  each sentinel (in order) is dialled, optionally `AUTH`ed with the sentinel
  credentials, then asked `SENTINEL get-master-addr-by-name <name>`; the first
  valid two-element reply becomes `host:port` and the normal
  `AUTH`/`SELECT` connection runs against the master;
- failures try the next sentinel and end with a `resolve Redis sentinel
  master "<name>" failed: <last error>` error.

Not ported: Redis Cluster slot routing, sentinel failover/subscription,
`SentinelPass`-only AUTH forms beyond `AUTH [user] pass`, master re-resolution
on connection loss and read-only replicas.

Consequences: two tests cover a fake sentinel returning the master address
(command assertion plus `PING` to the master) and the all-sentinels-fail error
path.

## ADR-115: `gi18n` offers opt-in auto reload and a manual `reload`

Status: accepted (2026-11-25)

Context: ADR-078/101 loaded the translation files once and cached them;
`SetPath` was the only way to force a refresh. Long-running processes that
edit translation files (or receive deployments) need a reload path without
recreating the manager.

Decision:

- `Options.auto_reload` (default off) with `Options::with_auto_reload`;
  when enabled, `ensure_loaded` compares a file stamp (path, length and
  nanosecond mtime of every file under the root, sorted) and reloads when it
  changed;
- `Manager::reload()` clears the cache so the next translation reloads;
- resource-backed managers have no file stamp and never auto reload.

Not ported: filesystem watcher callbacks (polling on access only), partial
per-file reloads, and gres-monitor-driven reloads.

Consequences: two tests cover auto reload after a file change and the manual
`reload` path with auto reload disabled; the gi18n suite is 10 tests.

## ADR-116: `gredis` gets a slot-routing cluster client

Status: accepted (2026-11-25)

Context: GoFrame's `Config.Cluster` switches `go-redis` to cluster mode; the
port had single-node and Sentinel clients only. Cluster support needs key-slot
computation, `CLUSTER SLOTS` discovery and MOVED/ASK redirection handling.

Decision: a separate `ClusterClient` (the single-node `Client` stays
unchanged):

- `Config.cluster` + `with_cluster(true)` mark the mode; `address` holds
  comma-separated seed nodes;
- `ClusterClient::new` discovers ranges with `CLUSTER SLOTS`, storing
  `SlotRange { start, end, address }` sorted by start;
- `command` computes the key slot (`CRC16-CCITT`, XMODEM style, with `{}`
  hashtag support), routes to the owning node with a cached connection per
  node, and on `MOVED`/`ASK` refreshes the slot table and retries up to twice;
- `key_slot` and `parse_slot_ranges` are public, documented helpers;
  `ping`, `slots` and `close` round out the surface.

Not ported: `ASKING` handshake before retrying an `ASK` (the client refreshes
and resends), replica/read-only routing, multi-key commands across slots
(cross-slot errors surface as Redis errors), cluster re-discovery timers and
`CLUSTER NODES` parsing.

Consequences: three tests cover the CRC16/hashtag slot values against Redis
references, single-range routing (`CLUSTER SLOTS` + `SET`/`GET` command
assertions) and a MOVED redirection that refreshes the table and retries on
the new node.

## ADR-117: `gdb` model results cache by SQL with TTL and write invalidation

Status: accepted (2026-11-25)

Context: GoFrame's `Model.Cache(duration)` caches query results; the port had
no cache, so repeated `All` calls always hit SQLite.

Decision:

- `Model::cache(ttl)` stores the TTL on the model;
- `all()` builds the SQL, keys the process-wide cache by
  `<db pointer>|<table>|<sql>|<args>`, returns a fresh entry when the TTL has
  not elapsed, otherwise queries and stores the result;
- `insert`/`update`/`delete` clear every cached entry whose key starts with
  `<db pointer>|<table>|`, so model writes invalidate the table while direct
  `Db` writes intentionally do not;
- entries are only created when a TTL is set, and no user callbacks run under
  the cache lock (ADR-007).

Not ported: per-query cache keys from `CacheKey`, adapter/warm-up APIs, cache
statistics, context-aware caches and caching of `count`/`value` (only `all`).

Consequences: two tests cover reuse of a cached result (a direct `Db` insert
stays invisible), invalidation after a model update, and TTL expiry.

## ADR-118: `ghttp` requests expose client IP, URL and AJAX helpers

Status: accepted (2026-11-25)

Context: GoFrame's `Request` carries `RemoteAddr` from the connection and
exposes `GetClientIp` (proxy headers), `GetRemoteIp`, `GetSchema`, `GetUrl`,
`IsAjaxRequest` and `GetReferer`; the port's `Request` had neither the peer
address nor these helpers.

Decision:

- `Request.remote_addr` is filled by both server loops (plain and TLS) from
  `peer_addr`; manual construction leaves it empty;
- `client_ip()` checks `X-Forwarded-For` first (first comma element, ignoring
  `unknown`), then `Proxy-Client-Ip`, `Wl-Proxy-Client-Ip`, `Http_client_ip`,
  `Http-X-Forwarded-For`, `X-Real-Ip`, finally `remote_ip()`;
- `remote_ip()` strips a trailing `:port` (IPv6 brackets removed);
- `schema()` honours `X-Forwarded-Proto: https`; `url()` renders
  `scheme://Host + path[?query]`; `is_ajax_request()` checks
  `X-Requested-With: XMLHttpRequest`; `referer()` returns `Referer`.

Not ported: TLS-derived scheme (the request has no TLS flag in the message
model), `X-Forwarded-Host`/port, RFC 7239 `Forwarded`, and the cached
`clientIp` field.

## ADR-119: `gview` supports `{{$x := pipeline}}` variable assignment

Status: accepted (2026-11-25)

Context: ADR-106 added definitions and builtins, but Go template variable
assignment (`{{$x := ...}}`/`{{$x = ...}}`) was unsupported, so templates that
store intermediate pipeline results (common with `printf`, `index` and
`range`) failed to parse.

Decision:

- `Node::Assign { name, expr }` is parsed from a `$name := value` or
  `$name = value` action (a bare `$name` reference is still an expression);
  a missing value is a parse error;
- rendering evaluates the expression and stores it in the render-context
  variable map, so later `$name`, `$name.field` and pipelines can use it;
- `:=` and `=` are treated the same (both write the variable), unlike Go's
  lexical scoping; declarations inside `if`/`with` are not restored when the
  block ends.

Not ported: block-scoped shadowing/restoration, assignment to `$` (the root),
and range-variable declaration forms (`{{$i, $v := ...}}`).

Consequences: one test covers declaration, a pipeline assignment feeding
`printf`, reassignment inside `if`, and the missing-value parse error. The
gview suite is 23 tests.

Consequences: two tests cover header precedence (including `unknown`),
URL/scheme/AJAX/referer helpers, and remote-IP parsing for IPv4, bracketed
IPv6 and bare addresses.

## ADR-120: `gredis` gains a connection pool with GoFrame's pool options

Status: accepted (2026-11-25)

Context: GoFrame's `Config` exposes `MinIdle`/`MaxIdle`/`MaxActive`/
`MaxConnLifetime`/`IdleTimeout`/`WaitTimeout` and go-redis maintains the
pool internally. The port had one long-lived connection per `Client` with no
pooling, so concurrent users could not share bounded connections.

Decision:

- `Config` gains `min_idle`, `max_idle`, `max_active`, `max_conn_lifetime`,
  `idle_timeout`, `wait_timeout` with builders `with_pool`,
  `with_pool_timeouts`, `with_wait_timeout`;
- `RedisPool::new` pre-opens `min_idle` connections; `get` borrows a
  `PooledClient` (derefs to `Client`), reusing a healthy idle connection
  (`PING` check) whose idle time and lifetime are within bounds;
- when none is idle it reserves a slot up to `max_active` and dials; when the
  pool is full with a non-zero `wait_timeout` it waits on a condvar, otherwise
  it returns `Redis connection pool is exhausted`;
- `PooledClient::drop` returns the client to the idle list (or closes it when
  the pool is full); `close` shuts every idle connection down;
- `idle_count`/`total` expose pool state for diagnostics and tests.

Not ported: background idle pruning (expiry happens on `get`), `WaitTimeout`
defaults from `TLS`/cluster modes, per-command read/write deadlines and the
`gredis.New` group registry.

Consequences: three tests cover idle reuse (same client pointer and `total`
stays 1), the `max_active` limit with a timed-out waiter followed by a
successful borrow, and idle-expiry replacement.

## ADR-121: `gdb` Model exposes SQL aggregates

Status: accepted (2026-11-25)

Context: GoFrame's Model has `Sum`/`Avg`/`Max`/`Min` (and `Count`), which the
port's chainable Model lacked; callers had to fall back to raw SQL.

Decision:

- `Model::aggregate(function, field)` builds `FUNCTION(quoted field) AS gf_agg`
  on a soft-delete-filtered clone, clears order/limit/offset and queries the
  first value;
- `sum`, `avg`, `max`, `min` delegate to it; an empty result set yields
  `Value::Null` (matching SQL `NULL`; callers use `gconv` for typed values);
- aggregates do not use the model result cache (ADR-117 caches only `all`).

Not ported: `COUNT(DISTINCT ...)`, group-by aggregates returning rows,
`SUM` over multiple fields and GoFrame's aggregate helper aliases.

Consequences: one test covers sum/avg/max/min over three rows, a filtered sum
and the empty-set `Null` result.

## ADR-122: `gdb` writes structs through `#[derive(ToRecord)]`

Status: accepted (2026-11-25)

Context: ADR-100 added derive-based reading (`FromRecord`). Writing still
required callers to build `(column, value)` pairs by hand, unlike GoFrame's
`Insert`/`Update` with a struct argument.

Decision:

- `gdb::ToColumnValue` converts a Rust value into a column `Value` (`Value`,
  `String`, `&str`, bool, integer/unsigned/float primitives, `Option<T>` →
  `Null`, `Vec<String>` → list);
- `gdb::ToRecord { fn to_record(&self) -> Vec<(String, Value)> }` is the
  write contract;
- `gf-database-derive` gains `#[derive(ToRecord)]` with the same
  `#[gdb(column = "...")]`/`#[gdb(skip)]` attributes as `FromRecord`;
- `Model::insert_record(&T)`/`update_record(&T)` delegate to
  `insert`/`update` with `to_record()` (so time fields, soft delete and cache
  invalidation still apply).

Not ported: nested structs/slices of structs, custom converters, `orm` tag
priority/omit-empty, partial-update field selection and batch struct writes.

Consequences: the derive integration test now covers the column order/values
(with rename and skip), an insert through `Model::insert_record` and a
`update_record` round-trip.

## ADR-123: `gdb` Model iterates rows with `chunk`

Status: accepted (2026-11-25)

Context: GoFrame's `Model.Chunk(size, handler)` pages through a large result
set so callers can process rows without holding everything in memory; the
port only exposed `all`/`one`.

Decision:

- `Model::chunk(size, handler)` copies the soft-delete-filtered builder, sets
  `limit = size`/`offset = page * size`, queries the page and calls
  `handler(&page)`;
- empty pages end the loop; a page shorter than `size` also ends it; a
  handler error aborts and is returned unchanged;
- non-positive sizes return `chunk size should be positive`.

Not ported: a streaming receiver (Go's channel form), total-count metadata,
concurrent page fetching and interaction with the model result cache.

Consequences: one test covers page sizes `[2, 1]` over three rows, handler
abort after the first page and the invalid-size error.

## ADR-124: `ghttp` redirects live on `Context`

Status: accepted (2026-11-25)

Context: GoFrame's `Response.RedirectTo(location, code...)` sets `Location`
and the status (default 302) and exits the current handler; `RedirectBack`
uses the request referer. The port's `Response` has no request handle, so the
exit step and referer lookup belong on `Context`.

Decision:

- `Context::redirect_to(location, code: Option<i32>)` sets the `Location`
  header, the status (302 by default) and calls `exit_all`;
- `Context::redirect_back(code)` reads `Request::referer()` and delegates
  (an empty referer produces an empty `Location`, matching Go).

Not ported: `RedirectBack`'s fallback when no referer exists, safe-URL
validation, `Response.RedirectTo`'s Go methods on `Response` itself and
automatic HTML body generation.

Consequences: one router test covers the default 302, a custom 301 and the
referer-based redirect.

## ADR-125: `ghttp` enforces `ClientMaxBodySize`

Status: accepted (2026-11-25)

Context: GoFrame's server config has `ClientMaxBodySize` (bytes); oversized
requests are rejected with 413 before handling. The port had no body limit, so
a single large upload could exhaust memory while parsing.

Decision:

- `Server::set_client_max_body_size(bytes)` stores the limit
  (unset means unlimited);
- `Server::serve` checks `request.body.len()` before routing/OpenAPI/static
  handling and returns `413` with `Request Entity Too Large` when exceeded;
- the check runs on the already-parsed body (request parsing still reads up to
  `Content-Length`), which is the port's single in-process request path.

Not ported: rejecting before reading the body (streaming limit), a default
8 MiB value, per-route limits and GoFrame's full `Config` map wiring.

Consequences: one server test covers the 20-byte request rejected with 413 and
a 5-byte request reaching the handler.

## ADR-126: server config map wires `clientMaxBodySize`; pool counts creations

Status: accepted (2026-11-25)

Context: ADR-090's `set_config_with_map` subset covered address, static
settings and OpenAPI paths, but not `clientMaxBodySize` (ADR-125). Separately
the pool idle-expiry test compared `Arc` pointers, which can coincide after a
freed allocation, making it flaky.

Decision:

- the config map accepts `clientMaxBodySize` (case-insensitive) and applies a
  positive value through `set_client_max_body_size`; non-positive values are
  ignored;
- `RedisPool` records how many connections it has opened
  (`created_count()`), incremented for both `min_idle` prefill and `get`
  dials, and the idle-expiry test asserts the count grows instead of
  comparing allocator-dependent pointers.

Not ported: the rest of GoFrame's config surface (logger, hooks, timeouts,
session keys) and per-pool creation metrics beyond the counter.

Consequences: one config-map test covers the 413 path; the pool tests keep
covering reuse, limit and expiry without allocator-dependent assertions.

## ADR-127: `ghttp` supports global serve/output hooks

Status: accepted (2026-11-25)

Context: GoFrame binds hook handlers per route via
`BindHookHandler(pattern, hook, handler)` with four hook points
(`BeforeServe`, `AfterServe`, `BeforeOutput`, `AfterOutput`). The port had
none, so cross-cutting pre/post request logic had to be middleware-only.

Decision:

- `server::HookName` mirrors the four points;
- `Server::bind_hook_handler(hook, handler)` registers handlers globally (no
  route pattern yet);
- `Server::serve` composes the chain as `BeforeServe` → middlewares → route
  handlers → `AfterServe` → `BeforeOutput` → `AfterOutput`, so all hooks run
  on the same `Context` and can read/modify the response;
- an error from a hook follows the normal handler-error path (500).

Not ported: per-route/per-pattern hook binding, `BindHookHandlerByMap`,
hook ordering by registration source and the Go `exceptionExit*` control flow.

Consequences: one server test covers the exact hook order around a route
handler and a header set in `BeforeServe`.

## ADR-128: `gredis` supports ACL usernames in AUTH

Status: accepted (2026-11-25)

Context: GoFrame's `Config.User`/`Pass` map to go-redis's ACL
`AUTH <user> <pass>`; the port only supported the legacy single-password
`AUTH <pass>`, so Redis 6 ACL users could not authenticate.

Decision:

- `Config.user` (empty by default) with `Config::with_user`;
- `open_connection` sends `AUTH <user> <pass>` when a user is configured and
  the password is non-empty, otherwise the legacy `AUTH <pass>`;
- Sentinel bootstrap keeps using the separate sentinel credentials
  (`sentinel_user`/`sentinel_pass`, ADR-114), so master and sentinel auth stay
  independent.

Not ported: passwordless ACL users, `AUTH` retry on later `NOAUTH` errors and
the full `go-redis` option surface.

Consequences: one test asserts the fake server receives
`AUTH app secret` before `PING`.

## ADR-129: `ghttp` binds hooks to route patterns

Status: accepted (2026-11-25)

Context: ADR-127 added global hooks only, while GoFrame's
`BindHookHandler(pattern, hook, handler)` scopes hooks to routes matching a
pattern.

Decision:

- `router::pattern_matches(pattern, path)` exposes the router's own segment
  matcher (static, `:param`, `*wildcard`) for path-only checks;
- `Server::bind_hook_handler_for(pattern, hook, handler)` stores the binding;
  `Server::hook_handlers(hook, path)` merges global hooks from ADR-127 with
  pattern hooks whose pattern matches the request path, preserving
  registration order (global first);
- the serve chain uses those merged lists for all four hook points.

Not ported: `BindHookHandlerByMap`, method-specific hook patterns, hook
ordering across global/pattern registrations by source priority, and the Go
`exceptionExit*` control flow.

Consequences: one test covers `/api/*` and `/admin/*` hooks firing only for
their matching routes (both hook points).

## ADR-130: `gdb` supports table sharding through a handler

Status: accepted (2026-11-25)

Context: GoFrame's `Model.Sharding(ShardingConfig)` computes the concrete
table/database from a sharding value with user callbacks. The port had no
sharding, so time-based tables (`user_2024`, ...) needed manual names on every
call.

Decision:

- `ShardHandler = Arc<dyn Fn(&Value) -> Result<String, Error>>` and
  `ShardingConfig { table: Option<ShardHandler> }` (the database/schema
  callback of Go is not ported);
- `Model::sharding(&value, config)` evaluates the handler and replaces the
  builder's table before any SQL is generated, so later `where`/`order`/writes
  use the sharded table; an empty table name is an error and a missing handler
  leaves the model unchanged;
- the handler runs synchronously during the builder method (no locks held,
  ADR-007).

Not ported: database/schema sharding, `ShardingValue` access from callbacks,
automatic shard routing for joins and the Go hook-based sharding injection.

Consequences: one test covers two shards (`user_2024`/`user_2025`), handler
errors, empty table names and the no-handler passthrough.

## ADR-131: `gstr` case conversions carry the Go test tables

Status: accepted (2026-11-25)

Context: `text/gstr` already implemented the case family in `gstr_more.rs`
(`CaseType`, `case_type_match`, `case_convert`, `case_camel*`,
`case_snake*`, `case_kebab*`, `case_delimited*`,
`case_snake_first_upper`), but only a light smoke test covered it, so the many
Go edge cases (acronyms, digit boundaries, leading underscores) were
unverified.

Decision: translate the Go case-conversion tables into `gstr` tests:

- `CaseCamel` (11 cases), `CaseCamelLower` (4), `CaseSnake` (17),
  `CaseDelimited` (9), screaming/kebab variants (5) and
  `CaseSnakeFirstUpper` (11 + custom underscore);
- `CaseTypeMatch` (case-insensitive names, unknown) and `CaseConvert`
  (camel/snake/lower).

The test tables exposed that the port's existing implementation handles all
Go cases; no behavioural change was needed.

Not ported: none from this family; `CaseSnakeFirstUpper`'s Go TODO about a
character scan instead of regex remains a performance note only.

Consequences: the gstr suite grows from 15 to 21 tests (the whole workspace
is 1005 tests); no public API changed.

## ADR-132: `gstr` adds the missing trim/prefix/newline helpers

Status: accepted (2026-11-25)

Context: comparing GoFrame's exported `gstr` surface with the port's modules
found five still-missing helpers: `HasPrefix`, `HasSuffix`, `Nl2Br`,
`TrimStr` and `TrimRightStr` (the port only had `trim_left_str` without a
count argument).

Decision:

- `trim_right_str(origin, cut, count: Option<usize>)` strips up to `count`
  trailing occurrences (`None` = unlimited), the exact Go loop with the
  empty-`cut` guard;
- `trim_str` strips both sides with the same count semantics and early-returns
  on an empty `cut` (an empty cut would otherwise loop forever);
- `has_prefix`/`has_suffix` are byte-wise `starts_with`/`ends_with`;
- `nl2_br(s, xhtml: Option<bool>)` converts `\r\n`/`\n\r` pairs to one break
  (`<br>` or `<br />`) and single `\r`/`\n` to one break each.

Not ported: Go's variadic `count ...int` (Rust uses `Option<usize>`; `-1` maps
to `None`), the internal `addWordBoundariesToNumbers`/`toCamelInitCase`
helpers (already private in `gstr_more`) and `IsPunctuation` (not exported by
GoFrame's `gstr`).

Consequences: two tests translate the Go `TrimStr`/`TrimRightStr` tables and
cover the prefix/suffix/newline helpers; the gstr suite is 23 tests (whole
workspace 1007).

## ADR-133: `gutil` ports the map/slice helper family

Status: accepted (2026-11-25)

Context: `util/gutil` in GoFrame exposes a family of `map[string]any`/`[]any`
helpers (`MapCopy`, `MapContains`, `MapDelete`, `MapMerge`,
`MapMergeCopy`, `MapOmitEmpty`, `MapToSlice`, `SliceCopy`,
`SliceInsertBefore/After`, `SliceDelete`, `SliceToMap`,
`SliceToMapWithColumnAsKey`); the port only had the comparators and the
case/symbol-insensitive key lookup.

Decision:

- `gutil::Map = IndexMap<String, Value>` is the port's `g.Map`;
- map helpers work on that type: copy/contains/delete/merge/merge-copy and
  `map_omit_empty` (via `is_empty_value`: nil, false, 0, "", empty
  list/map/bytes);
- slice helpers take `&[Value]` and return new vectors; invalid indices return
  the original slice, exactly like Go's `SliceInsert*`/`SliceDelete`;
- `map_to_slice` flattens map insertion order to `[key, value, ...]` and
  returns a list unchanged; `slice_to_map` returns `None` for odd lengths and
  non-lists; `slice_to_map_with_column_as_key` keys the map by
  `gconv::string(column)`.

Not ported: reflection-based pointer/struct handling, `MapPossibleItemByKey`'s
`utils` internals (already ported), `IsEmpty` for structs and Go's variadic
`...any` signatures (Rust uses slices).

Consequences: four tests translate the Go map/slice tables (merge overrides,
empty omission, boundary insert/delete, odd-length conversion, column-keyed
maps); gutil has 10 tests (whole workspace 1011).

## ADR-134: `garray` gains the missing mutators

Status: accepted (2026-11-25)

Context: the generic `Array<T>` lacked several Go `TArray` mutators:
`Replace`, `Fill`, `InsertBefore`/`InsertAfter`, `Pad`, `Reverse` and
`Shuffle`.

Decision:

- `replace(&[T])` overwrites from the beginning up to the shorter length;
- `fill(start, num, value)` overwrites positions from `start` and appends when
  past the end, returning `CODE_INVALID_PARAMETER` for out-of-range starts
  (Go's `Fill` error semantics);
- `insert_before`/`insert_after` return an error for invalid indices and
  otherwise splice the values in order;
- `pad(size, value)` pads to `abs(size)` (right for positive, left for
  negative, no-op when not growing);
- `reverse` and `shuffle` mutate in place (shuffle uses `gf_util::grand`).

Not ported: pointer/struct-slice variants (`TArray[T]` is generic in the port
already), `Iterator`/`Walk` callback forms (already present) and Go's
`lazyInit`.

Consequences: one test covers replace (shorter/longer), fill overwrite/append
and the two invalid-start errors, middle insert errors, both pad directions,
reverse and shuffle (multiset preserved).

## ADR-135: `glist` gains peek/insert/move/remove helpers

Status: accepted (2026-11-25)

Context: `container/glist` in GoFrame exposes `Front`/`Back`,
`InsertBefore`/`InsertAfter`, `MoveToFront`/`MoveToBack`,
`MoveBefore`/`MoveAfter`, `RemoveAll`, `Removes` and `DeepCopy`; the port had
only the push/pop/remove_at primitives.

Decision:

- `front`/`back` return clones without removing;
- `insert_before`/`insert_after` take an index and return
  `CODE_INVALID_PARAMETER` for out-of-range indices (Go's `*Element` API
  becomes index-based per ADR-030);
- `move_before`/`move_after` remove the source element, adjust the mark index
  after removal and re-insert, matching Go's element-moving semantics;
  `move_to_front`/`move_to_back` delegate to the deque;
- `remove_all` clears, `removes(&[T])` drops every matching value
  (`T: PartialEq`), `deep_copy` clones into a fresh list.

Not ported: `Next`/`Prev` element cursors, `Iterator*` callbacks,
`LockFunc`/`RLockFunc`, JSON marshalling on the generic list and
`PushBackList`/`PushFrontList` (already covered by the `*_backs` helpers).

Consequences: one test covers peek, insert (and both invalid-index errors),
all four moves, `remove_all`, `removes` and an independent `deep_copy`.

## ADR-136: `gset` gains copy/iteration/conditional-add helpers

Status: accepted (2026-11-25)

Context: GoFrame's `Set` exposes `DeepCopy`, `Iterator`, `Walk` and
`AddIfNotExistFunc`; the port had the algebra (union/intersect/diff/...) and
the basic add/remove but not these helpers.

Decision:

- `add_if_not_exist_func(item, f)` inserts only when the item is absent and
  the callback returns true, returning whether it inserted;
- `deep_copy` clones into an independent set;
- `iterator(f)` snapshots the set and calls `f` until it returns false
  (callbacks never run under the lock, ADR-007);
- `walk(f)` snapshots and visits every item.

Not ported: `LockFunc`/`RLockFunc` (would expose the internal `IndexSet` in
the public API), `SetNilChecker`, `ContainsI` (generic sets have no
case-insensitive comparison), `String`/JSON marshalling and `Sum` (generic
elements have no numeric conversion).

Consequences: one test covers conditional insertion (all three branches),
early-stopping iteration, full walk and copy independence.

## ADR-137: `gtree` ceiling/floor/pop/flip are pinned by Go tables

Status: accepted (2026-11-25)

Context: the ordered trees already implemented `ceiling`, `floor`,
`pop_left`, `pop_right` and `flip` (returning node references instead of Go's
`(value, found)` pair because the port has no node pointer API, ADR-030), but
no test pinned the boundary semantics or the in-place `flip`.

Decision: translate the Go `Test_AVLTree_CeilingFloor` table (found-equal,
found-not-equal, not-found) plus `PopLeft`/`PopRight` and `Flip`
examples into one test:

- `ceiling(8).value == "val8"`, `floor(20).value == "val20"`;
- `ceiling(9).value == "val10"`, `floor(5).value == "val4"`;
- `ceiling(21)`/`floor(-1)` return `None`, as do poppping an empty tree;
- `pop_left`/`pop_right` return the boundary node and shrink the tree;
- `flip(comparator)` swaps keys and values in place (`a -> 1`, `b -> 2`).

No production code changed (the existing implementation passed every case);
the unit closes the coverage gap for these tree operations.

Not ported: the B-tree variants of these operations keep their existing
coverage and Go's `*Element` node cursors remain index-free (ADR-030).

Consequences: gtree gains one test; the container suite is 187 tests (whole
workspace 1015).

## ADR-138: `Accept-Language` negotiation replaces the context language

Status: accepted (2026-11-25)

Context: GoFrame's `gi18n` reads the request language from
`context.Context`; the port has no context (ADR-052), so `gi18n` only offered
`translate_with_language` and nothing parsed `Accept-Language`.

Decision:

- `gi18n::parse_accept_language` parses the header into lower-case tags
  ordered by `q` value, dropping `q=0` entries, the `*` wildcard and empty
  parts;
- `gi18n::negotiate_language(header, available, fallback)` tries exact tags
  first, then primary-subtag matches, then `fallback`;
- `ghttp::language::middleware_accept_language` stores the best tag on
  `Context::language` (empty header → `DEFAULT_LANGUAGE`, English);
  handlers pass it to `translate_with_language`;
- `gf-net` depends on `gf-i18n` (acyclic: i18n depends only on
  core/encoding/errors/json/util).

Not ported: per-request i18n `Manager` lookup, `I18nLanguage` auto-injection
into `gview` (still an explicit parameter, ADR-088), `Content-Language`
response headers and Go's context value storage.

Consequences: one `gf-i18n` test covers parsing/negotiation (exact, primary
subtag, fallback) and two `ghttp` tests cover the middleware default and the
ordered header.

## ADR-139: `gredis` cluster can route reads to replicas

Status: accepted (2026-11-25)

Context: GoFrame's `Config.SlaveOnly` routes commands to slave nodes; the
port's cluster client (ADR-116) parsed only the master node of each
`CLUSTER SLOTS` range and always used it.

Decision:

- `Config.slave_only` with `with_slave_only(true)`;
- `SlotRange` gains `replicas: Vec<String>`, parsed from the third and later
  node arrays of each `CLUSTER SLOTS` entry;
- `address_for` returns the first advertised replica when `slave_only` is set
  and the range has one, otherwise the master;
- the single-node and Sentinel clients ignore the flag (they have no replica
  discovery), as documented.

Not ported: round-robin/random replica selection, replica health checks,
read-your-writes routing, `CLUSTER NODES` parsing and master change
re-discovery.

Consequences: one test covers a `CLUSTER SLOTS` range with a replica
(master GET returns `from-master`, `slave_only` GET reaches the replica and
returns `from-replica`).

## ADR-141: `gproc` gets a process-manager lane

Status: accepted (2026-11-25)

Context: ADR-057 ported only `gproc`'s shell lane; GoFrame also has a process
manager (`Manager`, `Process`, `StartProcess`, `AddProcess`, `GetProcess`,
`Kill`, `Wait`). Rust's `std::process::Child` supports spawn/kill/wait, so
the manager lane is portable; goroutine identity and cross-thread signals
are not.

Decision:

- `gproc::Process` wraps a `Child` behind a `Mutex`, exposing `name`, `pid`,
  `kill`, `wait` (exit code), `is_running` (`try_wait`) and `release`
  (detach without killing);
- `gproc::Manager` is a `RwLock<HashMap<String, Arc<Process>>>` registry with
  `start` (spawn and replace an existing same-name process by killing it),
  `add_process`, `get_process`, `remove_process`, `processes`, `kill_all` and
  `wait_all`; `gproc::manager()` provides the global instance like
  `GetManager`;
- user callbacks are not involved, but the child handle is never used while
  the registry lock is held.

Not ported: goroutine ids/communication, `os.Signal` handlers, process
environment/telemetry injection, `Process.Signal`, `Release` bookkeeping
beyond dropping the handle, and Go's `StartProcess` option surface.

Consequences: two tests cover spawn/get/wait exit codes and removal, plus
same-name replacement and kill (with a bounded wait for the child to stop).

## ADR-142: `gdb` Model gains NOT/OR condition helpers

Status: accepted (2026-11-25)

Context: the builder only had `or_where_eq`/`or_where_raw` and the Model only
AND-style helpers; GoFrame's Model exposes `WhereNotLike`,
`WhereNotBetween`, `WhereRaw` and the `WhereOr*` family.

Decision:

- the builder gains `or_where_op`, `where_not_between`, `or_where_in`,
  `or_where_not_in`, `or_where_like`, `or_where_not_like`,
  `or_where_between`, `or_where_not_between`, `or_where_null` and
  `or_where_not_null` (all delegating to the existing `WhereHolder` model);
- the Model gains `where_not_between`, `where_not_like`, `where_raw`,
  `or_where_op`/`or_where_gt`/`or_where_ge`/`or_where_lt`/`or_where_le`/
  `or_where_ne`, plus the OR forms of like/in/between/null/raw;
- `OR` conditions chain left-to-right in insertion order, matching the SQL
  builder's existing `WhereKind` handling.

Not ported: GoFrame's map/struct-based `WhereOr(value...)` overloads, grouped
`(a OR b) AND c` composition helpers, and `WherePrefix`/`WhereSuffix`.

Consequences: one test covers `NOT BETWEEN`, `OR >=`, `OR IS NULL`,
`OR IN`, raw fragments and `NOT LIKE` (including SQLite's NULL semantics for
`NOT LIKE`).

## ADR-143: `ghttp` exposes the registered route map

Status: accepted (2026-11-25)

Context: GoFrame's `Server.GetRoutes`/router map dump lists registered
routes; the port's `Router` kept only parsed segments, so there was no way to
inspect them.

Decision:

- `Router::route_list()` rebuilds `(methods, pattern)` from the parsed
  segments (`:param` and `*wildcard` render back to their source form);
- `Server::routes()` delegates, providing the port's `GetRoutes` equivalent;
- insertion order is preserved (GoFrame sorts the dump; callers can sort).

Not ported: the formatted router-map dump (`SetDumpRouterMap`), route
priority ordering in the listing, domain-scoped routes and handler names in
the output.

Consequences: one router test and one server test cover the listing,
multi-method routes and the empty router.

## ADR-144: `ghttp` session cookies honour a server cookie config

Status: accepted (2026-11-25)

Context: GoFrame's server has cookie settings (`SetCookieDomain`,
`SetCookiePath`, `SetCookieMaxAge`, `SetCookieSecure`, `SetCookieHttpOnly`,
`SetCookieSameSite`) that its session cookie uses; the port hard-coded
`Path=/` and `HttpOnly`.

Decision:

- `CookieConfig { domain, path, max_age, secure, http_only, same_site }`
  with defaults `path="/"`, `http_only=true` and empty domain/same_site;
- `Server::set_cookie_config`/`cookie_config` plus the individual
  `set_cookie_*` setters and `cookie_domain` getter;
- `close_session` builds the session `Set-Cookie` from the config (empty
  domain/same_site and zero max-age are omitted, as Go does).

Not ported: the per-request `Cookie` object API beyond the response helpers,
cookie signing/encryption and the full `Config`-map wiring for these keys.

Consequences: one test covers a session login with domain/path/max-age/
secure/same-site configured and asserts every attribute on the response
cookie.

## ADR-145: `gview` supports `{{break}}` and `{{continue}}`

Status: accepted (2026-11-25)

Context: Go's template language permits `{{break}}`/`{{continue}}` inside a
`{{range}}` body; the port had no control-flow channel out of `render_nodes`,
so neither action could work.

Decision:

- new `Node::Break`/`Node::Continue` parsed from the bare actions;
- `render_nodes` returns a `Flow` (`Normal`/`Break`/`Continue`) that `if`,
  `with`, `template` bodies and nested ranges propagate;
- a range stops on `Break`, skips the rest of the body on `Continue`, and
  always restores the saved dot/variables afterwards;
- `{{break}}`/`{{continue}}` outside a range makes the top-level render fail
  with an error (Go rejects this at parse time).

Not ported: labelled break/continue (Go has none), `{{break}}` inside
invoked templates breaking the caller's range (the flow propagates, which is
an acceptable superset), and Go's exact parse-time diagnostic text.

Consequences: one test covers break after `2`, continue at `2` and both
out-of-range errors.

## ADR-146: `glog` gains `Cat` and `Skip` chaining helpers

Status: accepted (2026-11-25)

Context: GoFrame's logger offers chainable `Cat(category)` (writes below a
category subdirectory) and `Skip(skip)` (extra stack frames skipped), which
the port only exposed as the lower-level `with_cat`/`set_stack_skip`.

Decision:

- `Logger::cat(category)` delegates to `with_cat` (same path joining);
- `Logger::skip(skip)` clones the logger and raises `st_skip` by `skip`
  (negative values clamp to zero);
- `level_prefix(level)` already provided the `GetLevelPrefix` mapping, so no
  duplicate accessor was added;
- `Config`/`ctx_keys` remain context-free (ADR-057 rationale): `To`/`Cat`
  context-key capture is not ported.

Not ported: `To(ctxKeys)`/`AppendCtxKeys` context output, `Clone` (Rust's
`.clone()`), `GetConfig` (the `config()` snapshot), and
`SetConfigWithMap` (the port uses typed `Config` setters).

Consequences: one test covers prefix lookup/override, category path joining
and the increased stack skip.

## ADR-147: `gdb` Builder/Model gain JOIN clauses

Status: accepted (2026-11-25)

Context: the builder docstring listed joins as not ported, so every
multi-table query had to be a raw SQL call; GoFrame's Model exposes
`LeftJoin`/`RightJoin`/`InnerJoin`.

Decision:

- `JoinKind` (`Inner`/`Left`/`Right`) with its SQL keywords and
  `JoinClause { kind, table, on }`;
- `Builder::{join, left_join, right_join, inner_join}` store clauses and
  `select_sql` renders `SELECT ... FROM t [LEFT|RIGHT|INNER] JOIN u ON ...`
  before the tail clauses;
- `Model::{left_join, right_join, inner_join}` delegate;
- the `ON` fragment is raw SQL (no bound arguments), matching how
  `where_raw` is handled; callers needing parameters use `where_raw` after
  the join.

Not ported: join sub-queries, table aliases beyond raw text, `USING`,
`NATURAL JOIN`, join arguments/placeholders, and GoFrame's `Join` overload
that takes a table-plus-condition list.

Consequences: one test joins `user` with `orders` (inner join filtering and
ordering, plus a left join preserving an order-less user with a NULL amount).

## ADR-148: `ghttp.Response` gains the write/buffer helpers

Status: accepted (2026-11-25)

Context: GoFrame's response exposes `Write`, `WriteJson`, `WriteStatus`,
`Buffer`, `BufferLength` and `ClearBuffer`; the port only had the setters, so
handlers could not append incrementally or inspect the buffered body.

Decision:

- `write(bytes)` appends to the body (no implicit status flush, unlike a real
  `http.ResponseWriter`);
- `write_json(&Value)` appends the serialized JSON and sets
  `Content-Type: application/json`;
- `write_status(status, content)` sets the status and appends `content`;
- `buffer`/`buffer_length`/`clear_buffer` expose and reset the body;
- `set_status`/`status_code` and `header`/`set_header` round out the message
  API (the fields stay public for the existing call sites).

Not ported: `WriteExit`/`WriteJsonExit`/`WriteStatusExit` (no `Context.Exit`
during a write), `WriteTpl` (no view on the response), streaming/chunked
flushing, and the `http.ResponseWriter` interface itself.

Consequences: one test covers append, buffer inspection/clear, JSON append
with content type, status set and header get/set.

## ADR-149: `gdb` gains `InsertIgnore`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `InsertIgnore`, which skips duplicate-key
rows instead of failing; the port only had plain `insert`.

Decision:

- `Builder::insert_ignore_sql(data)` rewrites the generated `INSERT` per
  dialect: `INSERT IGNORE INTO` (MySQL), `INSERT OR IGNORE INTO` (SQLite),
  and `INSERT ... ON CONFLICT DO NOTHING` (PostgreSQL);
- `Model::insert_ignore(data)` applies the model's time fields, executes the
  statement through `Db::execute`, clears the model cache and returns the
  affected-row result.

Not ported: `Replace`/`Save`/upsert (`ON CONFLICT ... DO UPDATE`), conflict
targets (`ON CONFLICT (cols)`), and `InsertIgnore` with a column list
distinct from the data keys.

Consequences: one test inserts a row, repeats the primary key with different
data (0 rows affected), and verifies the original row was kept.

## ADR-150: `gcache` completes the `Must*` convenience set

Status: accepted (2026-11-25)

Context: GoFrame's cache exposes `MustData`, `MustKeyStrings` and
`MustGetOrSetFuncLock`; the port had the non-panicking forms but not these
three wrappers.

Decision: add `must_data`, `must_key_strings` and
`must_get_or_set_func_lock`, all panicking with the wrapped error message on
failure like the existing `must_*` helpers.

Not ported: none beyond the already-recorded adapter gaps (Redis adapter,
file-crypto lanes).

Consequences: one test covers key/data snapshots after two inserts, the
locking get-or-set producing a third entry, and `must_size`.

## ADR-151: `gjson` completes the len and `Must*` encoder surface

Status: accepted (2026-11-25)

Context: the `gjson::Json` document already implements dotted paths,
set/remove and the format encoders (ADR-045), but it lacked GoFrame's
`Len`/`IsNil` and the `MustTo*` panicking variants, so callers had to unwrap
every encoder result.

Decision: add `len` (maps/lists/strings), `is_empty`, `is_nil`, and
`must_to_json`/`must_to_xml`/`must_to_yaml`/`must_to_toml`/`must_to_ini`/
`must_to_properties`, all panicking with the wrapped error message.

Not ported: `ToStruct` (Rust has no reflection; callers deserialize the
`Value` directly), `GetVar`/`GetVarOrSet` (`Var` accessors), and the filter
syntax already recorded in ADR-045.

Consequences: one `gf-encoding` test covers the document length, nil/empty
flags and every `MustTo*` encoder on a small map plus a nil document.

## ADR-152: `gdb` Model `Save` upserts by explicit key fields

Status: accepted (2026-11-25)

Context: GoFrame's `Model.Save` updates by the table primary key and inserts
when no row matched. The port has no table-metadata/reflection layer, so the
primary key cannot be discovered automatically.

Decision: `Model::save(data, key_fields)` requires the key field names:

- each key must be present in `data` (missing key → `CODE_INVALID_PARAMETER`);
- a clone of the builder gains `key = value` conditions for every key;
- the update runs first (with update-time fields); when it affects at least
  one row the result is returned;
- otherwise the row is inserted (with insert-time fields) and its result
  returned; the model cache is cleared on both paths.

Not ported: automatic primary-key discovery from table metadata, composite
key detection, `Save` on joined/sharded models with different tables, and
GoFrame's `ON DUPLICATE KEY UPDATE` path.

Consequences: one test updates an existing user by `id`, inserts a new `id`,
and asserts that a missing key field errors.
## ADR-153: `gvalid` assoc data reaches the builtin rules

Status: accepted (2026-11-25)

Context: `Validator::assoc` stored the associated map but the scalar
`run`/`check_value` lanes passed `Value::Null` as the rules' data, so
`required-if`, `required-unless`, `required-with*` and `required-without*`
never saw the associated fields (Go passes `v.assoc` as `DataRaw`).

Decision: the scalar lane now forwards `self.assoc` (or `Value::Null`) to
`do_check_value`, and therefore to both builtin `RunInput.data` and custom
`RuleFuncInput.data`. The map/struct lanes keep passing the validated map, as
Go does.

Not ported: `useAssocInsteadOfObjectAttributes` (no struct lane), recursive
assoc traversal, and `Assoc` on `Data` that is itself a map (map lane uses the
map, per Go).

Consequences: one translated test covers the Go tables for `phone`,
`phone-loose`, `telephone`, `postcode`, `qq`, `password`/`2`/`3`,
`resident-id`, `bank-card`, `date`, `datetime`, `date-format` and the seven
`required-if*-/unless/with*/without*` rules through `assoc`.
## ADR-154: `ghttp.Request` parses `multipart/form-data`

Status: accepted (2026-11-25)

Context: the message model documented multipart forms as not ported, so
uploaded files and multipart fields were invisible to handlers.

Decision:

- `MultipartPart { name, file_name, content_type, data }` and
  `MultipartForm { boundary, parts }` with `value(name)` (plain fields) and
  `file(name)` (uploaded files) lookups;
- `Request::multipart_form()` extracts the boundary from the
  `Content-Type`, walks the body byte-wise (`\r\n--boundary` separators) and
  parses each part's `Content-Disposition`/`Content-Type`;
- `Request` also gains `header`, `body_string`, `host` and `uri` helpers.

Not ported: `multipart/mixed` and nested multiparts, quoted boundary
escapes, temp-file spooling (all data stays in memory), chunked bodies and
the GoFrame `UploadFile` struct (the port exposes the raw part).

Consequences: one test covers a two-part body (plain field + text file),
boundary/part headers, `value`/`file` lookups, the request helpers and the
non-multipart case.
## ADR-155: `ghttp.Request` parses chunked request bodies

Status: accepted (2026-11-25)

Context: `Request::parse` only honoured `Content-Length`; a
`Transfer-Encoding: chunked` body was silently dropped (the module doc listed
chunked as not ported).

Decision: when the `Transfer-Encoding` header contains `chunked`, the parser
reads hex chunk sizes (ignoring `;ext` extensions), reads each chunk and its
`\r\n` terminator, stops at size `0`, and drains trailers until the blank
line. An invalid size or terminator is an error.

Not ported: chunked *response* writing, gzip/content-encoding layering,
trailer exposure to handlers, and HTTP/1.1 pipelining semantics.

Consequences: one test parses a two-chunk `Wikipedia` body, checks the host
and URI helpers and asserts that an invalid chunk size errors.
## ADR-156: `ghttp.Response` writes chunked bodies

Status: accepted (2026-11-25)

Context: ADR-155 added chunked request parsing but responses always wrote
`Content-Length`; Go's server switches to chunked encoding when the length is
unknown or chunking is requested.

Decision: when the response carries `Transfer-Encoding: chunked`,
`Response::write_to` omits `Content-Length`, writes the body as one hex-sized
chunk (`{:x}\r\n<body>\r\n`) and terminates with `0\r\n\r\n`; empty bodies
emit only the terminator. Non-chunked responses keep the existing
`Content-Length` behaviour.

Not ported: streaming/chunk-by-chunk flushing from handlers, gzip layering
with chunked encoding, HTTP/1.0 fallback semantics, and trailers.

Consequences: one test serializes a chunked `hello` response (no
`Content-Length`, exact chunk framing) and an empty 204 chunked response.
## ADR-157: `gclient` decodes chunked and length-framed responses

Status: accepted (2026-11-25)

Context: `gclient::parse_response` returned every byte after the header block
as the body, so chunked responses leaked their framing and over-long
`Content-Length` bodies were kept whole (the module doc still listed redirects
as unported although ADR-083 had implemented them).

Decision: after parsing the status line and headers, the parser now:

- decodes `Transfer-Encoding: chunked` bodies (hex sizes, optional `;ext`,
  `\r\n` terminators, `0` terminator) with errors for invalid sizes or
  truncation;
- otherwise truncates the body to `Content-Length` when that header is
  present;
- keeps the close-delimited body when neither header is present.

Not ported: chunked request bodies from the client, gzip/`Content-Encoding`
decoding, trailer headers and streaming response callbacks.

Consequences: one test decodes a two-chunk body, truncates an over-long
declared length, and asserts an invalid chunk size errors.
## ADR-158: `ghttp.Context` gains the value getters

Status: accepted (2026-11-25)

Context: handlers could read `Context::params`/`request.query` manually, but
GoFrame's Context exposes `Get`/`GetVar`/`GetRequestMap`; the port had no
unified lookup.

Decision:

- `Query::entries()` returns each key with its first value;
- `Context::get(name)` checks router params first, then the urlencoded form,
  then the query, returning `Option<Value>`;
- `get_value` returns `Value::Null` when absent and `get_string` returns an
  empty string;
- `get_map()` merges query → form → params (later sources win).

Not ported: `*gvar.Var` wrappers (`get_value` returns `Value`), body JSON
field lookup, multipart field merging into `get_map` (callers use
`request.multipart_form()`), and GoFrame's struct-tag data binding.

Consequences: one test covers param/form/query precedence, the typed
getters, the missing-value forms and the merged map.
## ADR-159: `gredis` round-robins cluster replicas

Status: accepted (2026-11-25)

Context: ADR-139 routed `slave_only` reads to the first replica advertised by
`CLUSTER SLOTS`, so every read hit the same node.

Decision: `ClusterClient` keeps an `AtomicUsize` cursor; when `slave_only` is
enabled and a range advertises replicas, `address_for` picks
`replicas[cursor % len]` and increments the cursor, distributing reads evenly
across the discovered replicas.

Not ported: replica health checks, read-your-writes routing, per-key replica
affinity, weighted selection, and replica rediscovery on topology changes.

Consequences: one test with two replicas issues four `GET`s and asserts each
replica received two.
## ADR-160: `gdb` Builder/Model gain `DISTINCT`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `Distinct()`; the port had raw `fields`
strings only, so callers prepended `DISTINCT` manually and the SELECT builder
could not model it.

Decision:

- `Builder.distinct` (default false) with `Builder::distinct()`;
- `select_sql` renders `SELECT DISTINCT <fields> FROM ...`;
- `Model::distinct()` delegates; other statements ignore the flag.

Not ported: `Distinct` interaction with aggregate `COUNT`, `DISTINCTROW`,
PostgreSQL `DISTINCT ON (...)`, and automatic field expansion for distinct
queries.

Consequences: one test inserts a duplicate age, then compares a four-row
plain projection with a three-row distinct projection.
## ADR-161: `gdb` gains WHERE prefix/suffix helpers

Status: accepted (2026-11-25)

Context: GoFrame's builder exposes `WherePrefix`/`WhereSuffix` (LIKE with an
added `%`) plus their OR forms; callers had to format the patterns by hand.

Decision: `Builder::{where_prefix, where_suffix, or_where_prefix,
or_where_suffix}` and the matching `Model` methods render
`field LIKE 'prefix%'` / `field LIKE '%suffix'` through the existing `Like`
operator (`AND`, or `OR` for the `or_` forms).

Not ported: escaping `%`/`_` inside the user prefix/suffix (GoFrame does not
escape either), `WhereContains` (use `where_like`), and case-insensitive
variants (use the `ci` flag where available).

Consequences: one test covers prefix (two names), suffix (one name) and an
OR prefix combined with `where_eq`.
## ADR-162: `gdb` Model gains `FieldsEx`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `FieldsEx` (select every field except the
given ones); the port only had explicit `fields`.

Decision: `Model::fields_ex(excluded)` reads the model's table fields
(`table_fields`), filters out the excluded names case-insensitively, and sets
the builder's field list to the joined remainder. When the table metadata is
unavailable or every field is excluded, the default `*` projection is kept.

Not ported: `FieldsEx` for joined tables (metadata covers the base table
only), struct-tag field aliases, and GoFrame's lazy field resolution
semantics.

Consequences: one test excludes `name` and `note` and asserts the row keeps
`id`/`age` but not the excluded columns.
## ADR-163: `gdb` Model gains offset/omit/has-field/insert-id helpers

Status: accepted (2026-11-25)

Context: several small GoFrame Model methods were missing: `Offset`
(offset without touching the limit), `OmitEmpty`/`OmitNil` (drop such values
from insert/update data), `HasField` (public field check) and
`InsertAndGetId`.

Decision:

- `Builder::offset` and `Model::offset` set only the offset;
- `Model::omit_empty`/`omit_nil` record flags; insert, insert-ignore, update
  and the update/insert halves of `save` filter the data through
  `filtered_data` (`is_nil` / `gutil::is_empty_value`);
- `Model::has_field(name)` is the public, case-insensitive field check; the
  internal soft-delete probe is renamed `table_has_field` (still fallible);
- `Model::insert_and_get_id` inserts and returns `last_insert_id`, erroring
  when the driver did not report one.

Not ported: `OmitEmptyWhere`/`OmitNilWhere` (where-side), `OmitEmptyData`/
`OmitNilData` variants, `Data`/`Array` column extraction and `Replace`.

Consequences: one test covers offset-after-limit, insert-id retrieval,
omit-empty update semantics and the has-field probe.
## ADR-164: `gdb` gains `Increment`/`Decrement`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `Increment`/`Decrement`, which issue
`UPDATE ... SET field = field + n`; the port only wrote explicit values.

Decision:

- `Builder.increments` records `(field, amount)` pairs;
- `update_sql` appends `field= field + (amount)` assignments after the data
  assignments (amounts are integers, rendered literally; field names go
  through the dialect quoting);
- `Model::increment(field, amount)` calls the update lane with an empty data
  slice; `Model::decrement` delegates with the negated amount and both clear
  the model cache.

Not ported: `Increment`/`Decrement` on struct data with automatic timestamps
(no data slice means no time fields are touched), float amounts, and the
`UpdateAndGetAffected`-style return of refreshed rows.

Consequences: one test increments an age by 2 (18 → 20) and decrements it by
5 (→ 15), checking the affected count on the first call.
## ADR-165: `gdb` Model gains `Data`/`Array`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `Data` (first row as a field map) and
`Array` (one column's values across rows); callers had to loop `all()`
manually.

Decision:

- `Model::data()` returns the first matching row as a `Map`, or an empty map;
- `Model::array(field)` returns that field's values across the rows, using
  the first column when `field` is `None`; a missing field in a row yields
  `Value::Null`.

Not ported: `Array`'s variadic multi-field form, `Data`'s struct-mapping
behaviour, and column-name inference through struct tags.

Consequences: one test reads a single row via `data()` and checks both
`array(Some("name"))` and `array(None)` against the ordered rows.
## ADR-166: `gdb` gains table aliases and batch insert

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `As` (table alias, needed to qualify columns
in joins) and `Batch` (insert many rows with a chunk size); the port had
neither.

Decision:

- `Builder.alias` + `Builder::as_alias` render `FROM "table" AS "alias"` in
  `select_sql`; `Model::as_alias` delegates; other statements keep the plain
  table name;
- `Model::batch(data, batch_size)` walks the rows in chunks of at least one,
  applies the time fields and omit filters per row, inserts them one by one
  through `Db::insert`, sums the affected counts, keeps the last insert id
  and clears the model cache;
- batching is not transactional (a failing row stops the loop with earlier
  rows already inserted), matching the port's non-transactional helper
  granularity rather than GoFrame's batch SQL.

Not ported: single-statement multi-row INSERT, transactional batching,
`As` on UPDATE/DELETE statements, and alias-aware field qualification.

Consequences: one test selects through the `u` alias and batch-inserts three
rows in chunks of two (affected 3, table count 6).
## ADR-167: `gdb` Model gains `Raw` SELECT

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `Raw(sql, args...)` to run a hand-written
SELECT through the model's result pipeline; the port only had `where_raw`
(raw WHERE fragments) but no raw projection.

Decision:

- `Builder.raw: Option<(String, Vec<Value>)>` with `Builder::raw(sql, args)`;
  `select_sql` returns the raw statement and arguments verbatim when set;
- `Model::raw(sql, args)` delegates, so `all`/`one`/`data`/`array` and the
  model cache keep working over the raw query;
- the raw path bypasses table metadata (no soft-delete filter, no field
  mapping); callers own the SQL and its dialect placeholders.

Not ported: GoFrame's variadic `Raw(sql, args ...any)`, raw UPDATE/DELETE
statements, sub-query models, and automatic placeholder rewriting per
dialect (`?` for MySQL/SQLite, `$n` for PostgreSQL).

Consequences: one test runs `SELECT name, age FROM user WHERE age > ? ORDER
BY age` with one bound argument and checks the two ordered rows.
## ADR-168: `gdb` gains `CountColumn` and `QuoteWord`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `CountColumn(field)` and `QuoteWord(word)`;
the port had neither (only the row-level `count` and dialect-internal
quoting).

Decision:

- `Model::count_column(field)` delegates to the existing `aggregate` lane
  with `COUNT(field)` and converts the first value to `i64`;
- `Builder::quote_word` and `Model::quote_word` expose the dialect's
  identifier quoting (MySQL backticks, PostgreSQL/SQLite double quotes).

Not ported: `CountColumn` with `DISTINCT`, `QuoteWord` for dotted paths
(callers use `quote_path` internally), and GoFrame's ``QuoteWord``
placeholder expansion in raw strings.

Consequences: one test counts the `age` column across three rows and checks
the quoting of `name` for the SQLite model plus MySQL/PostgreSQL builders.
## ADR-169: `gdb` gains `Replace`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `Replace` (delete-then-insert semantics per
the database's REPLACE statement); the port only had insert/insert-ignore/save.

Decision:

- `Builder::replace_sql(data)` rewrites the INSERT per dialect: `REPLACE
  INTO` (MySQL), `INSERT OR REPLACE INTO` (SQLite); PostgreSQL keeps the
  plain INSERT and the model rejects the call with an error that points to
  `save`;
- `Model::replace(data)` applies the ordinary (non-insert) time fields, the
  omit filters and executes through `Db::execute`, clearing the model cache.

Not ported: PostgreSQL `ON CONFLICT ... DO UPDATE` replacement, MySQL's
`REPLACE` interplay with foreign keys, and partial-column replace semantics
(omitted columns become defaults/NULL, as the database defines).

Consequences: one test replaces primary key 10 twice and verifies the second
write overwrote the name.
## ADR-170: `ghttp` enforces a request header limit

Status: accepted (2026-11-25)

Context: GoFrame's server offers `SetMaxHeaderBytes`; the port parsed headers
with no ceiling, so an oversized header block could grow memory unboundedly.

Decision:

- `Request::parse_with_limit(reader, max_header_bytes)` counts the request
  line and every header line, erroring with "request header too large" once
  the limit is crossed; `Request::parse` delegates with `usize::MAX`;
- `Server::set_max_header_bytes`/`max_header_bytes` store the limit;
  both the plain and TLS accept loops parse with it and write a bare
  `HTTP/1.1 431 Request Header Fields Too Large` response before closing when
  the marker error surfaces.

Not ported: per-request dynamic limits, config-map wiring of the key, exact
Go `http.Server` accounting (which counts only header bytes, not the request
line), and 431 bodies.

Consequences: one message test checks the limit boundary and one server test
over TCP receives the 431 response.
## ADR-171: `gdb` gains `OnConflict`-style upsert

Status: accepted (2026-11-25)

Context: GoFrame exposes `OnConflict`/`OnDuplicate` INSERT modifiers; the
port had plain insert, insert-ignore, replace and `save`, but no
single-statement upsert.

Decision:

- `Builder::insert_on_conflict_sql(data, key_fields)` appends the dialect's
  conflict clause: `ON DUPLICATE KEY UPDATE col = VALUES(col)`
  (MySQL) or `ON CONFLICT (keys) DO UPDATE SET col = excluded.col`
  (SQLite/PostgreSQL), updating every non-key column present in `data`;
- `Model::insert_on_conflict(data, key_fields)` applies insert-time fields
  and omit filters, executes the statement and clears the model cache;
- when `data` has only key columns the plain INSERT is executed (no clause).

Not ported: `OnDuplicateEx` (explicit update columns), PostgreSQL conflict
targets other than the supplied keys, MySQL `VALUES()` replacement syntax
changes, and `OnConflict` with `DO NOTHING` (use `insert_ignore`).

Consequences: one test seeds id 20, upserts with new name/age and verifies
both non-key columns changed.
## ADR-172: `gsession` file storage can encrypt session files

Status: accepted (2026-11-25)

Context: GoFrame's `StorageFile` has `SetCryptoEnabled`/`SetCryptoKey` and
AES-encrypts the JSON payload (after the 8-byte timestamp prefix); the port
stored plaintext JSON.

Decision:

- `StorageFile::set_crypto_enabled(bool)` and `set_crypto_key(&[u8])`
  mutate the shared storage state; the default key is Go's
  `DefaultStorageFileCryptoKey` (`Session storage file crypto key!`);
- `set_session` encrypts the JSON bytes with `gcrypto::gaes` before writing
  the timestamp prefix; `get_session` decrypts before parsing;
- the timestamp prefix stays plaintext so expiry checks/touches keep working.

Not ported: Go's `gaes.Encrypt` byte-for-byte format compatibility check
(the port uses its own `gcrypto::gaes`), key rotation, and encryption for the
Redis storage (no Redis lane).

Consequences: one test enables crypto with a 16-byte key, writes a session,
asserts the file contains no plaintext and reads the value back through a
new session.
## ADR-173: `gres` gains `PackToGoFile`

Status: accepted (2026-11-25)

Context: `MAP.md` recorded `PackToGoFile` as not ported; GoFrame uses it to
embed a packed resource archive into a generated Go source file.

Decision: `gres::pack_to_go_file(src_paths, go_file_path, package,
key_prefix)` packs the paths (gzip+zip, existing `pack`), BASE64-encodes the
bytes and writes a Go file with `package <name>`, the `gres` import and an
`init` calling `gres.Add("<base64>")` with the same panic text as Go.

Not ported: `PackToGoFileWithOption` (the port has `PackOption` and
`pack_with_option`; callers can compose the two manually), Go's deprecated
variadic `keyPrefix`, and exact template whitespace compatibility.

Consequences: one test packs two temp files and checks the generated package
header, import, `gres.Add(` and `func init()`.
## ADR-174: `gf-database` provides the Redis cache adapter

Status: accepted (2026-11-25)

Context: `MAP.md` recorded `os/gcache` as gated without the Redis adapter.
The adapter must use `gf-database::gredis`, but `gf-os` cannot depend on
`gf-database` (the latter already depends on `gf-os`), so it cannot live in
`gf-os`.

Decision: implement `gf_database::gcache_redis::RedisAdapter` against the
`gf_os::gcache::Adapter` trait plus `new_adapter_redis(config)`:

- keys use `gconv::string(key.value())` (Go's `gconv.String`);
- values are stored as JSON text so `Value` types survive, unlike GoFrame's
  direct Redis string storage (recorded divergence);
- TTLs use `SET ... PX` (milliseconds) and `PEXPIRE`/`PERSIST` for
  `update_expire`;
- `get_expire` returns `None` for both "no expiry" and "missing" (PTTL -1/-2);
- `size` is `DBSIZE`, `clear` is `FLUSHDB`, `keys`/`data`/`values` use
  `KEYS *` (GoFrame does the same), `remove` returns the last old value and
  `update` preserves the remaining TTL (PTTL then `SET ... PX`).

Not ported: key namespacing (GoFrame uses raw keys too), `MGET`/`MSET` batch
round trips (the port loops), Lua-atomic `set_if_not_exist_func`, and Redis
Cluster routing for the cache adapter (it takes a plain `Client`).

Consequences: three tests run against an in-process fake RESP server:
round trip incl. TTL/update/keys/data/remove/NX/clear, TTL expiry, and
get-or-set-function caching.
## ADR-175: `gf-database` provides the Redis session storage

Status: accepted (2026-11-25)

Context: `MAP.md` recorded `os/gsession` without the Redis storage. Like the
cache adapter (ADR-174) it needs `gredis` while `gf-os` cannot depend on
`gf-database`.

Decision: `gf_database::gsession_redis::{RedisStorage, new_storage_redis}`
implements `gf_os::gsession::Storage` with one Redis string per session
(`prefix + session_id`, JSON map):

- only the session-level hooks are overridden (`new_session`, `get_session`,
  `set_session`, `update_ttl`, `remove_all`); per-key `get`/`set`/`remove`
  keep the trait's "disabled" default so `Session` keeps its in-memory map
  and persists the whole map on close, mirroring GoFrame's `StorageBase`;
- `set_session` writes `SET key json PX ttl` and deletes the key for an empty
  map; `update_ttl` issues `PEXPIRE` immediately (GoFrame batches it through a
  timer);
- `remove_all` is `DEL`, `get_session` is `GET` + JSON.

Not ported: the batched TTL-refresh timer, `StorageRedis`'s `updatingIdMap`,
Lua-atomic session updates, and Redis Cluster routing.

Consequences: two tests run against an in-process fake RESP server: the
round trip (prefix key, restore, remove key, remove all) and the TTL refresh
path.
## ADR-176: `gclient` gains OPTIONS/TRACE/CONNECT

Status: accepted (2026-11-25)

Context: GoFrame's client exposes `Options`, `Trace` and `Connect`; the port
only had GET/POST/PUT/DELETE/HEAD/PATCH.

Decision: add `Client::{options, trace, connect}` delegating to the existing
`request` builder with the corresponding method string (no body). The
`ghttp` test server can route and answer these methods, so the port's
client/server pair round-trips them.

Not ported: CONNECT tunnelling (the client sends the request but does not
switch the connection into a transparent tunnel), TRACE echo semantics on the
server, and proxy CONNECT handling.

Consequences: one test binds OPTIONS/TRACE/CONNECT routes on the test server
and asserts each client method reaches its handler.
## ADR-177: `gtime` caches Go-layout tokenization

Status: accepted (2026-11-25)

Context: HANDOFF listed "36x slower" `gtime::format` as optional work because
every call re-scanned the layout string with `match_token` (allocating the
char vector and re-tokenizing each time).

Decision: `layout::layout_segments` tokenizes a layout once into
`LayoutSegment { start, token }` and stores it in a process-wide
`OnceLock<Mutex<HashMap<String, Arc<Vec<LayoutSegment>>>>>`;
`format_go_layout` reuses the cached segments (the cache is bounded only by
the number of distinct layouts, typically a handful).

Not ported: an eviction policy for unbounded dynamic layouts, caching
`parse_go_layout` similarly, and caching `php_go_layout` translations.

Consequences: one layout test formats the same layout eleven times and
asserts stable output; the workspace suite stays green.
## ADR-178: `gdb` Model gains `FieldsPrefix`

Status: accepted (2026-11-25)

Context: GoFrame's Model exposes `FieldsPrefix(prefix, fields)` to qualify a
comma-separated field list with a table alias; the port only had raw `fields`.

Decision: `Model::fields_prefix(prefix, fields)` splits the list on commas,
trims entries, drops empty ones and joins `prefix.field` back into the
builder's projection. Callers pair it with `as_alias` for joined queries.

Not ported: `FieldsExPrefix` (exclude-by-prefix), alias validation against the
joined tables, and automatic prefixing of `*`.

Consequences: one test selects `u.name, u.age` through the `u` alias and
checks both values.
## ADR-179: remaining `ghttp` infrastructure gaps are recorded, not ported

Status: accepted (2026-11-25)

Context: the round-118 HANDOFF snapshot lists the last `net/ghttp` items:
graceful restart / fd passing, HTTP/2 and ALPN, per-domain routing and the
admin/Swagger APIs. Later ADRs covered TLS, hooks, static files, sessions,
cookies, OpenAPI/Swagger UI and header limits; this ADR consolidates what is
still intentionally unported so the remaining list is fully recorded.

Decision: the following stay not ported:

- graceful restart/fd passing (`SO_REUSEPORT`/socket hand-off): the port's
  `Server::shutdown(timeout)` already drains in-flight requests, but a
  zero-downtime binary restart is a deployment concern outside the library
  and is hostile on Windows;
- HTTP/2 and ALPN: `ghttp`/`gclient` speak HTTP/1.1 only (no HPACK, frames,
  server push or stream multiplexing); adding them needs a protocol stack
  crate and a server-loop rewrite;
- per-domain routing: `Router` is a flat table without host matching
  (ADR-067);
- the GoFrame admin API (`EnableAdmin`, pprof, config dumps) and the
  generated Swagger UI bundle (the port serves a supplied document with a
  CDN UI, ADR-080).

All of these are recorded in ADR-067/ADRs above and in MAP rows; no future
work is planned unless the deployment needs one of them.

Consequences: `HANDOFF.md`'s remaining list now maps one-to-one onto ADRs
(052, 056, 057, 047/108, 076/077, 067/080/179), so the port is complete for
every portable unit.
Update: per-domain routing moved from this list to ADR-180 (implemented);
the admin endpoint base moved to ADR-185 (implemented, without pprof or
process re-exec).

## ADR-180: `ghttp` supports per-domain routes

Status: accepted (2026-11-25)

Context: ADR-067 recorded per-domain routing as not ported. GoFrame's pattern
grammar puts the domain after an `@` (`GET:/who@api.example.com`) and gives
host-specific routes priority over the `default` domain's routes.

Decision:

- `split_domain(path)` recognises a trailing `@domain` (letters, digits, `.`,
  `-`, `_`), lower-cases it and returns `default` otherwise;
- `Route.domain` participates in matching: a non-default route only matches a
  request whose `Host` (port already stripped by `Request::host`) equals it,
  case-insensitively;
- domain-specific routes gain one score point so they win ties against the
  same path in the `default` domain, while static segments still dominate;
- `Server::serve` passes `request.host()` into `Router::lookup`;
  `Router::route_list` renders the `@domain` suffix back.

Not ported: wildcard domains (`*.example.com`), default-domain fallback when a
host-specific route exists but does not match the method, and per-domain
middleware lists (route/group middlewares still apply per registration).

Consequences: one router test and one server test cover host-specific vs
default selection, case-insensitive hosts and the route listing.
## ADR-181: `gproc` signal handling with a Rust platform layer

Status: accepted (2026-11-25)

Context: ADR-057 recorded signal handling as not ported because `std` has no
cross-thread signal API. GoFrame's `gproc` exposes `AddSigHandler`,
`AddSigHandlerShutdown` and a blocking `Listen` over `os/signal`.

Decision: add a thin platform layer and a portable registry:

- `Signal { Interrupt, Quit, Terminate, Abort, Hangup, User1, User2, Kill }`
  with Go-style names; `SigHandler = Arc<dyn Fn(Signal) + Send + Sync>`;
- `add_sig_handler(handler, &[Signal])` and `add_sig_handler_shutdown(&[..])`
  fill a global registry; shutdown signals are `SIGINT/QUIT/TERM/ABRT/KILL`;
- `listen()` installs the OS listeners once and blocks on a condvar until a
  shutdown signal is handled (multiple callers share the loop);
- Unix uses `signal-hook` (`SIGINT/QUIT/TERM/ABRT/HUP/USR1/USR2`), Windows
  uses `ctrlc` (Ctrl-C/Ctrl-Break mapped to `Interrupt`); other platforms
  have no OS listener but still support `trigger`;
- `trigger(signal)` dispatches manually (tests/embedding): handlers run on
  their own threads and shutdown handlers are joined before `listen` returns.

Not ported: `SIGKILL` delivery (uncatchable, kept for parity), Go's
`signal.Notify` re-arming after handlers are added later, goroutine-per-handler
scheduling equivalence beyond detached threads, and signal masks.

Consequences: one test covers handler dispatch, unregistered signals, the
shutdown flag and (on Unix) a real `listen` loop ended by `trigger`.
## ADR-182: `gctx` is an explicit trace-context value

Status: accepted (2026-11-25)

Context: ADR-052 skipped `os/gctx` because Go's `context.Context` has no Rust
counterpart. GoFrame only uses it to carry a trace/span id (plus
`context.WithValue` payloads) and to seed an init context from the
environment's OpenTelemetry propagator.

Decision: port the observable behaviour as an explicit value type:

- `gctx::Context { trace_id, span_id, span_name, values }` with
  `new()` (32-hex trace + 16-hex span), `background()`, `with_span(name)`
  (returns the receiver when it already has a trace id, like Go),
  `with_value`/`value` (the `context.WithValue` equivalent), `trace_id`,
  `span_id`, `span_name`;
- W3C `traceparent` formatting/parsing (`00-<32hex>-<16hex>-01`), with invalid
  headers rejected;
- a process-wide init context seeded from the `TRACEPARENT` environment
  variable with `set_init_ctx`/`get_init_ctx`, like `SetInitCtx`/`GetInitCtx`.

Not ported: implicit goroutine-local propagation (callers pass `&Context`),
OpenTelemetry's global propagator/exporter, real span creation/ending and
baggage.

Consequences: three tests cover id generation and `with_span` idempotence,
background values, and the init context plus `traceparent` round trip.
## ADR-183: `gstructs` is a derive-macro metadata equivalent

Status: accepted (2026-11-25)

Context: ADR-052 skipped `os/gstructs` because Go reads struct fields and tags
through runtime reflection. Rust has no reflection, so a direct port is
impossible; the equivalent is compile-time metadata.

Decision: add two crates:

- `gf-structs` (runtime): `FieldMeta { name, type_name, optional, tags }`,
  the `StructMeta` trait (`NAME`, `FIELDS`) and helpers `fields`,
  `field_map(priority)`, `tag_map_name(priority)`, `field_by_tag`; tag names
  resolve by a Go-style priority list (`gconv` before `json`, ...), falling
  back to the Rust field name, and take the value before `,` like GoFrame;
- `gf-structs-derive`: `#[derive(StructMeta)]` emitting the static metadata;
  Go-style tags are written as `#[gf(tag = "json:\"name,omitempty\"")]` and
  parsed at compile time; `Option<T>` fields are marked `optional`.

Not ported: runtime lookup of arbitrary/`dyn Any` values, embedded-struct and
pointer traversal, `FieldsInput`/`FieldMapInput` option structs, struct-tag
ordering beyond the caller's priority list, and mutable field metadata.

Consequences: one `gf-structs` unit test covers the trait defaults and one
integration test derives `User` and checks fields, tag priority, the field
maps, the name map and tag lookup.
## ADR-184: `gmetric` gains an in-memory provider

Status: accepted (2026-11-25)

Context: ADR-056 kept the OpenTelemetry-shaped API with no backend, so every
instrument was a no-op unless a provider was installed. GoFrame's real
backend is the external OTel SDK; a self-contained equivalent makes the
package usable and testable without an exporter.

Decision: add a memory backend behind the existing `Provider`/`MeterPerformer`
traits:

- `MemoryCounter`, `MemoryUpDownCounter`, `MemoryHistogram` performers with
  `Mutex` accumulators and `value()`/`snapshot()` accessors;
- `MemoryProvider` registers each instrument by name and can install itself
  (`install()` / `Provider::set_as_global`) through a `Weak` self-reference;
- `counter_value`, `up_down_counter_value`, `histogram`, and a `MemorySnapshot`
  with counter/up-down/histogram maps; `HistogramSnapshot` exposes
  count/sum/min/max/values.

Not ported: real OTel exporters/providers, delta vs cumulative temporality,
attribute-level aggregation, histogram bucket boundaries (values are stored
raw), and asynchronous observable instruments (they stay callback-driven).

Consequences: one test installs the provider, measures a counter, up-down
counter and histogram through the public meter API, and checks the snapshot;
the existing provider test now shares a mutex with it because both mutate the
global provider.
## ADR-185: `ghttp` admin endpoints are explicit routes

Status: accepted (2026-11-25)

Context: ADR-067/179 recorded the GoFrame admin API as not ported. GoFrame's
`EnableAdmin` binds an object whose methods become `Index`, `Restart` and
`Shutdown` routes; the port has no object-method binding, so the equivalent is
explicit route registration.

Decision: `Server::enable_admin(pattern)` binds, under
`pattern` (default `/debug/admin`):

- `GET {base}`: an HTML page with the pid, current executable path and
  restart/shutdown links;
- `GET {base}/restart`: 501 with an explanation (replacing the running process
  is platform-specific and unsafe to automate);
- `GET {base}/shutdown`: answers `server shutdown`, then closes the accept
  loop (`Server::close`).

Not ported: `RestartAllServer`/process re-exec, the pprof endpoints, object
method binding, and the one-second delayed shutdown (the port closes right
after the handler instead). GoFrame's config-dump/admin pages beyond this index
are not ported either.

Consequences: two tests cover the default and a custom base path, including
the 501 restart answer and the closed flag after shutdown.
## ADR-186: `gcmd` object methods become subcommands through macros

Status: accepted (2026-11-25)

Context: ADR-047/108 recorded GoFrame's struct-object lane (reflecting the
methods of an object into subcommands) as not ported because Rust has no
reflection. The equivalent is compile-time generation: an attribute macro can
read an `impl` block, which a derive macro cannot.

Decision:

- `gcmd::CommandObject` gains `sub_commands(&self)` (default empty);
  `gcmd::new_from_object_with(object)` registers them and uses the instance
  for the root action, mirroring `NewFromObject(object)`;
- a new `gcmd::CommandInput` trait describes one subcommand's input:
  `command_meta()` (its arguments) and `from_parser()`;
- `#[derive(CommandInput)]` builds that trait from a struct's `#[cmd(...)]`
  fields (same keys as `CommandObject`: `rename`, `short`, `brief`, `default`,
  `arg`, `orphan`, `skip`);
- `#[command_object(name = ..., usage = ..., brief = ..., description = ...,
  examples = ...)]` on `impl Root { ... }` generates `CommandObject` for
  `Root` and one subcommand per method: the Rust method name (snake_case) is
  the command name (`#[cmd(name = "...")]` overrides it), its single input
  type supplies the arguments, and the generated runner rebuilds the root via
  `from_parser` then calls the method. `Root` must implement `Default`.

Not ported: Go method-name casing rules (`Env` → `env` is manual here),
reflection over arbitrary objects, method overloading, context parameters,
and output structs (methods return `Result<(), Error>`).

Consequences: one test derives `Root` with two methods, checks the registered
subcommand names and runs `test -n=john -v` through the public command API.
## ADR-187: `gdb` gains a backend trait and a PostgreSQL driver

Status: accepted (2026-11-25)

Context: ADR-076/077 recorded MySQL/PostgreSQL drivers as not ported; `Db`
wrapped `rusqlite::Connection` directly, so a second driver needed a seam.

Decision:

- `gdb_exec::Backend` (crate-private, `Send`) exposes `execute`, `query`,
  `insert`, `in_transaction`, `sqlite_connection`; `Db` holds
  `Box<dyn Backend>` plus its `Dialect`;
- `SqliteBackend` keeps the previous rusqlite behaviour (values, rows,
  `last_insert_rowid`);
- `PostgresBackend` uses the pure-Rust `postgres` crate over `NoTls` with a
  `Mutex<Client>`: libpq-style DSN parsing, parameters mapped to
  `Box<dyn ToSql>` (maps/lists JSON-encoded), rows mapped by PostgreSQL type
  name (bool/int2/int4/int8/float4/float8/bytea and text-like; other types
  fall back to text or NULL);
- `Db::open_postgres(dsn)` selects `Dialect::Postgres`; `DbPool` gains
  `open_postgres`/`open_postgres_with_size`;
- `table_fields` reads `information_schema` for PostgreSQL and keeps
  `PRAGMA table_info` for SQLite.

Not ported: MySQL driver, TLS/`sslmode` connections (`NoTls` only), prepared
statement caching, `RETURNING`-based insert ids (PostgreSQL `insert` returns
`None`), numeric/decimal/JSON/timestamp decoding (delivered as text or NULL),
and separate transaction state tracking (`in_transaction` is false for
PostgreSQL).

Consequences: one unit test checks that a refused PostgreSQL connection
surfaces a driver error; an ignored `postgres_live_round_trip` test runs the
full create/insert/query/table-fields/drop cycle when `GF_POSTGRES_DSN` is
set. The SQLite suite stays green.
## ADR-188: `ghttp` supports SO_REUSEPORT for zero-downtime restarts

Status: accepted (2026-11-25)

Context: ADR-179 recorded graceful restart/fd passing as not ported. Full
fd-passing hand-off is platform-specific; the portable half is the
`SO_REUSEPORT` socket option that lets a replacement process bind the same
address while the old process stops accepting and drains in-flight requests
(`Server::shutdown`).

Decision:

- `Server::set_reuse_port(bool)` / `reuse_port()` store the flag;
- `run` binds through `bind_listener`: on Unix with the flag it uses
  `socket2` to create the socket, set `SO_REUSEADDR` + `SO_REUSEPORT`, bind,
  listen (backlog 1024) and become non-blocking; otherwise it uses
  `TcpListener::bind`;
- on non-Unix platforms the flag is ignored (Windows has no `SO_REUSEPORT`),
  so behavior stays the previous bind.

Not ported: fd passing (`SCM_RIGHTS`/`ResumeSocket`), the GoFrame
`RestartAllServer` process spawner, per-server graceful restart coordination,
and Windows equivalents (there is none).

Consequences: a Unix-only test starts two servers on the same port with the
flag and asserts the second binds the same port; on Windows the test is a
no-op.
## ADR-189: `gdb` gains a MySQL backend

Status: accepted (2026-11-25)

Context: ADR-187 left MySQL as the last missing `gdb` driver after the
`Backend` seam and the PostgreSQL backend landed.

Decision: `MysqlBackend` uses the pure-Rust `mysql` crate (sync `Pool`):

- `Db::open_mysql(dsn)` accepts either a `mysql://user:pass@host:port/db` URL
  or GoFrame's `user:pass@tcp(host:port)/db` form (parsed into `OptsBuilder`;
  query parameters after `?` are dropped);
- parameters map to `mysql::Value` (maps/lists JSON-encoded); rows map back by
  value kind (bytes become UTF-8 text when valid, `Date`/`Time` become
  formatted strings);
- `execute` returns `affected_rows` and `last_insert_id`; `query` collects
  `mysql::Row`s; `insert` reuses `execute`;
- `table_fields` reads `information_schema.columns` with `?`;
  `DbPool::open_mysql`/`open_mysql_with_size` mirror the other backends.

Not ported: TLS options (`SslOpts`), named parameters, server-side prepared
statement reuse, multi-statement mode, transaction-state tracking
(`in_transaction` is false) and the MySQL cluster/sharding lanes.

Consequences: two unit tests cover DSN parsing (Go and URL forms) and a
refused connection error; an ignored `mysql_live_round_trip` runs the full
create/insert/query/table-fields/drop cycle when `GF_MYSQL_DSN` is set.
## ADR-190: HTTP/2, ALPN and fd passing are not ported (effort decision)

Status: accepted (2026-11-25)

Context: the last `net/ghttp` gaps are HTTP/2 with ALPN, and fd-passing
graceful restart (ADR-179/188). This ADR records the effort decision the
goal asked for.

HTTP/2 + ALPN:

- the port is a synchronous HTTP/1.1 stack (`std::net` + one request per
  connection, keep-alive, rustls without ALPN); HTTP/2 needs an async runtime,
  a frame/HPACK implementation, stream multiplexing, flow control, priorities
  and a TLS ALPN handshake. `h2`/`hyper` provide that but require rewriting
  both `Server::run`/`serve_stream*` and `gclient` around tokio, plus new
  tests that drive a real h2 client;
- estimated size: 1-2k lines of changed/added code and a new async dependency
  surface, with `Model`/hooks/session/static/openapi behaviour re-verified on
  the new loop;
- decision: **not ported in-tree**. Deployments that need h2 terminate it at a
  reverse proxy (nginx/caddy) in front of the HTTP/1.1 port; the Rust port
  keeps advertising no ALPN protocol.

fd passing:

- seamless hand-off needs `SCM_RIGHTS`/`sendmsg`/`recvmsg` and inherited
  listeners, i.e. platform `unsafe` plus Unix-only code; the repository rule
  is no `unsafe` outside `genv` (ADR conventions). `SO_REUSEPORT` restarts
  (ADR-188) already give zero-downtime replacement on Unix without unsafe;
- decision: **not ported**; the documented restart recipe is "start the new
  process with `set_reuse_port(true)`, then `shutdown` the old one".

Not ported: HTTP/2 server and client, ALPN protocol negotiation, server push,
fd passing/`ResumeSocket`, Windows restart equivalents.

Consequences: HANDOFF's remaining list now consists only of external-runtime
concerns (an OTel exporter and optional profiling work) and these recorded
non-ports. No further HTTP/2 work is planned unless a deployment needs it,
in which case the reverse-proxy route is the recommended equivalent.