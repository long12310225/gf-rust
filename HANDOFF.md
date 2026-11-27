# Handoff

## Where the port stands (2026-11-24)

- Workspace in `D:\github\gf-rust`; Go source read-only at `D:\github\gf`
  (`github.com/gogf/gf/v2`, pinned at `1020b0afa19312373f6d7448768cdb5aa77a980d`,
  `v2.10.5-22-g1020b0afa`).
- Crates: `gf-core`, `gf-internal`, `gf-errors`, `gf-encoding`, `gf-json`,
  `gf-crypto`, `gf-text`, `gf-util`, `gf-valid`, `gf-container`, `gf-net`,
  `gf-debug`, `gf-os`, `gf` (facade), `gf-bench` (Go vs Rust benchmarks).
- Ported and gated by translated Go tests (1079 tests):
  `version.go`, `internal/consts`, `internal/command`, `internal/errors`,
  `internal/empty`, `internal/utils`, `internal/fileinfo`, `internal/deepcopy`,
  `internal/json`, `internal/intlog`, `errors/gcode`, `errors/gerror` (with the
  `gfmt` formatter), `encoding/ghash`, `encoding/gbase64`, `encoding/gurl`,
  `encoding/ghtml`, `encoding/gtoml`, `encoding/gyaml`, `encoding/gbinary`,
  `encoding/gproperties`, `encoding/gcharset`, `encoding/gxml`,
  `encoding/gcompress` (gzip/zlib/zip), `encoding/gini`, `encoding/gjson`,
  `crypto/gmd5`, `crypto/gsha1`, `crypto/gsha256`, `crypto/gsha512`,
  `crypto/gcrc32`, `crypto/gaes`, `crypto/gdes`, `crypto/grsa`,
  `debug/gdebug`, `container/gvar`, `container/gmap` (hash core, list and tree
  maps, `KVMap` extras), `container/garray` (core and sorted variants),
  `container/gset`, `container/glist`, `container/gring`, `container/gqueue`,
  `container/gpool`, `container/gtype` (all 13 wrappers),
  `container/gtree` (AVL, red-black, B-tree and KV forms, `SetComparator`,
  `PopLeft`/`PopRight` and JSON bridges), `text/gregex`, `text/gstr` (complete),
  `util/gtag`, `util/grand`, `util/gmode`, `util/gconv`, `util/gutil`
  (comparators and the possible-key lookup), `util/gvalid` (all 67 builtin
  rules, value/map/tag-sequence lanes), `util/gpage`, `util/guid`, `net/gipv4`,
  `net/gipv6`, `os/gtime`, `os/genv`, `os/gbuild`, `os/gmutex`, `os/gfile`
  (everything but `MainPkgPath`; the cache lane runs on an inline TTL map),
  `os/gspath` (direct search lane), `os/gfsnotify` (on the `notify` crate,
  ADR-042), `os/gres` (`PackToGoFile` not ported, ADR-044), `os/gcfg`
  (context-free, ADR-045), `os/glog` (context-free, no `slog` bridge,
  ADR-046), `os/gcmd` (context-free; derive object lane, ADR-047/108),
  `os/gcache` (`Ttl`/`Key`, Redis adapter in `gf-database::gcache_redis` (ADR-174), ADR-048), `os/gmlock` (RAII
  guards, ADR-049), `os/grpool` (thread supervisor, ADR-050), `os/gfpool`
  (`FileOptions`/`Ttl`, ADR-051), `os/gtimer` (thread-local `exit`,
  ADR-053), `os/gcron` (`chrono` dates, ADR-054), `os/gsession` (no Redis/
  no file crypto, ADR-055), `os/gmetric` (API surface, ADR-056), `os/gproc`
  (shell lane, ADR-057), `os/gview` (own template subset engine, ADR-058),
  `net/gtrace` (context-free subset, ADR-059), `net/gudp` (UDP conns;
  relative deadlines, ADR-060), `net/gtcp` (conns + package protocol + server;
  no TLS/pool, ADR-061), `net/gsvc` + `net/gsel` (context-free service
  interfaces and selectors, ADR-062), `net/ghttp/internal/response` (own
  writer trait, ADR-063), `gtcp` connection pool (typed pool, ADR-064),
  `net/goai` (OpenAPI model + Value schema builder, ADR-065), `ghttp`
  HTTP/1.1 message model (ADR-066), `ghttp` router + server (ADR-067),
  static file serving (ADR-068), `net/gclient` HTTP/1.1 client (ADR-069),
  `database/gredis` RESP2 client (ADR-070), `gdb` value layer (ADR-071), `gdb` SQL builder (ADR-072), `ghttp` keep-alive/status/status
  handlers (ADR-073), `ghttp` cookies + session wiring (ADR-074), `frame/g` facade (ADR-075), `gdb` SQLite execution (ADR-076), `gdb` Model (ADR-077), `i18n/gi18n` (ADR-078), `gdb` transactions (ADR-079), `ghttp` OpenAPI serving (ADR-080), `frame/gins` container (ADR-081), `gclient` redirects (ADR-082) and HTTPS via rustls (ADR-083); `ghttp` HTTPS serving via rustls (ADR-084), `goai` manual registration (ADR-085), `gtcp` TLS client and server (ADR-086/087), `gview` i18n result translation (ADR-088), `test/gtest` assertions (ADR-089), `ghttp` config-map subset (ADR-090), `gspath` cache lane (ADR-091), `gvalid` derive struct lane (ADR-092), `gregex` trailing capture groups (ADR-093), `garray` sorted pop parity (ADR-094), `gf-json` Go float text (ADR-095), `gdb` SQLite pool (ADR-096), `gredis` pipelines/transactions (ADR-097) and pub/sub subscriptions (ADR-098), scripting EVAL/SCRIPT (ADR-099), derive struct scanning (ADR-100), gi18n packed-resource lane (ADR-101), `ghttp` CORS (ADR-102) and gzip compression (ADR-103), JSON-body/handler-response middleware (ADR-104), W3C tracing middleware (ADR-105), gview define/template/block (ADR-106) and std template builtins (ADR-107), `gcmd` derive object lane (ADR-108), `gdb` table-field introspection (ADR-109), time fields/soft delete (ADR-110), `gredis` RESP3 replies and `HELLO` (ADR-111), `ghttp` group verb helpers (ADR-112), static-file validators/304 (ADR-113), `gredis` Sentinel master resolution (ADR-114), `gi18n` auto reload (ADR-115), `gredis` cluster slot routing (ADR-116), `gdb` model result cache (ADR-117), `ghttp` request client-IP/url helpers (ADR-118), `gview` variable assignment (ADR-119), `gredis` connection pool (ADR-120), `gdb` model aggregates (ADR-121), struct writes via `#[derive(ToRecord)]` (ADR-122), paged `Model::chunk` (ADR-123), `ghttp` redirects (ADR-124), `ClientMaxBodySize` (ADR-125) with config-map wiring (ADR-126), global server hooks (ADR-127), `gredis` ACL usernames (ADR-128), pattern-scoped `ghttp` hooks (ADR-129), `gdb` table sharding (ADR-130), `gstr` case-conversion test tables (ADR-131) and trim/prefix/nl2br helpers (ADR-132), `gutil` map/slice helpers (ADR-133), `garray` mutators (ADR-134), `glist` move/peek helpers (ADR-135), `gset` copy/iteration helpers (ADR-136), `gtree` ceiling/floor/pop/flip coverage (ADR-137), `Accept-Language` negotiation (ADR-138), `gredis` cluster replica routing (ADR-139), `gproc` process manager (ADR-141), `gdb` NOT/OR condition helpers (ADR-142), `ghttp` route listing (ADR-143), session cookie config (ADR-144), `gview` break/continue (ADR-145), `glog` cat/skip (ADR-146), `gdb` joins (ADR-147), response write/buffer helpers (ADR-148), `gdb` insert-ignore (ADR-149), `gcache` Must* helpers (ADR-150), `gjson` len/must encoders (ADR-151), `gdb` Model save upsert (ADR-152), `gvalid` assoc-driven rule tables (ADR-153), `ghttp` multipart parsing (ADR-154), chunked request bodies (ADR-155), chunked response framing (ADR-156), `gclient` chunked decoding (ADR-157), `ghttp.Context` value getters (ADR-158), `gredis` replica round-robin (ADR-159), `gdb` DISTINCT (ADR-160), WHERE prefix/suffix helpers (ADR-161), `FieldsEx` (ADR-162), model offset/omit/has-field/insert-id helpers (ADR-163), `Increment`/`Decrement` (ADR-164), `Data`/`Array` (ADR-165), table aliases + batch insert (ADR-166), raw SELECT (ADR-167), `CountColumn`/`QuoteWord` (ADR-168), `Replace` (ADR-169), `ghttp` header limit + 431 (ADR-170), `OnConflict` upsert (ADR-171), gsession AES file crypto (ADR-172), `gres` `PackToGoFile` (ADR-173), Redis cache adapter (ADR-174), Redis session storage (ADR-175), `gclient` OPTIONS/TRACE/CONNECT (ADR-176), `gtime` layout-token cache (ADR-177), `FieldsPrefix` (ADR-178), per-domain routing (ADR-180), `gproc` signals (ADR-181), `gctx` equivalent (ADR-182), `gstructs` derive equivalent (ADR-183), `gmetric` memory provider (ADR-184), `ghttp` admin endpoints (ADR-185), `gcmd` object subcommands (ADR-186), PostgreSQL driver (ADR-187), SO_REUSEPORT restarts (ADR-188), MySQL driver (ADR-189).
  `os/gctx` and
  `os/gstructs` are skipped (ADR-052).
- `os/genv` has one bridged export: `MapFromEnv` is recorded in
  `verify/genv` and its gate passes; the environment readers/writers,
  `*gvar.Var` returns, variadic `Remove` and the unordered
  `Build`/`MapToEnv`/`Filter` stay in Go with that reason in
  `rust-anything.json`.
- Replaced instead of ported: `internal/reflection`, `internal/mutex`,
  `internal/rwmutex`, `utils.IsStruct`, `utils.CanCallIsNil` (ADR-011).
- Verification: `cargo test --workspace`, `cargo fmt --all`,
  `cargo clippy --workspace --all-targets -- -D warnings` all clean.
- Performance: see [`BENCH.md`](BENCH.md). Rust is 3.7x-15x faster on
  charset conversion, random numbers, regexp matching, tag stripping and
  emptiness checks; 1.4x-3.8x faster on gzip, JSON/TOML/YAML/properties/XML,
  BASE64, argument parsing, CRC32, AES, RSA-independent crypto, IPv4 and URL
  handling; SHA-256, gbinary and the ghash family are at parity; MD5 is
  1.3x slower. Only `gerror` creation trails (1.6x-2.7x) because
  `Backtrace::force_capture` costs ~1.0 纰宻 against Go's ~0.25 纰宻
  `runtime.Callers`; a stack-less `Error::new` is ~58 ns. Among the new units,
  sorted arrays and `gtype` beat Go, list maps win at small sizes, and the
  comparator-ordered trees (2x), tree iteration (4.8x), list iteration (17x,
  both snapshot to keep callbacks out of the lock), `gtime::format` (36x, it
  rebuilds the format each call) and `genv::MapFromEnv` (2x) trail.
- Plan and decisions: [`MAP.md`](MAP.md), [`DECISIONS.md`](DECISIONS.md),
  [`GLOSSARY.md`](GLOSSARY.md); measured plan in [`whole-app.md`](whole-app.md).

## Toolchain

- Go 1.25.0 installed and on the machine PATH; the running editor session still
  inherits the pre-install environment, so the Kilo tool shell needs the editor
  restarted (or the absolute path `C:\Program Files\Go\bin\go.exe`).
- cgo works: MSYS2 was installed with `winget install -e --id MSYS2.MSYS2` and
  `mingw-w64-x86_64-gcc` 16.2.0 with
  `C:\msys64\usr\bin\pacman.exe -S --noconfirm --needed mingw-w64-x86_64-gcc`.
  Put `C:\msys64\mingw64\bin` on `PATH` and set `CGO_ENABLED=1` for Go units.
  A minimal `import "C"` program builds and runs.
- Phase 0 spike (2026-11-24): `rust-anything` 3.4.0 has `bridge --out`, and the
  Go lane works end to end:
  - `util/gpage` cannot be bridged at all: every export returns or takes
    `*Page` (`bridge` refuses and lists the pointer receiver). It stays on the
    translated-test lane.
  - `os/genv` partial bridge: `MapFromEnv` recorded, ported in
    `verify/genv/src/port.rs` (delegates to `gf_os::genv`), `diff` 23/23 and
    `gate` passed. The other exports stay in Go: environment readers/writers,
    `*gvar.Var` returns, variadic `Remove`, and `Build`/`MapToEnv`/`Filter`,
    whose array order is Go's unspecified map iteration order.
  - Harness crates live in `verify/<unit>` and are excluded from the workspace
    (root `Cargo.toml` `exclude = ["verify"]`), so they never enter the facade.
- `go env -w GOPROXY=https://goproxy.cn,direct` and `GOSUMDB=off` were set so
  the bench module can resolve modules on this network.

## Next steps (round-118 snapshot)

Done and gated (per-unit evidence lives in `MAP.md`):

- `os/*`: every portable unit is complete, including the `gspath` cached lane
  (ADR-091), `gview` i18n/break/continue (ADR-088/145), `gproc`'s shell and
  manager lanes (ADR-141), `gsession` file crypto and Redis storage
  (ADR-172/175), `gcache` Redis adapter (ADR-174), `gres` `PackToGoFile`
  (ADR-173) and the `gtime` layout-token cache (ADR-177), `FieldsPrefix` (ADR-178), per-domain routing (ADR-180), `gproc` signals (ADR-181), `gctx` equivalent (ADR-182), `gstructs` derive equivalent (ADR-183), `gmetric` memory provider (ADR-184), `ghttp` admin endpoints (ADR-185), `gcmd` object subcommands (ADR-186), PostgreSQL driver (ADR-187), SO_REUSEPORT restarts (ADR-188), MySQL driver (ADR-189).
- `net/*`: gipv4/gipv6/gudp/gtcp, gsvc/gsel/gtrace, goai, ghttp
  (messages/router/server/static/keep-alive/status/session/openapi/config-map/
  HTTPS/hooks/multipart/chunked/header limits) and gclient (HTTP/HTTPS,
  redirects, chunked decoding, OPTIONS/TRACE/CONNECT).
- `database/*`: gredis RESP2/RESP3 core, sentinel, cluster slot routing with
  replica round-robin, pipelines, transactions, pub/sub, scripts and pools;
  gdb value layer, SQL builder (joins/distinct/upsert/raw), SQLite execution,
  chained Model, transactions, pools, sharding and model caching; Redis cache
  and session adapters (ADR-174/175).
- `i18n/gi18n` (file/packed-resource/auto-reload/accept-language lanes),
  `frame/g` + `frame/gins`, `test/gtest` and `util/gvalid`'s derive struct lane.

Remaining (each recorded in an ADR; all are reflection/platform/driver gaps, and ADR-179 consolidates the `ghttp` ones — no work is planned unless a deployment needs them):

1. `net/ghttp`: HTTP/2/ALPN and fd-passing restart are recorded as not
   ported (ADR-190); SO_REUSEPORT restarts, per-domain routing and admin are
   done (ADR-180/185/188). Use a reverse proxy for h2.
2. `os/gmetric`: external OTel exporters/providers remain an external
   concern; the port ships an in-memory provider (ADR-184).
3. Optional: cheaper stack capture for `gerror`/`gdebug` (bounded frame walk
   via the `backtrace` crate) and dropping the tree comparator `Arc`
   indirection.