# Go vs Rust benchmarks

Machine: Windows, 12 logical cores, `rustc 1.99.1` (release, default profile) and
`go1.25.0 windows/amd64`. Method: identical workloads in `crates/gf-bench`
(Rust) and `bench/go` (Go; module `github.com/gogf/gf/v2/zzbench` with a
`replace` to the reference checkout so `internal/` packages can be imported).
Each benchmark warms up, then runs five timed rounds; the tables report the best
round in nanoseconds per operation. Absolute numbers move with machine load, so
compare the ratio column, which is stable.

Run:

```powershell
cargo run --release -p gf-bench
cd bench/go; go run .
```

## Results (one paired run per side)

| Benchmark | Go ns/op | Rust ns/op | Rust vs Go |
| --- | ---: | ---: | --- |
| `grand` — Intn(1000) | 82.4 | **8.6** | **9.6x faster** |
| `gregex` — match `(.+):(\d+)` | 311 | **75** | **4.1x faster** |
| `empty` — 4 emptiness checks | 12.1 | **3.1** | **3.9x faster** |
| `ghtml` — StripTags | 544 | **146** | **3.7x faster** |
| `gtoml` — decode | 7_240 | **2_722** | **2.7x faster** |
| `json` — marshal 8-key map | 1_924 | **736** | **2.6x faster** |
| `command` — parse 7 arguments | 1_780 | **775** | **2.3x faster** |
| `gcrc32` — 1 KiB | 105 | **48.5** | **2.2x faster** |
| `gaes` — CBC encrypt | 530 | **248** | **2.1x faster** |
| `base64` — encode 1 KiB | 1_253 | **656** | **1.9x faster** |
| `gcharset` — GBK to UTF-8 | 2_972 | **203** | **14.6x faster** |
| `gqueue` — push + pop | 750 | **51.3** | **14.6x faster** |
| `gring` — put + slice | 323 | **137** | **2.4x faster** |
| `garray` — append + At | 135 | **76.1** | **1.8x faster** |
| `gset` — add + contains | 222 | **208** | 1.1x faster |
| `glist` — push + pop | 74.5 | 73.8 | tie |
| `gvar` — `Int()` of a new variable | 34.0 | **21.4** | **1.6x faster** |
| `gmap` — set + get | 326 | **249** | **1.3x faster** |
| `gdebug` — caller info | 1_708 | 332_617 | 195x slower (backtrace symbolization) |
| `gstr` — case-insensitive contains | 90.3 | **30.0** | **3.0x faster** |
| `gstr` — CaseSnake | 1_681 | **665** | **2.5x faster** |
| `gstr` — similar_text | 82.7 | **41.9** | **2.0x faster** |
| `gstr` — WordWrap | 414 | **254** | **1.6x faster** |
| `gstr` — UcWords | 791 | **260** | **3.0x faster** |
| `gstr` — CompareVersion | 451 | 526 | 1.2x slower |
| `gstr` — NumberFormat | 395 | 377 | tie |
| `gstr` — IsSubDomain | 124 | 159 | 1.3x slower |
| `gstr` — split + join | 154 | 476 | 3.1x slower |
| `gstr` — Parse `a[b]=c` pairs | 744 | 1_754 | 2.4x slower |
| `gzip` — 1 KiB | 111_684 | **27_214** | **4.1x faster** |
| `gjson` — parse a 2-user document | 5_549 | **2_044** | **2.7x faster** |
| `gxml` — decode | 6_489 | **3_216** | **2.0x faster** |
| `gconv` — string to i64 | 24.5 | **12.5** | **2.0x faster** |
| `gxml` — encode | 1_223 | **803** | **1.5x faster** |
| `gjson` — dotted path get | 227 | **171** | **1.3x faster** |
| `gunzip` — 1 KiB | 10_045 | **7_973** | **1.3x faster** |
| `gipv4` — IsIntranet | 27.5 | **14.9** | **1.85x faster** |
| `gproperties` — decode 3 keys | 9_311 | **5_247** | **1.8x faster** |
| `gaes` — CBC decrypt | 412 | **234** | **1.76x faster** |
| `gyaml` — decode the Go fixture | 19_117 | **10_929** | **1.75x faster** |
| `gerror` — render a 3-level chain | 129 | **75.6** | **1.7x faster** |
| `json` — unmarshal 8-key map | 3_354 | **2_082** | **1.6x faster** |
| `utils` — strings | 492 | **324** | **1.5x faster** |
| `gurl` — parse the test URL | 603 | **421** | **1.43x faster** |
| `gurl` — encode the test URL | 361 | **257** | **1.4x faster** |
| `sha256` — 1 KiB | 2_903 | 2_782 | tie |
| `gbinary` — encode Int/UInt/String | 118 | **128** | tie |
| `ghash` — 16 hashes over 1 KiB | 17_143 | 17_740 | tie |
| `md5` — 1 KiB | 1_466 | 1_960 | 1.3x slower |
| `ghtml` — escape five characters | 120 | 196 | 1.6x slower |
| `gerror` — build a 3-level chain | 2_572 | 4_064 | 1.6x slower |
| `gerror` — `Newf(...)` + render | 698 | 1_808 | 2.6x slower |
| `gerror` — one `New` | 472 | 1_294 | 2.7x slower |

## Results — containers and the base layer (second paired run)

Same machine, same method, run after the `gtype`, sorted-array, `gtree`,
`gmap` list/tree, `gpage`, `guid` and `genv` units landed. Absolute numbers for
the earlier rows moved a few percent between runs; the rows below are from this
run only.

| Benchmark | Go ns/op | Rust ns/op | Rust vs Go |
| --- | ---: | ---: | --- |
| `garray` — sorted int add + search | 258.5 | **159.6** | **1.6x faster** |
| `gmap` — list set + get (1 entry) | 268.9 | **197.4** | **1.4x faster** |
| `gtype` — `Any` set + val | 63.6 | **51.0** | **1.25x faster** |
| `gtype` — `Int` set + add + cas + val | 19.1 | **18.0** | **1.06x faster** |
| `guid` — `S()` | 495.1 | 509.9 | 1.03x slower |
| `gpage` — `PageBar()` | 5_716 | 6_750 | 1.18x slower |
| `gtree` — set + get | 203.4 | 392.6 | 1.9x slower |
| `gmap` — tree set + get | 203.3 | 400.9 | 2.0x slower |
| `gtime` — `StrToTime("2018-07-18 12:01:02")` | 944.0 | 1_816.5 | 1.9x slower |
| `genv` — `MapFromEnv` (2 entries) | 176.4 | 363.5 | 2.1x slower |
| `gtree` — iterate 100 entries | 594.1 | 2_838.6 | 4.8x slower |
| `gmap` — list iterate 100 entries | 359.4 | 6_235.3 | 17x slower |
| `gtime` — `Format("Y-m-d H:i:s")` | 601.8 | 21_560 | 36x slower |

Why the tree and time numbers trail:

- Every tree operation goes through a comparator `Arc`, a comparator-ordered
  key wrapper and a poison-tolerant `RwLock`; Go's red-black tree compares
  through a function field without a lock when `safe` is false.
- Iteration in every container snapshots the entries so callbacks never run
  under the lock (ADR-007). For a Go list map (live linked-list walk) that is
  the 17x cost on 100 entries; for the red-black/B-tree map it is the 4.8x.
- `gmap`'s list map is a vector of pairs: constant-time for one entry, linear
  for many, where Go's list map hashes. The benchmark measures one entry.
- `MapFromEnv` validates every entry and returns `Result` where Go panics, and
  builds an insertion-ordered map.
- `gtime::format` translates the PHP-style format on every call and then asks
  `chrono` to render it; Go translates to a Go layout and `time.Format` walks
  the layout directly. `StrToTime` runs the three Go regexes and then a layout
  list, as Go does, but builds a `chrono` value from the parts.
- `PageBar` escapes and formats every link, like Go, but the escaping is a
  character scan.

### Where the error gap comes from

| Component | Go ns/op | Rust ns/op |
| --- | ---: | ---: |
| stack capture (`runtime.Callers` / `Backtrace::force_capture`) | 262 | 1_026 |
| one `gerror.New`, stack captured | 472 | 1_294 |
| one `gerror.New`, stack **not** captured (Rust only) | — | **58** |

Rust's error machinery is cheap; capture is ~96% of `Error::new`. Go records 64
program counters while `std::backtrace` walks the whole native stack and stores
per-frame metadata. `Error` stays at 128 bytes or less (boxed detail and args).

## What the wins come from

- **grand**: a per-thread 1 KiB buffer filled from the system CSPRNG replaces
  Go's shared channel of 4-byte buffers; no lock or channel per draw.
- **gregex**: compiled patterns are cached and the engine is a lazy DFA.
- **ghtml**: a byte scanner instead of `html/template`'s tokenizer state machine.
- **JSON / TOML / YAML / properties**: direct serialization or a single-pass
  parser, without intermediate trees or reflection.
- **crypto**: `ring` (SHA-NI) for hashes, `crc32fast` (SSE4.2) for CRC32, and
  RustCrypto AES/DES with manual CBC loops; all match the Go test vectors.
- **gcharset**: `encoding_rs` looks the encoding up in a table and decodes in
  one pass; Go's `x/text` goes through `transform.Reader`, which dominates.
- **gconv / gjson**: enum matches and a single `serde_json` pass; `Json::get`
  walks the `Value` without allocating until the leaf is cloned.
- **containers**: one generic type per shape over `RwLock<IndexMap|Vec|VecDeque>`;
  `gqueue` uses condition variables instead of Go's goroutine-pumped channel
  (14x), and `gring` avoids linked nodes (2.4x).
- **gxml / gzip**: `quick-xml` and `flate2` stream without Go's interface
  indirection; both were validated against the Go output.
- **base64 / gbinary / gurl**: write into one buffer with reserved capacity;
  parsing borrows the input where possible.
- **command / empty / gipv4**: hand-written scanners and enum matches instead of
  regex/reflection/netip.

## Caveats

- These measure the port's current surface (base layer), not an end-to-end app.
- Rust uses the default release profile (no LTO).
- MD5 trails Go's assembly implementation (`md-5` has no accelerated path).
- `ghtml::entities` and `gproperties` escaping lose to Go's prebuilt replacers;
  they are cold paths compared with parsing.
- `gstr::split` returns owned pieces (Go returns substrings sharing the input),
  which is why split+join measures 3x slower; `pos_i`/`contains_i` use a
  zero-allocation ASCII fast path and beat Go. `gstr::parse` builds nested
  `Value` maps with owned keys, which costs more than Go's `map[string]any`
  inserts.
- `gerror.Stack()` content differs by nature (native symbols vs `FuncForPC`).
- `gdebug::caller` symbolizes the captured stack, which costs ~0.33 ms per call
  on this machine against Go's 1.7 µs `runtime.Caller`; it is debugging API and
  should stay out of hot paths.
