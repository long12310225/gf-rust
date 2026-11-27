# Rust migration plan: gf (Go)

31 packages, 1036 source files (126966 lines), 977 test files, 0 UI files, grouped into 320 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 200 | 120 | 0 | 0 |

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `encoding/gini/` | 107 | 86 | 89 | 1 | cgo | String offsets and slicing (2); Null references (14); Dynamic typing (1) |
| 2 | `encoding/gproperties/` | 118 | 85 | 82 | 1 | cgo | String offsets and slicing (1); Pointers and manual memory (1); Null references (13) |
| 3 | `os/genv/` | 142 | 79 | 73 | 7 | cgo | String offsets and slicing (2); Pointers and manual memory (2); Null references (9) |
| 4 | `encoding/ghtml/` | 119 | 78 | 93 | 3 | cgo | Hash iteration order (1); Reflection and prototypes (1); Null references (1) |
| 5 | `encoding/ghash/` | 243 | 78 | 80 | 2 | cgo | Integer width and overflow (46); Division and modulo (4) |
| 6 | `encoding/gyaml/` | 78 | 76 | 91 | 1 | cgo | Pointers and manual memory (1); Null references (9) |
| 7 | `util/gmode/` | 80 | 75 | 100 | 2 | cgo | - |
| 8 | `encoding/gcharset/` | 103 | 75 | 99 | 2 | cgo | Null references (8) |
| 9 | `crypto/gsha1/` | 46 | 74 | 98 | 2 | cgo | String offsets and slicing (1); Null references (5); Exceptions (2) |
| 10 | `crypto/gsha256/` | 46 | 74 | 98 | 1 | cgo | String offsets and slicing (1); Null references (5); Exceptions (2) |

### `encoding/gini/`

107 lines in 1 file, wave 4, priority 86, readiness 89, bridge **cgo**.

- 1 test file imports it directly.
- String offsets and slicing x2: `encoding/gini/gini.go:49`, `encoding/gini/gini.go:63`
- Null references x14: `encoding/gini/gini.go:33`, `encoding/gini/gini.go:67`, `encoding/gini/gini.go:68`
- Dynamic typing x1: `encoding/gini/gini.go:87`

### `encoding/gproperties/`

118 lines in 1 file, wave 10, priority 85, readiness 82, bridge **cgo**.

- 1 Go modules dependencies not in the crate table: check them by hand.
- 1 test file imports it directly.
- String offsets and slicing x1: `encoding/gproperties/gproperties.go:36`
- Pointers and manual memory x1: `encoding/gproperties/gproperties.go:70`
- Null references x13: `encoding/gproperties/gproperties.go:26`, `encoding/gproperties/gproperties.go:26`, `encoding/gproperties/gproperties.go:28`
- Dynamic typing x2: `encoding/gproperties/gproperties.go:101`, `encoding/gproperties/gproperties.go:124`

### `os/genv/`

142 lines in 2 files, wave 13, priority 79, readiness 73, bridge **cgo**.

- 7 test files import it directly.
- String offsets and slicing x2: `os/genv/genv.go:122`, `os/genv/genv.go:122`
- Pointers and manual memory x2: `os/genv/genv.go:35`, `os/genv/genv.go:90`
- Null references x9: `os/genv/genv.go:41`, `os/genv/genv.go:50`, `os/genv/genv.go:59`
- Exceptions x2: `os/genv/genv_must.go:12`, `os/genv/genv_must.go:19`

### `encoding/ghtml/`

119 lines in 1 file, wave 3, priority 78, readiness 93, bridge **cgo**.

- 1 Go modules dependencies not in the crate table: check them by hand.
- 3 test files import it directly.
- Hash iteration order x1: `encoding/ghtml/ghtml.go:86`
- Reflection and prototypes x1: `encoding/ghtml/ghtml.go:12`
- Null references x1: `encoding/ghtml/ghtml.go:131`

### `encoding/ghash/`

243 lines in 9 files, wave 0, priority 78, readiness 80, bridge **cgo**.

- Leaf: imports no other unit.
- 2 test files import it directly.
- Integer width and overflow x46: `encoding/ghash/ghash_ap.go:14`, `encoding/ghash/ghash_ap.go:14`, `encoding/ghash/ghash_ap.go:14`
- Division and modulo x4: `encoding/ghash/ghash_pjw.go:13`, `encoding/ghash/ghash_pjw.go:14`, `encoding/ghash/ghash_pjw.go:32`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. github.com/gogf/gf/v2 (`.`, portable, 463 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `internal/consts/` | ready | 18 | 98 | cgo | - |
| 0 | `version.go` | ready | 5 | 95 | cgo | - |
| 0 | `internal/mutex/` | caution | 51 | 91 | cgo | Pointers and manual memory (6); Null references (3) |
| 0 | `internal/command/` | ready | 124 | 88 | cgo | Regex features the `regex` crate lacks (1) |
| 0 | `internal/fileinfo/` | caution | 43 | 86 | cgo | Pointers and manual memory (8); Null references (1) |
| 0 | `internal/reflection/` | caution | 87 | 86 | cgo | Dynamic typing (3); Null references (2) |
| 0 | `os/gmutex/` | caution | 103 | 83 | cgo | Pointers and manual memory (8); Callback-shaped APIs (6) |
| 0 | `errors/gcode/` | caution | 99 | 81 | cgo | Null references (23); Dynamic typing (1) |
| 0 | `internal/deepcopy/` | caution | 121 | 81 | cgo | Dynamic typing (5); Null references (2) |
| 0 | `encoding/ghash/` | ready | 243 | 80 | cgo | Integer width and overflow (46); Division and modulo (4) |
| 0 | `internal/rwmutex/` | caution | 67 | 77 | cgo | Pointers and manual memory (8); Null references (5) |
| 0 | `net/ghttp/internal/response/` | caution | 153 | 71 | cgo | Pointers and manual memory (23); Dynamic typing (3) |
| 1 | `internal/errors/` | ready | 49 | 100 | cgo | - |
| 1 | `internal/empty/` | caution | 219 | 74 | cgo | Dynamic typing (11); Null references (6) |
| 2 | `internal/utils/` | caution | 455 | 42 | cgo | Pointers and manual memory (15); String offsets and slicing (2) |
| 2 | `errors/gerror/` | caution | 815 | 19 | cgo | Pointers and manual memory (49); String offsets and slicing (4) |
| 3 | `encoding/ghtml/` | ready | 119 | 93 | cgo | Hash iteration order (1); Reflection and prototypes (1) |
| 3 | `encoding/gurl/` | ready | 81 | 93 | cgo | Null references (3) |
| 3 | `encoding/gbase64/` | ready | 108 | 85 | cgo | String offsets and slicing (1); Null references (11) |
| 3 | `internal/json/` | ready | 74 | 81 | cgo | Pointers and manual memory (2); Null references (4) |
| 3 | `crypto/gaes/` | ready | 182 | 69 | cgo | String offsets and slicing (3); Pointers and manual memory (1) |
| 3 | `crypto/gdes/` | ready | 248 | 66 | cgo | String offsets and slicing (13); Null references (56) |
| 3 | `debug/gdebug/` | ready | 342 | 65 | cgo | String offsets and slicing (8); Regex features the `regex` crate lacks (1) |
| 3 | `net/gipv4/` | ready | 297 | 65 | cgo | Pointers and manual memory (3); String offsets and slicing (1) |
| 3 | `crypto/grsa/` | ready | 480 | 61 | cgo | Pointers and manual memory (15); Null references (107) |
| 3 | `util/grand/` | ready | 257 | 54 | cgo | String offsets and slicing (6); Integer width and overflow (17) |
| 3 | `text/gregex/` | ready | 177 | 53 | cgo | Regex features the `regex` crate lacks (2); Pointers and manual memory (2) |
| 4 | `net/gipv6/` | ready | 12 | 100 | cgo | - |
| 4 | `encoding/gtoml/` | ready | 43 | 96 | cgo | Pointers and manual memory (1); Null references (10) |
| 4 | `encoding/gini/` | ready | 107 | 89 | cgo | String offsets and slicing (2); Null references (14) |
| 4 | `internal/intlog/` | caution | 111 | 82 | cgo | Callback-shaped APIs (2); Null references (2) |
| 4 | `util/gtag/` | ready | 148 | 71 | cgo | String offsets and slicing (1); Regex features the `regex` crate lacks (1) |
| 5 | `encoding/gcharset/` | ready | 103 | 99 | cgo | Null references (8) |
| 5 | `encoding/gbinary/` | caution | 663 | 48 | cgo | String offsets and slicing (6); Integer width and overflow (46) |
| 5 | `os/gstructs/` | caution | 675 | 29 | cgo | Pointers and manual memory (26); String offsets and slicing (5) |
| 5 | `os/gtime/` | caution | 1332 | 0 | cgo | Pointers and manual memory (123); String offsets and slicing (3) |
| 6 | `encoding/gxml/` | ready | 119 | 86 | cgo | Null references (20); Integer width and overflow (2) |
| 6 | `util/gconv/internal/localinterface/` | ready | 95 | 84 | cgo | Pointers and manual memory (1); Dynamic typing (21) |
| 7 | `util/gconv/internal/structcache/` | caution | 558 | 40 | cgo | Pointers and manual memory (51); Null references (9) |
| 8 | `util/gconv/internal/converter/` | caution | 6079 | 0 | cgo | Pointers and manual memory (153); String offsets and slicing (5) |
| 9 | `util/gconv/` | caution | 1289 | 18 | cgo | Pointers and manual memory (35); Null references (23) |
| 10 | `crypto/gcrc32/` | caution | 16 | 100 | cgo | - |
| 10 | `internal/httputil/` | caution | 76 | 100 | cgo | Dynamic typing (1); Null references (1) |
| 10 | `crypto/gsha1/` | ready | 46 | 98 | cgo | String offsets and slicing (1); Null references (5) |
| 10 | `crypto/gsha256/` | ready | 46 | 98 | cgo | String offsets and slicing (1); Null references (5) |
| 10 | `crypto/gsha512/` | ready | 46 | 98 | cgo | String offsets and slicing (1); Null references (5) |
| 10 | `crypto/gmd5/` | ready | 86 | 97 | cgo | Null references (11); Integer width and overflow (1) |
| 10 | `encoding/gyaml/` | ready | 78 | 91 | cgo | Pointers and manual memory (1); Null references (9) |
| 10 | `encoding/gproperties/` | ready | 118 | 82 | cgo | String offsets and slicing (1); Pointers and manual memory (1) |
| 10 | `container/glist/` | caution | 1146 | 10 | cgo | Pointers and manual memory (179); Null references (68) |
| 10 | `container/gtype/` | caution | 993 | 0 | cgo | Pointers and manual memory (202); String offsets and slicing (1) |
| 10 | `text/gstr/` | caution | 2094 | 0 | cgo | String offsets and slicing (75); Regex features the `regex` crate lacks (2) |
| 11 | `internal/tracing/` | caution | 41 | 91 | cgo | String offsets and slicing (5); Integer width and overflow (1) |
| 11 | `util/gpage/` | caution | 222 | 81 | cgo | Pointers and manual memory (11); Integer width and overflow (3) |
| 11 | `util/guid/` | caution | 126 | 79 | cgo | String offsets and slicing (8); Integer width and overflow (6) |
| 11 | `container/gqueue/` | caution | 185 | 69 | cgo | Pointers and manual memory (17); Concurrency (9) |
| 11 | `test/gtest/` | caution | 450 | 64 | cgo | Pointers and manual memory (16); Null references (11) |
| 11 | `container/gring/` | caution | 333 | 54 | cgo | Pointers and manual memory (55); Null references (14) |
| 11 | `os/gtimer/` | caution | 636 | 23 | cgo | Pointers and manual memory (67); String offsets and slicing (1) |
| 11 | `container/gset/` | caution | 1278 | 0 | cgo | Pointers and manual memory (225); Null references (40) |
| 11 | `util/gutil/` | caution | 1269 | 0 | cgo | String offsets and slicing (12); Hash iteration order (2) |
| 12 | `net/gtrace/internal/provider/` | caution | 55 | 81 | cgo | Pointers and manual memory (6) |
| 12 | `container/gpool/` | caution | 225 | 65 | cgo | Pointers and manual memory (22); Null references (12) |
| 12 | `os/grpool/` | ready | 365 | 51 | cgo | Pointers and manual memory (30); Integer width and overflow (12) |
| 12 | `container/gvar/` | caution | 592 | 43 | cgo | Pointers and manual memory (81); Null references (10) |
| 12 | `container/garray/` | caution | 4253 | 0 | cgo | Pointers and manual memory (641); String offsets and slicing (60) |
| 13 | `os/gbuild/` | ready | 77 | 97 | cgo | Pointers and manual memory (2); Null references (2) |
| 13 | `util/gmeta/` | caution | 39 | 87 | cgo | Pointers and manual memory (1); Null references (3) |
| 13 | `os/genv/` | ready | 142 | 73 | cgo | String offsets and slicing (2); Pointers and manual memory (2) |
| 13 | `container/gtree/` | caution | 2737 | 0 | cgo | Pointers and manual memory (380); Callback-shaped APIs (117) |
| 13 | `util/gvalid/internal/builtin/` | caution | 2725 | 0 | cgo | Pointers and manual memory (3); Null references (102) |
| 14 | `container/gmap/` | caution | 3669 | 0 | cgo | Pointers and manual memory (624); Null references (111) |
| 15 | `internal/instance/` | ready | 71 | 90 | cgo | Pointers and manual memory (2); Callback-shaped APIs (2) |
| 15 | `os/gmlock/` | ready | 193 | 68 | cgo | Pointers and manual memory (22); Callback-shaped APIs (10) |
| 15 | `net/gudp/` | caution | 487 | 47 | cgo | Pointers and manual memory (37); String offsets and slicing (1) |
| 15 | `database/gredis/` | caution | 790 | 24 | cgo | Pointers and manual memory (78); Null references (35) |
| 16 | `os/gcache/` | caution | 1727 | 0 | cgo | Pointers and manual memory (168); Null references (150) |
| 17 | `os/gfsnotify/` | caution | 784 | 15 | cgo | Pointers and manual memory (66); Null references (52) |
| 18 | `os/gfpool/` | caution | 234 | 64 | cgo | Pointers and manual memory (24); Null references (25) |
| 18 | `os/gfile/` | caution | 1833 | 0 | cgo | String offsets and slicing (12); Pointers and manual memory (10) |
| 19 | `util/gmode/` | ready | 80 | 100 | cgo | - |
| 19 | `os/gspath/` | caution | 338 | 55 | cgo | Pointers and manual memory (24); String offsets and slicing (4) |
| 19 | `encoding/gcompress/` | ready | 448 | 53 | cgo | Pointers and manual memory (12); String offsets and slicing (2) |
| 19 | `os/gsession/` | caution | 1306 | 3 | cgo | Pointers and manual memory (115); String offsets and slicing (2) |
| 19 | `encoding/gjson/` | caution | 1452 | 0 | cgo | Pointers and manual memory (123); String offsets and slicing (7) |
| 19 | `net/gtcp/` | caution | 1145 | 0 | cgo | Pointers and manual memory (85); String offsets and slicing (9) |
| 20 | `net/gtrace/` | caution | 364 | 38 | cgo | Pointers and manual memory (16); Null references (10) |
| 20 | `os/gres/` | caution | 864 | 16 | cgo | Pointers and manual memory (56); String offsets and slicing (6) |
| 20 | `os/gmetric/` | caution | 1217 | 3 | cgo | Pointers and manual memory (57); String offsets and slicing (1) |
| 21 | `net/ghttp/internal/swaggerui/` | ready | 21 | 99 | cgo | Null references (1); Exceptions (1) |
| 21 | `os/gctx/` | caution | 118 | 59 | cgo | Pointers and manual memory (6); String offsets and slicing (2) |
| 22 | `i18n/gi18n/` | ready | 396 | 50 | cgo | Pointers and manual memory (19); String offsets and slicing (2) |
| 22 | `os/gcfg/` | caution | 1697 | 0 | cgo | Pointers and manual memory (180); Null references (151) |
| 22 | `os/glog/` | caution | 2545 | 0 | cgo | Pointers and manual memory (180); String offsets and slicing (5) |
| 23 | `os/gcron/` | caution | 1062 | 15 | cgo | Pointers and manual memory (83); Null references (49) |
| 23 | `os/gproc/` | caution | 989 | 0 | cgo | Pointers and manual memory (49); String offsets and slicing (1) |
| 23 | `util/gvalid/` | caution | 1556 | 0 | cgo | Pointers and manual memory (58); String offsets and slicing (11) |
| 24 | `net/ghttp/internal/graceful/` | caution | 343 | 54 | cgo | Pointers and manual memory (30); Null references (21) |
| 24 | `net/goai/` | caution | 2166 | 0 | cgo | Pointers and manual memory (166); String offsets and slicing (9) |
| 24 | `os/gcmd/` | caution | 1380 | 0 | cgo | Pointers and manual memory (67); String offsets and slicing (4) |
| 25 | `net/gsvc/` | caution | 509 | 37 | cgo | Pointers and manual memory (18); Null references (27) |
| 25 | `os/gview/` | caution | 1200 | 16 | cgo | Pointers and manual memory (94); Null references (59) |
| 25 | `database/gdb/` | caution | 11941 | 0 | cgo | Pointers and manual memory (919); String offsets and slicing (30) |
| 26 | `net/gsel/` | caution | 351 | 44 | cgo | Pointers and manual memory (31); Null references (16) |
| 26 | `net/ghttp/` | caution | 8808 | 0 | cgo | Pointers and manual memory (615); String offsets and slicing (21) |
| 27 | `net/gclient/` | caution | 2511 | 0 | cgo | Pointers and manual memory (324); String offsets and slicing (2) |
| 28 | `frame/gins/` | caution | 536 | 51 | cgo | Pointers and manual memory (20); Null references (27) |
| 29 | `frame/g/` | ready | 270 | 66 | cgo | Pointers and manual memory (17); Callback-shaped APIs (5) |

### 2. github.com/gogf/gf/contrib/config/apollo/v2 (`contrib/config/apollo`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 23 | `contrib/config/apollo/apollo_adapter_ctx.go` | caution | 114 | 71 | cgo | Pointers and manual memory (22); Null references (7) |
| 30 | `contrib/config/apollo/apollo.go` | caution | 179 | 59 | cgo | Pointers and manual memory (20); Null references (21) |

### 3. github.com/gogf/gf/contrib/config/consul/v2 (`contrib/config/consul`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 23 | `contrib/config/consul/consul_adapter_ctx.go` | caution | 82 | 75 | cgo | Pointers and manual memory (16); Null references (5) |
| 30 | `contrib/config/consul/consul.go` | caution | 201 | 59 | cgo | Pointers and manual memory (24); Null references (32) |

### 4. github.com/gogf/gf/contrib/config/kubecm/v2 (`contrib/config/kubecm`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 19 | `contrib/config/kubecm/kubecm_kube.go` | ready | 53 | 74 | cgo | Pointers and manual memory (6); Null references (5) |
| 23 | `contrib/config/kubecm/kubecm_adapter_ctx.go` | caution | 114 | 71 | cgo | Pointers and manual memory (22); Null references (7) |
| 30 | `contrib/config/kubecm/kubecm.go` | caution | 203 | 51 | cgo | Pointers and manual memory (22); Null references (28) |

### 5. github.com/gogf/gf/contrib/config/nacos/v2 (`contrib/config/nacos`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 23 | `contrib/config/nacos/nacos_adapter_ctx.go` | caution | 113 | 72 | cgo | Pointers and manual memory (19); Null references (6) |
| 30 | `contrib/config/nacos/nacos.go` | caution | 151 | 52 | cgo | Pointers and manual memory (17); Null references (24) |

### 6. github.com/gogf/gf/contrib/config/polaris/v2 (`contrib/config/polaris`, portable, 0 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 23 | `contrib/config/polaris/polaris_adapter_ctx.go` | caution | 111 | 68 | cgo | Pointers and manual memory (19); Null references (6) |
| 30 | `contrib/config/polaris/polaris.go` | caution | 185 | 47 | cgo | Pointers and manual memory (19); Null references (22) |

### 7. github.com/gogf/gf/contrib/drivers/clickhouse/v2 (`contrib/drivers/clickhouse`, portable, 38 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/clickhouse/clickhouse_dialect.go` | ready | 17 | 99 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/clickhouse/clickhouse_insert.go` | ready | 22 | 92 | cgo | Pointers and manual memory (3); Null references (2) |
| 0 | `contrib/drivers/clickhouse/clickhouse_ping.go` | ready | 35 | 83 | cgo | Pointers and manual memory (5); Null references (2) |
| 3 | `contrib/drivers/clickhouse/clickhouse_result.go` | ready | 35 | 97 | cgo | Pointers and manual memory (2) |
| 26 | `contrib/drivers/clickhouse/clickhouse_do_delete.go` | ready | 16 | 99 | cgo | Pointers and manual memory (1) |
| 26 | `contrib/drivers/clickhouse/clickhouse_do_commit.go` | ready | 21 | 95 | cgo | Pointers and manual memory (2); Null references (1) |
| 26 | `contrib/drivers/clickhouse/clickhouse_tables.go` | ready | 30 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/clickhouse/clickhouse_do_update.go` | ready | 28 | 92 | cgo | Pointers and manual memory (1); Null references (4) |
| 26 | `contrib/drivers/clickhouse/clickhouse_open.go` | ready | 42 | 91 | cgo | Pointers and manual memory (3); Null references (2) |
| 26 | `contrib/drivers/clickhouse/clickhouse_local_type.go` | ready | 204 | 89 | cgo | String offsets and slicing (1); Pointers and manual memory (1) |
| 26 | `contrib/drivers/clickhouse/clickhouse_transaction.go` | ready | 28 | 87 | cgo | Pointers and manual memory (4); Callback-shaped APIs (2) |
| 26 | `contrib/drivers/clickhouse/clickhouse.go` | caution | 48 | 85 | cgo | Pointers and manual memory (6); Null references (3) |
| 26 | `contrib/drivers/clickhouse/clickhouse_table_fields.go` | ready | 125 | 75 | cgo | String offsets and slicing (5); Pointers and manual memory (4) |
| 26 | `contrib/drivers/clickhouse/clickhouse_do_filter.go` | ready | 347 | 52 | cgo | String offsets and slicing (25); Pointers and manual memory (4) |
| 26 | `contrib/drivers/clickhouse/clickhouse_convert.go` | caution | 533 | 41 | cgo | Pointers and manual memory (18); Null references (68) |
| 26 | `contrib/drivers/clickhouse/clickhouse_do_insert.go` | caution | 374 | 33 | cgo | Pointers and manual memory (16); String offsets and slicing (6) |

### 8. github.com/gogf/gf/contrib/drivers/dm/v2 (`contrib/drivers/dm`, portable, 47 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/dm/dm_result.go` | ready | 20 | 99 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/dm/dm_identifier.go` | ready | 69 | 88 | cgo | String offsets and slicing (4); Pointers and manual memory (1) |
| 0 | `contrib/drivers/dm/dm_conn.go` | ready | 146 | 68 | cgo | Pointers and manual memory (18); Null references (12) |
| 0 | `contrib/drivers/dm/dm_convert.go` | ready | 183 | 64 | cgo | Pointers and manual memory (4); String offsets and slicing (1) |
| 0 | `contrib/drivers/dm/dm_connector.go` | ready | 207 | 55 | cgo | Pointers and manual memory (9); String offsets and slicing (1) |
| 6 | `contrib/drivers/dm/dm_time.go` | ready | 207 | 66 | cgo | Pointers and manual memory (12); Null references (11) |
| 26 | `contrib/drivers/dm/dm_tables.go` | ready | 34 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/dm/dm_local_type.go` | ready | 109 | 94 | cgo | Pointers and manual memory (1); Null references (2) |
| 26 | `contrib/drivers/dm/dm_do_exec.go` | ready | 27 | 93 | cgo | Regex features the `regex` crate lacks (1); Pointers and manual memory (1) |
| 26 | `contrib/drivers/dm/dm_do_commit.go` | ready | 94 | 81 | cgo | Pointers and manual memory (8); Null references (6) |
| 26 | `contrib/drivers/dm/dm_open.go` | ready | 92 | 81 | cgo | Pointers and manual memory (4); Null references (10) |
| 26 | `contrib/drivers/dm/dm_table_fields.go` | ready | 200 | 81 | cgo | Pointers and manual memory (5); Null references (12) |
| 26 | `contrib/drivers/dm/dm_do_filter.go` | ready | 245 | 76 | cgo | String offsets and slicing (8); Regex features the `regex` crate lacks (1) |
| 26 | `contrib/drivers/dm/dm_do_insert.go` | caution | 547 | 46 | cgo | Pointers and manual memory (29); Null references (54) |
| 30 | `contrib/drivers/dm/dm.go` | caution | 47 | 82 | cgo | Pointers and manual memory (6); Null references (2) |

### 9. github.com/gogf/gf/contrib/drivers/gaussdb/v2 (`contrib/drivers/gaussdb`, portable, 39 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/gaussdb/gaussdb_order.go` | ready | 10 | 100 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/gaussdb/gaussdb_result.go` | ready | 19 | 100 | cgo | Null references (1) |
| 26 | `contrib/drivers/gaussdb/gaussdb_local_type.go` | ready | 235 | 98 | cgo | - |
| 26 | `contrib/drivers/gaussdb/gaussdb_dialect.go` | ready | 22 | 97 | cgo | Pointers and manual memory (2) |
| 26 | `contrib/drivers/gaussdb/gaussdb.go` | caution | 41 | 89 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/drivers/gaussdb/gaussdb_open.go` | ready | 64 | 87 | cgo | Pointers and manual memory (4); Null references (6) |
| 26 | `contrib/drivers/gaussdb/gaussdb_do_exec.go` | ready | 85 | 85 | cgo | Pointers and manual memory (1); Null references (12) |
| 26 | `contrib/drivers/gaussdb/gaussdb_do_filter.go` | ready | 55 | 85 | cgo | String offsets and slicing (1); Pointers and manual memory (1) |
| 26 | `contrib/drivers/gaussdb/gaussdb_tables.go` | ready | 92 | 85 | cgo | Pointers and manual memory (2); Regex features the `regex` crate lacks (1) |
| 26 | `contrib/drivers/gaussdb/gaussdb_table_fields.go` | ready | 100 | 84 | cgo | Pointers and manual memory (5); Null references (6) |
| 26 | `contrib/drivers/gaussdb/gaussdb_do_insert.go` | ready | 629 | 51 | cgo | Pointers and manual memory (15); String offsets and slicing (1) |
| 30 | `contrib/drivers/gaussdb/gaussdb_convert.go` | ready | 224 | 56 | cgo | Pointers and manual memory (3); Null references (37) |

### 10. github.com/gogf/gf/contrib/drivers/mssql/v2 (`contrib/drivers/mssql`, portable, 40 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/mssql/mssql_identifier.go` | ready | 13 | 100 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/mssql/mssql_transaction.go` | ready | 36 | 96 | cgo | Regex features the `regex` crate lacks (1); Null references (2) |
| 0 | `contrib/drivers/mssql/mssql_result.go` | ready | 19 | 94 | cgo | Pointers and manual memory (2); Null references (2) |
| 0 | `contrib/drivers/mssql/mssql_alias.go` | ready | 122 | 88 | cgo | String offsets and slicing (5); Null references (1) |
| 26 | `contrib/drivers/mssql/mssql_tables.go` | ready | 32 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/mssql/mssql_lock.go` | ready | 97 | 93 | cgo | String offsets and slicing (3) |
| 26 | `contrib/drivers/mssql/mssql.go` | caution | 45 | 85 | cgo | Pointers and manual memory (7); Null references (4) |
| 26 | `contrib/drivers/mssql/mssql_open.go` | ready | 113 | 81 | cgo | Pointers and manual memory (7); Null references (11) |
| 26 | `contrib/drivers/mssql/mssql_table_fields.go` | ready | 237 | 70 | cgo | Pointers and manual memory (5); String offsets and slicing (3) |
| 26 | `contrib/drivers/mssql/mssql_convert.go` | ready | 167 | 66 | cgo | Pointers and manual memory (10); Null references (19) |
| 26 | `contrib/drivers/mssql/mssql_do_insert.go` | ready | 311 | 66 | cgo | Pointers and manual memory (11); Null references (17) |
| 26 | `contrib/drivers/mssql/mssql_do_exec.go` | ready | 203 | 62 | cgo | Pointers and manual memory (11); String offsets and slicing (2) |
| 26 | `contrib/drivers/mssql/mssql_do_filter.go` | caution | 495 | 49 | cgo | String offsets and slicing (23); Pointers and manual memory (2) |

### 11. github.com/gogf/gf/contrib/drivers/mysql/v2 (`contrib/drivers/mysql`, portable, 35 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/drivers/mysql/mysql_do_filter.go` | ready | 16 | 99 | cgo | Pointers and manual memory (1) |
| 26 | `contrib/drivers/mysql/mysql_tables.go` | ready | 29 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/mysql/mysql_open.go` | ready | 55 | 90 | cgo | Pointers and manual memory (4); Null references (2) |
| 26 | `contrib/drivers/mysql/mysql_table_fields.go` | ready | 97 | 85 | cgo | Pointers and manual memory (4); Null references (6) |
| 30 | `contrib/drivers/mysql/mysql.go` | caution | 46 | 77 | cgo | Pointers and manual memory (6); Null references (2) |

### 12. github.com/gogf/gf/contrib/drivers/mariadb/v2 (`contrib/drivers/mariadb`, portable, 33 test files, uses github.com/gogf/gf/v2, github.com/gogf/gf/contrib/drivers/mysql/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/drivers/mariadb/mariadb_table_fields.go` | ready | 76 | 86 | cgo | Pointers and manual memory (4); Null references (6) |
| 31 | `contrib/drivers/mariadb/mariadb.go` | caution | 54 | 87 | cgo | Pointers and manual memory (7); Null references (4) |

### 13. github.com/gogf/gf/contrib/drivers/oceanbase/v2 (`contrib/drivers/oceanbase`, portable, 33 test files, uses github.com/gogf/gf/v2, github.com/gogf/gf/contrib/drivers/mysql/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 31 | `contrib/drivers/oceanbase/oceanbase.go` | caution | 43 | 91 | cgo | Pointers and manual memory (2); Dynamic typing (1) |

### 14. github.com/gogf/gf/contrib/drivers/oracle/v2 (`contrib/drivers/oracle`, portable, 40 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/oracle/oracle_identifier.go` | ready | 13 | 100 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/oracle/oracle_order.go` | ready | 10 | 100 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/oracle/oracle_transaction.go` | ready | 17 | 99 | cgo | Regex features the `regex` crate lacks (1) |
| 0 | `contrib/drivers/oracle/oracle_result.go` | ready | 20 | 95 | cgo | Pointers and manual memory (2); Null references (1) |
| 0 | `contrib/drivers/oracle/oracle_conn.go` | ready | 53 | 83 | cgo | Pointers and manual memory (3); Null references (5) |
| 26 | `contrib/drivers/oracle/oracle_do_commit.go` | ready | 25 | 97 | cgo | Pointers and manual memory (1); Null references (1) |
| 26 | `contrib/drivers/oracle/oracle_tables.go` | ready | 34 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/oracle/oracle.go` | caution | 39 | 86 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/drivers/oracle/oracle_open.go` | ready | 57 | 85 | cgo | Pointers and manual memory (4); Null references (3) |
| 26 | `contrib/drivers/oracle/oracle_do_exec.go` | ready | 116 | 78 | cgo | Pointers and manual memory (8); Null references (15) |
| 26 | `contrib/drivers/oracle/oracle_table_fields.go` | ready | 174 | 73 | cgo | Pointers and manual memory (5); String offsets and slicing (3) |
| 26 | `contrib/drivers/oracle/oracle_do_insert.go` | ready | 321 | 70 | cgo | Pointers and manual memory (9); Null references (10) |
| 26 | `contrib/drivers/oracle/oracle_convert.go` | ready | 250 | 63 | cgo | Pointers and manual memory (8); Null references (20) |
| 26 | `contrib/drivers/oracle/oracle_do_filter.go` | ready | 467 | 63 | cgo | String offsets and slicing (21); Pointers and manual memory (1) |

### 15. github.com/gogf/gf/contrib/drivers/pgsql/v2 (`contrib/drivers/pgsql`, portable, 39 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/pgsql/pgsql_order.go` | ready | 10 | 100 | cgo | Pointers and manual memory (1) |
| 0 | `contrib/drivers/pgsql/pgsql_result.go` | ready | 19 | 100 | cgo | Null references (1) |
| 26 | `contrib/drivers/pgsql/pgsql_local_type.go` | ready | 205 | 99 | cgo | - |
| 26 | `contrib/drivers/pgsql/pgsql_dialect.go` | ready | 23 | 97 | cgo | Pointers and manual memory (2) |
| 26 | `contrib/drivers/pgsql/pgsql_do_insert.go` | ready | 78 | 93 | cgo | Pointers and manual memory (1); Null references (4) |
| 26 | `contrib/drivers/pgsql/pgsql.go` | caution | 41 | 92 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/drivers/pgsql/pgsql_format_upsert.go` | ready | 87 | 87 | cgo | Pointers and manual memory (4); Dynamic typing (2) |
| 26 | `contrib/drivers/pgsql/pgsql_open.go` | ready | 64 | 87 | cgo | Pointers and manual memory (4); Null references (6) |
| 26 | `contrib/drivers/pgsql/pgsql_do_exec.go` | ready | 85 | 85 | cgo | Pointers and manual memory (1); Null references (12) |
| 26 | `contrib/drivers/pgsql/pgsql_do_filter.go` | ready | 51 | 85 | cgo | String offsets and slicing (1); Pointers and manual memory (1) |
| 26 | `contrib/drivers/pgsql/pgsql_tables.go` | ready | 91 | 85 | cgo | Pointers and manual memory (2); Regex features the `regex` crate lacks (1) |
| 26 | `contrib/drivers/pgsql/pgsql_table_fields.go` | ready | 99 | 84 | cgo | Pointers and manual memory (5); Null references (6) |
| 30 | `contrib/drivers/pgsql/pgsql_convert.go` | ready | 224 | 60 | cgo | Pointers and manual memory (3); Null references (37) |

### 16. github.com/gogf/gf/contrib/drivers/sqlite/v2 (`contrib/drivers/sqlite`, portable, 35 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/sqlite/sqlite_order.go` | ready | 10 | 100 | cgo | Pointers and manual memory (1) |
| 26 | `contrib/drivers/sqlite/sqlite_tables.go` | ready | 34 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/sqlite/sqlite_do_insert.go` | ready | 63 | 94 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/sqlite/sqlite_local_type.go` | ready | 50 | 90 | cgo | Pointers and manual memory (1); Null references (5) |
| 26 | `contrib/drivers/sqlite/sqlite_open.go` | ready | 58 | 89 | cgo | Pointers and manual memory (3); Null references (4) |
| 26 | `contrib/drivers/sqlite/sqlite_table_fields.go` | ready | 45 | 88 | cgo | Pointers and manual memory (4); Null references (5) |
| 26 | `contrib/drivers/sqlite/sqlite_format_upsert.go` | ready | 83 | 87 | cgo | Pointers and manual memory (4); Dynamic typing (2) |
| 26 | `contrib/drivers/sqlite/sqlite.go` | caution | 41 | 78 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/drivers/sqlite/sqlite_do_filter.go` | ready | 225 | 75 | cgo | String offsets and slicing (12); Pointers and manual memory (1) |

### 17. github.com/gogf/gf/cmd/gf/v2 (`cmd/gf`, portable, 30 test files, uses github.com/gogf/gf/v2, github.com/gogf/gf/contrib/drivers/clickhouse/v2, github.com/gogf/gf/contrib/drivers/dm/v2, github.com/gogf/gf/contrib/drivers/mssql/v2, github.com/gogf/gf/contrib/drivers/mysql/v2, github.com/gogf/gf/contrib/drivers/oracle/v2, github.com/gogf/gf/contrib/drivers/pgsql/v2, github.com/gogf/gf/contrib/drivers/sqlite/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `cmd/gf/internal/consts/` | ready | 399 | 91 | cgo | - |
| 0 | `cmd/gf/internal/cmd/agents/common/` | caution | 1579 | 4 | cgo | Pointers and manual memory (10); String offsets and slicing (2) |
| 1 | `cmd/gf/internal/cmd/agents/registry/` | ready | 803 | 66 | cgo | Pointers and manual memory (2); Null references (13) |
| 2 | `cmd/gf/internal/cmd/agents/` | ready | 719 | 53 | cgo | Pointers and manual memory (8); Null references (34) |
| 21 | `cmd/gf/internal/packed/` | ready | 27 | 96 | cgo | Null references (3); Exceptions (3) |
| 25 | `cmd/gf/internal/utility/allyes/` | ready | 23 | 97 | cgo | Null references (1) |
| 25 | `cmd/gf/internal/utility/mlog/` | ready | 62 | 84 | cgo | Null references (2) |
| 26 | `cmd/gf/internal/cmd/geninit/` | caution | 1049 | 14 | cgo | Pointers and manual memory (25); String offsets and slicing (4) |
| 26 | `cmd/gf/internal/utility/utils/` | caution | 660 | 14 | cgo | String offsets and slicing (9); Pointers and manual memory (8) |
| 30 | `cmd/gf/internal/service/` | ready | 227 | 85 | cgo | Pointers and manual memory (2); Null references (2) |
| 30 | `cmd/gf/internal/cmd/genenums/` | caution | 215 | 76 | cgo | Pointers and manual memory (10); Null references (6) |
| 30 | `cmd/gf/internal/cmd/genpb/` | ready | 475 | 74 | cgo | Pointers and manual memory (3); Null references (22) |
| 30 | `cmd/gf/internal/cmd/genpbentity/` | ready | 558 | 57 | cgo | Pointers and manual memory (5); Regex features the `regex` crate lacks (1) |
| 30 | `cmd/gf/internal/cmd/genctrl/` | caution | 1140 | 34 | cgo | Pointers and manual memory (41); Null references (58) |
| 30 | `cmd/gf/internal/cmd/genservice/` | caution | 1048 | 32 | cgo | Pointers and manual memory (42); Null references (60) |
| 30 | `cmd/gf/internal/cmd/gendao/` | caution | 1485 | 15 | cgo | Pointers and manual memory (19); String offsets and slicing (1) |
| 31 | `cmd/gf/internal/cmd/` | caution | 2612 | 0 | cgo | Pointers and manual memory (45); String offsets and slicing (1) |
| 32 | `cmd/gf/gfcmd/` | caution | 106 | 77 | cgo | Pointers and manual memory (4); Null references (9) |
| 33 | `cmd/gf/main.go` | ready | 26 | 99 | cgo | Null references (2); Exceptions (1) |

### 18. github.com/gogf/gf/contrib/drivers/sqlitecgo/v2 (`contrib/drivers/sqlitecgo`, portable, 36 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/drivers/sqlitecgo/sqlitecgo_order.go` | ready | 10 | 100 | cgo | Pointers and manual memory (1) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_tables.go` | ready | 34 | 95 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_do_insert.go` | ready | 63 | 94 | cgo | Pointers and manual memory (1); Null references (3) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_local_type.go` | ready | 50 | 90 | cgo | Pointers and manual memory (1); Null references (5) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_open.go` | ready | 65 | 89 | cgo | Pointers and manual memory (3); Null references (4) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_table_fields.go` | ready | 51 | 88 | cgo | Pointers and manual memory (4); Null references (5) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo.go` | caution | 43 | 87 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_format_upsert.go` | ready | 83 | 87 | cgo | Pointers and manual memory (4); Dynamic typing (2) |
| 26 | `contrib/drivers/sqlitecgo/sqlitecgo_do_filter.go` | ready | 225 | 75 | cgo | String offsets and slicing (12); Pointers and manual memory (1) |

### 19. github.com/gogf/gf/contrib/drivers/tidb/v2 (`contrib/drivers/tidb`, portable, 33 test files, uses github.com/gogf/gf/v2, github.com/gogf/gf/contrib/drivers/mysql/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 31 | `contrib/drivers/tidb/tidb.go` | caution | 54 | 81 | cgo | Pointers and manual memory (7); Null references (4) |

### 20. github.com/gogf/gf/contrib/metric/otelmetric/v2 (`contrib/metric/otelmetric`, portable, 2 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/metric/otelmetric/otelmetric_option.go` | ready | 115 | 72 | cgo | Pointers and manual memory (1); Callback-shaped APIs (6) |
| 21 | `contrib/metric/otelmetric/otelmetric.go` | caution | 27 | 100 | cgo | Null references (4); Exceptions (1) |
| 21 | `contrib/metric/otelmetric/otelmetric_callback.go` | ready | 47 | 86 | cgo | Pointers and manual memory (2); Null references (3) |
| 21 | `contrib/metric/otelmetric/otelmetric_metric_callback.go` | ready | 50 | 86 | cgo | Pointers and manual memory (3); Null references (3) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_counter_performer.go` | ready | 57 | 85 | cgo | Pointers and manual memory (4); Null references (3) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_observable_counter_performer.go` | ready | 50 | 85 | cgo | Pointers and manual memory (2); Null references (4) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_observable_gauge_performer.go` | ready | 50 | 85 | cgo | Pointers and manual memory (2); Null references (4) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_observable_updown_counter_performer.go` | ready | 51 | 85 | cgo | Pointers and manual memory (2); Null references (4) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_updown_counter_performer.go` | ready | 61 | 84 | cgo | Pointers and manual memory (5); Null references (3) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_histogram_performer.go` | ready | 76 | 83 | cgo | Pointers and manual memory (4); Null references (6) |
| 21 | `contrib/metric/otelmetric/otelmetric_util.go` | ready | 150 | 74 | cgo | Pointers and manual memory (3); Dynamic typing (5) |
| 21 | `contrib/metric/otelmetric/otelmetric_meter_performer.go` | ready | 129 | 69 | cgo | Pointers and manual memory (10); Null references (2) |
| 21 | `contrib/metric/otelmetric/otelmetric_provider.go` | ready | 134 | 64 | cgo | Pointers and manual memory (5); Null references (12) |
| 30 | `contrib/metric/otelmetric/otelmetric_prometheus.go` | ready | 28 | 99 | cgo | Pointers and manual memory (1) |

### 21. github.com/gogf/gf/contrib/nosql/redis/v2 (`contrib/nosql/redis`, portable, 17 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 6 | `contrib/nosql/redis/redis_func.go` | ready | 80 | 92 | cgo | Null references (10); Reflection and prototypes (1) |
| 16 | `contrib/nosql/redis/redis_group_hash.go` | ready | 175 | 91 | cgo | Pointers and manual memory (3) |
| 16 | `contrib/nosql/redis/redis_group_script.go` | ready | 105 | 90 | cgo | Pointers and manual memory (3); Null references (2) |
| 16 | `contrib/nosql/redis/redis_group_set.go` | ready | 232 | 90 | cgo | Pointers and manual memory (3) |
| 16 | `contrib/nosql/redis/redis_group_sorted_set.go` | ready | 238 | 88 | cgo | Pointers and manual memory (4) |
| 16 | `contrib/nosql/redis/redis_group_pubsub.go` | ready | 74 | 86 | cgo | Pointers and manual memory (3); Null references (12) |
| 16 | `contrib/nosql/redis/redis_group_list.go` | ready | 241 | 85 | cgo | Pointers and manual memory (7) |
| 16 | `contrib/nosql/redis/redis.go` | caution | 106 | 84 | cgo | Pointers and manual memory (8); Callback-shaped APIs (1) |
| 16 | `contrib/nosql/redis/redis_group_generic.go` | ready | 337 | 83 | cgo | Pointers and manual memory (3); Null references (3) |
| 16 | `contrib/nosql/redis/redis_group_string.go` | ready | 229 | 83 | cgo | Pointers and manual memory (8); Null references (1) |
| 16 | `contrib/nosql/redis/redis_operation.go` | ready | 78 | 79 | cgo | Pointers and manual memory (8); Null references (4) |
| 21 | `contrib/nosql/redis/redis_conn.go` | caution | 265 | 38 | cgo | Pointers and manual memory (30); String offsets and slicing (1) |

### 22. github.com/gogf/gf/contrib/registry/consul/v2 (`contrib/registry/consul`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/consul/consul_discovery.go` | ready | 79 | 80 | cgo | Pointers and manual memory (4); Null references (6) |
| 26 | `contrib/registry/consul/consul.go` | ready | 165 | 63 | cgo | Pointers and manual memory (12); Null references (17) |
| 26 | `contrib/registry/consul/consul_watcher.go` | ready | 181 | 50 | cgo | Pointers and manual memory (12); Null references (19) |

### 23. github.com/gogf/gf/contrib/registry/etcd/v2 (`contrib/registry/etcd`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/etcd/etcd_discovery.go` | ready | 55 | 87 | cgo | Pointers and manual memory (2); Null references (5) |
| 26 | `contrib/registry/etcd/etcd_service.go` | caution | 57 | 86 | cgo | Pointers and manual memory (6); Null references (2) |
| 26 | `contrib/registry/etcd/etcd_watcher.go` | ready | 69 | 76 | cgo | Pointers and manual memory (7); Null references (8) |
| 26 | `contrib/registry/etcd/etcd_registrar.go` | ready | 106 | 72 | cgo | Pointers and manual memory (5); Null references (10) |
| 30 | `contrib/registry/etcd/etcd.go` | caution | 149 | 77 | cgo | Pointers and manual memory (8); String offsets and slicing (1) |

### 24. github.com/gogf/gf/contrib/registry/file/v2 (`contrib/registry/file`, portable, 2 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/file/file.go` | caution | 36 | 100 | cgo | Pointers and manual memory (2) |
| 26 | `contrib/registry/file/file_watcher.go` | ready | 37 | 89 | cgo | Pointers and manual memory (3); Null references (2) |
| 26 | `contrib/registry/file/file_registrar.go` | ready | 55 | 86 | cgo | Pointers and manual memory (5); Null references (2) |
| 26 | `contrib/registry/file/file_service.go` | caution | 57 | 86 | cgo | Pointers and manual memory (6); Null references (2) |
| 30 | `contrib/registry/file/file_discovery.go` | ready | 132 | 76 | cgo | Pointers and manual memory (8); Null references (7) |

### 25. github.com/gogf/gf/contrib/registry/nacos/v2 (`contrib/registry/nacos`, portable, 2 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/nacos/nacos_service.go` | caution | 50 | 89 | cgo | Pointers and manual memory (1); Null references (2) |
| 26 | `contrib/registry/nacos/nacos_register.go` | ready | 71 | 85 | cgo | Pointers and manual memory (2); Integer width and overflow (2) |
| 26 | `contrib/registry/nacos/nacos_discovery.go` | ready | 82 | 79 | cgo | Pointers and manual memory (3); Callback-shaped APIs (2) |
| 26 | `contrib/registry/nacos/nacos_watcher.go` | ready | 65 | 73 | cgo | Pointers and manual memory (9); Concurrency (3) |
| 30 | `contrib/registry/nacos/nacos.go` | ready | 161 | 60 | cgo | Pointers and manual memory (19); Callback-shaped APIs (5) |

### 26. github.com/gogf/gf/contrib/registry/polaris/v2 (`contrib/registry/polaris`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/polaris/polaris_service.go` | ready | 29 | 97 | cgo | Pointers and manual memory (2) |
| 26 | `contrib/registry/polaris/polaris_discovery.go` | ready | 139 | 73 | cgo | Pointers and manual memory (10); Null references (4) |
| 26 | `contrib/registry/polaris/polaris_registry.go` | ready | 104 | 67 | cgo | Pointers and manual memory (17); Null references (5) |
| 26 | `contrib/registry/polaris/polaris_watcher.go` | caution | 166 | 50 | cgo | Pointers and manual memory (14); String offsets and slicing (3) |
| 30 | `contrib/registry/polaris/polaris.go` | ready | 132 | 63 | cgo | Pointers and manual memory (16); Callback-shaped APIs (11) |

### 27. github.com/gogf/gf/contrib/registry/zookeeper/v2 (`contrib/registry/zookeeper`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 26 | `contrib/registry/zookeeper/zookeeper_func.go` | ready | 43 | 89 | cgo | Pointers and manual memory (5); Dynamic typing (1) |
| 26 | `contrib/registry/zookeeper/zookeeper_discovery.go` | ready | 92 | 83 | cgo | Pointers and manual memory (2); Null references (12) |
| 26 | `contrib/registry/zookeeper/zookeeper.go` | ready | 62 | 77 | cgo | Pointers and manual memory (8); Callback-shaped APIs (3) |
| 26 | `contrib/registry/zookeeper/zookeeper_registrar.go` | ready | 131 | 67 | cgo | Pointers and manual memory (5); Null references (10) |
| 26 | `contrib/registry/zookeeper/zookeeper_watcher.go` | ready | 131 | 64 | cgo | Pointers and manual memory (8); Null references (20) |

### 28. github.com/gogf/gf/contrib/rpc/grpcx/v2 (`contrib/rpc/grpcx`, portable, 4 test files, uses github.com/gogf/gf/v2, github.com/gogf/gf/contrib/registry/file/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `contrib/rpc/grpcx/internal/utils/` | caution | 45 | 97 | cgo | Pointers and manual memory (1); Dynamic typing (1) |
| 11 | `contrib/rpc/grpcx/grpcx_grpc_server_unary.go` | ready | 82 | 80 | cgo | Pointers and manual memory (6); Integer width and overflow (2) |
| 27 | `contrib/rpc/grpcx/grpcx_grpc_client.go` | ready | 101 | 88 | cgo | Pointers and manual memory (2); Null references (6) |
| 30 | `contrib/rpc/grpcx/grpcx_registry_file.go` | ready | 32 | 100 | cgo | Null references (1) |
| 30 | `contrib/rpc/grpcx/internal/grpcctx/` | ready | 105 | 86 | cgo | Pointers and manual memory (2); Null references (6) |
| 30 | `contrib/rpc/grpcx/grpcx_grpc_server_config.go` | ready | 97 | 85 | cgo | Pointers and manual memory (6); Null references (3) |
| 30 | `contrib/rpc/grpcx/internal/balancer/` | ready | 203 | 72 | cgo | Pointers and manual memory (9); Null references (6) |
| 30 | `contrib/rpc/grpcx/internal/resolver/` | caution | 215 | 71 | cgo | Pointers and manual memory (12); Null references (9) |
| 30 | `contrib/rpc/grpcx/grpcx_grpc_server.go` | ready | 300 | 55 | cgo | Pointers and manual memory (26); String offsets and slicing (1) |
| 31 | `contrib/rpc/grpcx/grpcx.go` | ready | 34 | 100 | cgo | - |
| 31 | `contrib/rpc/grpcx/internal/tracing/` | caution | 502 | 16 | cgo | Pointers and manual memory (24); Null references (18) |
| 32 | `contrib/rpc/grpcx/grpcx_interceptor_client.go` | ready | 43 | 88 | cgo | Pointers and manual memory (4); Null references (2) |
| 32 | `contrib/rpc/grpcx/grpcx_interceptor_server.go` | ready | 91 | 78 | cgo | Pointers and manual memory (6); Null references (6) |

### 29. github.com/gogf/gf/contrib/sdk/httpclient/v2 (`contrib/sdk/httpclient`, portable, 1 test file, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 28 | `contrib/sdk/httpclient/httpclient_config.go` | ready | 18 | 97 | cgo | Pointers and manual memory (2) |
| 28 | `contrib/sdk/httpclient/httpclient.go` | caution | 85 | 90 | cgo | Pointers and manual memory (5); Null references (4) |
| 30 | `contrib/sdk/httpclient/httpclient_handler.go` | caution | 61 | 82 | cgo | Pointers and manual memory (7); Null references (4) |

### 30. github.com/gogf/gf/contrib/trace/otlpgrpc/v2 (`contrib/trace/otlpgrpc`, portable, 0 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 30 | `contrib/trace/otlpgrpc/otlpgrpc.go` | caution | 97 | 54 | cgo | Null references (10); Callback-shaped APIs (2) |

### 31. github.com/gogf/gf/contrib/trace/otlphttp/v2 (`contrib/trace/otlphttp`, portable, 0 test files, uses github.com/gogf/gf/v2)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 30 | `contrib/trace/otlphttp/otlphttp.go` | caution | 94 | 55 | cgo | Null references (8); Callback-shaped APIs (2) |

## Go modules dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `go.opentelemetry.io/otel` | 12 | ? | unknown |  |
| `go.opentelemetry.io/otel/trace` | 12 | ? | unknown |  |
| `go.opentelemetry.io/otel/metric` | 10 | ? | unknown |  |
| `go.opentelemetry.io/otel/attribute` | 9 | ? | unknown |  |
| `go.opentelemetry.io/otel/propagation` | 8 | ? | unknown |  |
| `github.com/polarismesh/polaris-go` | 5 | ? | unknown |  |
| `go.opentelemetry.io/otel/codes` | 5 | ? | unknown |  |
| `github.com/hashicorp/consul/api` | 4 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/vo` | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter` | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter/renderer` | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter/tw` | 4 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/pkg/model` | 4 | ? | unknown |  |
| `go.etcd.io/etcd/client/v3` | 4 | ? | unknown |  |
| `gitee.com/chunanyong/dm` | 3 | ? | unknown |  |
| `github.com/go-zookeeper/zk` | 3 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/model` | 3 | ? | unknown |  |
| `github.com/sijms/go-ora/v2` | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/metric` | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/resource` | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/trace` | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.24.0` | 3 | ? | unknown |  |
| `gitee.com/opengauss/openGauss-connector-go-pq` | 2 | ? | unknown |  |
| `github.com/charmbracelet/lipgloss` | 2 | ? | unknown |  |
| `github.com/gorilla/websocket` | 2 | ? | unknown |  |
| `github.com/hashicorp/consul/api/watch` | 2 | ? | unknown |  |
| `github.com/microsoft/go-mssqldb` | 2 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients` | 2 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/common/constant` | 2 | ? | unknown |  |
| `go.opentelemetry.io/otel/baggage` | 2 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace` | 2 | ? | unknown |  |
| `golang.org/x/tools/go/packages` | 2 | ? | unknown |  |
| `k8s.io/client-go/kubernetes` | 2 | ? | unknown |  |
| `k8s.io/client-go/rest` | 2 | ? | unknown |  |
| `github.com/ClickHouse/clickhouse-go/v2` | 1 | ? | unknown |  |
| `github.com/ClickHouse/clickhouse-go/v2/lib/driver` | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4` | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4/env/config` | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4/storage` | 1 | ? | unknown |  |
| `github.com/charmbracelet/bubbles/key` | 1 | ? | unknown |  |
| `github.com/charmbracelet/bubbletea` | 1 | ? | unknown |  |
| `github.com/charmbracelet/huh` | 1 | ? | unknown |  |
| `github.com/clbanning/mxj/v2` | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/avltree` | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/btree` | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/redblacktree` | 1 | ? | unknown |  |
| `github.com/fatih/color` | 1 | ? | unknown |  |
| `github.com/fsnotify/fsnotify` | 1 | ? | unknown |  |
| `github.com/glebarez/go-sqlite` | 1 | ? | unknown |  |
| `github.com/go-sql-driver/mysql` | 1 | ? | unknown |  |
| `github.com/gogf/selfupdate` | 1 | ? | unknown |  |
| `github.com/grokify/html-strip-tags-go` | 1 | ? | unknown |  |
| `github.com/magiconair/properties` | 1 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients/config_client` | 1 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients/naming_client` | 1 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/api` | 1 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/pkg/config` | 1 | ? | unknown |  |
| `github.com/schollz/progressbar/v3` | 1 | ? | unknown |  |
| `github.com/shopspring/decimal` | 1 | ? | unknown |  |
| `go.opentelemetry.io/contrib/instrumentation/runtime` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/instrumentation` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/metric/exemplar` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.18.0` | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.7.0` | 1 | ? | unknown |  |
| `go.yaml.in/yaml/v3` | 1 | ? | unknown |  |
| `golang.org/x/mod/modfile` | 1 | ? | unknown |  |
| `golang.org/x/mod/semver` | 1 | ? | unknown |  |
| `golang.org/x/net/proxy` | 1 | ? | unknown |  |
| `golang.org/x/tools/go/ast/astutil` | 1 | ? | unknown |  |
| `k8s.io/apimachinery/pkg/apis/meta/v1` | 1 | ? | unknown |  |
| `k8s.io/apimachinery/pkg/watch` | 1 | ? | unknown |  |
| `k8s.io/client-go/tools/clientcmd` | 1 | ? | unknown |  |
| `modernc.org/sqlite/lib` | 1 | ? | unknown |  |
| `golang.org/x/sync/singleflight` | 2 | tokio, rayon | partial |  |
| `golang.org/x/text/cases` | 1 | unicode-normalization, icu | partial |  |
| `golang.org/x/text/encoding` | 1 | unicode-normalization, icu | partial |  |
| `golang.org/x/text/encoding/ianaindex` | 1 | unicode-normalization, icu | partial |  |
| `golang.org/x/text/language` | 1 | unicode-normalization, icu | partial |  |
| `golang.org/x/text/transform` | 1 | unicode-normalization, icu | partial |  |
| `google.golang.org/grpc` | 9 | tonic | good |  |
| `github.com/google/uuid` | 4 | uuid | good |  |
| `google.golang.org/grpc/status` | 4 | tonic | good |  |
| `github.com/redis/go-redis/v9` | 3 | redis | good |  |
| `google.golang.org/grpc/codes` | 3 | tonic | good |  |
| `google.golang.org/protobuf/proto` | 3 | prost | good |  |
| `github.com/lib/pq` | 2 | sqlx, tokio-postgres | good |  |
| `github.com/mattn/go-sqlite3` | 2 | rusqlite | good |  |
| `google.golang.org/grpc/metadata` | 2 | tonic | good |  |
| `github.com/BurntSushi/toml` | 1 | toml | good |  |
| `github.com/prometheus/client_golang/prometheus` | 1 | prometheus | good |  |
| `github.com/prometheus/client_golang/prometheus/collectors` | 1 | prometheus | good |  |
| `github.com/prometheus/client_golang/prometheus/promhttp` | 1 | prometheus | good |  |
| `google.golang.org/grpc/attributes` | 1 | tonic | good |  |
| `google.golang.org/grpc/balancer` | 1 | tonic | good |  |
| `google.golang.org/grpc/balancer/base` | 1 | tonic | good |  |
| `google.golang.org/grpc/credentials/insecure` | 1 | tonic | good |  |
| `google.golang.org/grpc/encoding/gzip` | 1 | tonic | good |  |
| `google.golang.org/grpc/peer` | 1 | tonic | good |  |
| `google.golang.org/grpc/resolver` | 1 | tonic | good |  |
| `google.golang.org/protobuf/encoding/protojson` | 1 | prost | good |  |

## Risk guide

- **String offsets and slicing** (High): JS indexes strings in UTF-16 code units, Rust in UTF-8 bytes. Offsets that cross the boundary must be converted, or non-ASCII text gets wrong ranges silently.
- **Regex features the `regex` crate lacks** (High): Lookaround, backreferences and the `y` flag are not in the `regex` crate. Use `fancy-regex` for those patterns, or rewrite them, and test them on non-ASCII text: `\w`, `\b` and case-insensitive matching differ between engines even where a pattern compiles. JS `/i` without `u` never matches a non-ASCII letter to an ASCII one, Rust `(?i)` does (`ſ` matches `s`, the Kelvin sign `K` matches `k`): write `(?i-u)` or compare with `eq_ignore_ascii_case`.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Exceptions** (Low): Thrown errors become `Result`. Errors crossing the bridge must map back to the same JS error shape.
- **Reflection and prototypes** (Medium): Proxy, Reflect, Symbol and prototype tricks need a redesign, not a translation.
- **Callback-shaped APIs** (Medium): Function-typed parameters and methods cross the bridge on every call. Keep boundaries coarse: pass data in, get data out.
- **JSON round-trips** (Low): JSON.stringify key order and number formatting must match `serde_json` output if the result is user-visible.
- **Integer width and overflow** (Medium): Python ints never overflow; C, Go and Java ints wrap or are undefined on overflow. Pick i64, i128 or a bignum crate deliberately, and use wrapping_* where the source wraps.
- **Division and modulo** (Medium): Python `//` and `%` floor toward negative infinity; Rust `/` and `%` truncate: use `div_euclid`/`rem_euclid` or explicit floor math. C division by zero is undefined.
- **Hash iteration order** (High): Set and map iteration order comes from the source language's hash function. A port cannot reproduce it: sort before output, or make the order part of the contract.
- **Pointers and manual memory** (High): Pointer arithmetic and manual memory become slices, indices and owned types. Code that exposes pointers across the boundary needs `unsafe`, isolated and explained.
- **Concurrency** (Medium): Threads, goroutines and locks need a design in Rust (channels, Send/Sync), not a line-by-line translation; keep concurrency in the caller where possible.
- **Null references** (Medium): Any reference may be null in the source: decide per parameter whether it is `Option` and what null means, instead of mapping every value to `Option`.

## Whole app

Shape: **cli**.

| Entry point | Kind | Found from |
| --- | --- | --- |
| gf: `cmd/gf` | cli | a `cmd/` folder |

### Every dependency

| Dependency | Declared | Used by | Rust | Fit | Note |
| --- | --- | --- | --- | --- | --- |
| `go.opentelemetry.io/otel` | yes | 12 | ? | unknown |  |
| `go.opentelemetry.io/otel/trace` | yes | 12 | ? | unknown |  |
| `go.opentelemetry.io/otel/metric` | yes | 10 | ? | unknown |  |
| `go.opentelemetry.io/otel/attribute` | no | 9 | ? | unknown |  |
| `go.opentelemetry.io/otel/propagation` | no | 8 | ? | unknown |  |
| `github.com/polarismesh/polaris-go` | yes | 5 | ? | unknown |  |
| `go.opentelemetry.io/otel/codes` | no | 5 | ? | unknown |  |
| `github.com/hashicorp/consul/api` | yes | 4 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/vo` | no | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter` | yes | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter/renderer` | no | 4 | ? | unknown |  |
| `github.com/olekukonko/tablewriter/tw` | no | 4 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/pkg/model` | no | 4 | ? | unknown |  |
| `go.etcd.io/etcd/client/v3` | yes | 4 | ? | unknown |  |
| `gitee.com/chunanyong/dm` | yes | 3 | ? | unknown |  |
| `github.com/go-zookeeper/zk` | yes | 3 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/model` | no | 3 | ? | unknown |  |
| `github.com/sijms/go-ora/v2` | yes | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/metric` | yes | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/resource` | no | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/trace` | no | 3 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.24.0` | no | 3 | ? | unknown |  |
| `gitee.com/opengauss/openGauss-connector-go-pq` | yes | 2 | ? | unknown |  |
| `github.com/charmbracelet/lipgloss` | yes | 2 | ? | unknown |  |
| `github.com/gorilla/websocket` | yes | 2 | ? | unknown |  |
| `github.com/hashicorp/consul/api/watch` | no | 2 | ? | unknown |  |
| `github.com/microsoft/go-mssqldb` | yes | 2 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients` | no | 2 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/common/constant` | no | 2 | ? | unknown |  |
| `go.opentelemetry.io/otel/baggage` | no | 2 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace` | yes | 2 | ? | unknown |  |
| `golang.org/x/tools/go/packages` | no | 2 | ? | unknown |  |
| `k8s.io/client-go/kubernetes` | no | 2 | ? | unknown |  |
| `k8s.io/client-go/rest` | no | 2 | ? | unknown |  |
| `github.com/ClickHouse/clickhouse-go/v2` | yes | 1 | ? | unknown |  |
| `github.com/ClickHouse/clickhouse-go/v2/lib/driver` | no | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4` | yes | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4/env/config` | no | 1 | ? | unknown |  |
| `github.com/apolloconfig/agollo/v4/storage` | no | 1 | ? | unknown |  |
| `github.com/charmbracelet/bubbles/key` | no | 1 | ? | unknown |  |
| `github.com/charmbracelet/bubbletea` | yes | 1 | ? | unknown |  |
| `github.com/charmbracelet/huh` | yes | 1 | ? | unknown |  |
| `github.com/clbanning/mxj/v2` | yes | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/avltree` | no | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/btree` | no | 1 | ? | unknown |  |
| `github.com/emirpasic/gods/v2/trees/redblacktree` | no | 1 | ? | unknown |  |
| `github.com/fatih/color` | yes | 1 | ? | unknown |  |
| `github.com/fsnotify/fsnotify` | yes | 1 | ? | unknown |  |
| `github.com/glebarez/go-sqlite` | yes | 1 | ? | unknown |  |
| `github.com/go-sql-driver/mysql` | yes | 1 | ? | unknown |  |
| `github.com/gogf/selfupdate` | yes | 1 | ? | unknown |  |
| `github.com/grokify/html-strip-tags-go` | yes | 1 | ? | unknown |  |
| `github.com/magiconair/properties` | yes | 1 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients/config_client` | no | 1 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2/clients/naming_client` | no | 1 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/api` | no | 1 | ? | unknown |  |
| `github.com/polarismesh/polaris-go/pkg/config` | no | 1 | ? | unknown |  |
| `github.com/schollz/progressbar/v3` | yes | 1 | ? | unknown |  |
| `github.com/shopspring/decimal` | yes | 1 | ? | unknown |  |
| `go.opentelemetry.io/contrib/instrumentation/runtime` | yes | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc` | yes | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp` | yes | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/instrumentation` | no | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk/metric/exemplar` | no | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.18.0` | no | 1 | ? | unknown |  |
| `go.opentelemetry.io/otel/semconv/v1.7.0` | no | 1 | ? | unknown |  |
| `go.yaml.in/yaml/v3` | yes | 1 | ? | unknown |  |
| `golang.org/x/mod/modfile` | no | 1 | ? | unknown |  |
| `golang.org/x/mod/semver` | no | 1 | ? | unknown |  |
| `golang.org/x/net/proxy` | no | 1 | ? | unknown |  |
| `golang.org/x/tools/go/ast/astutil` | no | 1 | ? | unknown |  |
| `k8s.io/apimachinery/pkg/apis/meta/v1` | no | 1 | ? | unknown |  |
| `k8s.io/apimachinery/pkg/watch` | no | 1 | ? | unknown |  |
| `k8s.io/client-go/tools/clientcmd` | no | 1 | ? | unknown |  |
| `modernc.org/sqlite/lib` | no | 1 | ? | unknown |  |
| `github.com/charmbracelet/bubbles` | yes | 0 | ? | unknown |  |
| `github.com/emirpasic/gods/v2` | yes | 0 | ? | unknown |  |
| `github.com/hashicorp/go-cleanhttp` | yes | 0 | ? | unknown |  |
| `github.com/nacos-group/nacos-sdk-go/v2` | yes | 0 | ? | unknown |  |
| `go.opentelemetry.io/otel/exporters/prometheus` | yes | 0 | ? | unknown |  |
| `go.opentelemetry.io/otel/sdk` | yes | 0 | ? | unknown |  |
| `golang.org/x/mod` | yes | 0 | ? | unknown |  |
| `golang.org/x/net` | yes | 0 | ? | unknown |  |
| `golang.org/x/tools` | yes | 0 | ? | unknown |  |
| `k8s.io/api` | yes | 0 | ? | unknown |  |
| `k8s.io/apimachinery` | yes | 0 | ? | unknown |  |
| `k8s.io/client-go` | yes | 0 | ? | unknown |  |
| `modernc.org/sqlite` | yes | 0 | ? | unknown |  |
| `golang.org/x/sync` (also `golang.org/x/sync/singleflight`) | yes | 2 | tokio, rayon | partial |  |
| `golang.org/x/text` (also `golang.org/x/text/cases`, `golang.org/x/text/encoding`, `golang.org/x/text/encoding/ianaindex`, `golang.org/x/text/language`, `golang.org/x/text/transform`) | yes | 2 | unicode-normalization, icu | partial |  |
| `google.golang.org/grpc` (also `google.golang.org/grpc/attributes`, `google.golang.org/grpc/balancer`, `google.golang.org/grpc/balancer/base`, `google.golang.org/grpc/codes`, `google.golang.org/grpc/credentials/insecure`, `google.golang.org/grpc/encoding/gzip`, `google.golang.org/grpc/metadata`, `google.golang.org/grpc/peer`, `google.golang.org/grpc/resolver`, `google.golang.org/grpc/status`) | yes | 12 | tonic | good |  |
| `github.com/google/uuid` | yes | 4 | uuid | good |  |
| `github.com/redis/go-redis/v9` | yes | 3 | redis | good |  |
| `google.golang.org/protobuf` (also `google.golang.org/protobuf/encoding/protojson`, `google.golang.org/protobuf/proto`) | yes | 3 | prost | good |  |
| `github.com/lib/pq` | yes | 2 | sqlx, tokio-postgres | good |  |
| `github.com/mattn/go-sqlite3` | yes | 2 | rusqlite | good |  |
| `github.com/BurntSushi/toml` | yes | 1 | toml | good |  |
| `github.com/prometheus/client_golang` (also `github.com/prometheus/client_golang/prometheus`, `github.com/prometheus/client_golang/prometheus/collectors`, `github.com/prometheus/client_golang/prometheus/promhttp`) | yes | 1 | prometheus | good |  |

### Pieces, in port order

- wave 0: `cmd/gf/internal/cmd/agents/common/` (1579 lines)
- wave 0: `cmd/gf/internal/consts/` (399 lines)
- wave 0: `contrib/drivers/clickhouse/clickhouse_dialect.go` (17 lines)
- wave 0: `contrib/drivers/clickhouse/clickhouse_insert.go` (22 lines)
- wave 0: `contrib/drivers/clickhouse/clickhouse_ping.go` (35 lines)
- wave 0: `contrib/drivers/dm/dm_conn.go` (146 lines)
- wave 0: `contrib/drivers/dm/dm_connector.go` (207 lines)
- wave 0: `contrib/drivers/dm/dm_convert.go` (183 lines)
- wave 0: `contrib/drivers/dm/dm_identifier.go` (69 lines)
- wave 0: `contrib/drivers/dm/dm_result.go` (20 lines)
- wave 0: `contrib/drivers/gaussdb/gaussdb_order.go` (10 lines)
- wave 0: `contrib/drivers/gaussdb/gaussdb_result.go` (19 lines)
- wave 0: `contrib/drivers/mssql/mssql_alias.go` (122 lines)
- wave 0: `contrib/drivers/mssql/mssql_identifier.go` (13 lines)
- wave 0: `contrib/drivers/mssql/mssql_result.go` (19 lines)
- wave 0: `contrib/drivers/mssql/mssql_transaction.go` (36 lines)
- wave 0: `contrib/drivers/oracle/oracle_conn.go` (53 lines)
- wave 0: `contrib/drivers/oracle/oracle_identifier.go` (13 lines)
- wave 0: `contrib/drivers/oracle/oracle_order.go` (10 lines)
- wave 0: `contrib/drivers/oracle/oracle_result.go` (20 lines)
- wave 0: `contrib/drivers/oracle/oracle_transaction.go` (17 lines)
- wave 0: `contrib/drivers/pgsql/pgsql_order.go` (10 lines)
- wave 0: `contrib/drivers/pgsql/pgsql_result.go` (19 lines)
- wave 0: `contrib/drivers/sqlite/sqlite_order.go` (10 lines)
- wave 0: `contrib/drivers/sqlitecgo/sqlitecgo_order.go` (10 lines)
- wave 0: `contrib/metric/otelmetric/otelmetric_option.go` (115 lines)
- wave 0: `contrib/rpc/grpcx/internal/utils/` (45 lines)
- wave 0: `encoding/ghash/` (243 lines)
- wave 0: `errors/gcode/` (99 lines)
- wave 0: `internal/command/` (124 lines)
- wave 0: `internal/consts/` (18 lines)
- wave 0: `internal/deepcopy/` (121 lines)
- wave 0: `internal/fileinfo/` (43 lines)
- wave 0: `internal/mutex/` (51 lines)
- wave 0: `internal/reflection/` (87 lines)
- wave 0: `internal/rwmutex/` (67 lines)
- wave 0: `net/ghttp/internal/response/` (153 lines)
- wave 0: `os/gmutex/` (103 lines)
- wave 0: `version.go` (5 lines)
- wave 1: `cmd/gf/internal/cmd/agents/registry/` (803 lines)
- wave 1: `internal/empty/` (219 lines)
- wave 1: `internal/errors/` (49 lines)
- wave 2: `cmd/gf/internal/cmd/agents/` (719 lines)
- wave 2: `errors/gerror/` (815 lines)
- wave 2: `internal/utils/` (455 lines)
- wave 3: `contrib/drivers/clickhouse/clickhouse_result.go` (35 lines)
- wave 3: `crypto/gaes/` (182 lines)
- wave 3: `crypto/gdes/` (248 lines)
- wave 3: `crypto/grsa/` (480 lines)
- wave 3: `debug/gdebug/` (342 lines)
- wave 3: `encoding/gbase64/` (108 lines)
- wave 3: `encoding/ghtml/` (119 lines)
- wave 3: `encoding/gurl/` (81 lines)
- wave 3: `internal/json/` (74 lines)
- wave 3: `net/gipv4/` (297 lines)
- wave 3: `text/gregex/` (177 lines)
- wave 3: `util/grand/` (257 lines)
- wave 4: `encoding/gini/` (107 lines)
- wave 4: `encoding/gtoml/` (43 lines)
- wave 4: `internal/intlog/` (111 lines)
- wave 4: `net/gipv6/` (12 lines)
- wave 4: `util/gtag/` (148 lines)
- wave 5: `encoding/gbinary/` (663 lines)
- wave 5: `encoding/gcharset/` (103 lines)
- wave 5: `os/gstructs/` (675 lines)
- wave 5: `os/gtime/` (1332 lines)
- wave 6: `contrib/drivers/dm/dm_time.go` (207 lines)
- wave 6: `contrib/nosql/redis/redis_func.go` (80 lines)
- wave 6: `encoding/gxml/` (119 lines)
- wave 6: `util/gconv/internal/localinterface/` (95 lines)
- wave 7: `util/gconv/internal/structcache/` (558 lines)
- wave 8: `util/gconv/internal/converter/` (6079 lines)
- wave 9: `util/gconv/` (1289 lines)
- wave 10: `container/glist/` (1146 lines)
- wave 10: `container/gtype/` (993 lines)
- wave 10: `crypto/gcrc32/` (16 lines)
- wave 10: `crypto/gmd5/` (86 lines)
- wave 10: `crypto/gsha1/` (46 lines)
- wave 10: `crypto/gsha256/` (46 lines)
- wave 10: `crypto/gsha512/` (46 lines)
- wave 10: `encoding/gproperties/` (118 lines)
- wave 10: `encoding/gyaml/` (78 lines)
- wave 10: `internal/httputil/` (76 lines)
- wave 10: `text/gstr/` (2094 lines)
- wave 11: `container/gqueue/` (185 lines)
- wave 11: `container/gring/` (333 lines)
- wave 11: `container/gset/` (1278 lines)
- wave 11: `contrib/rpc/grpcx/grpcx_grpc_server_unary.go` (82 lines)
- wave 11: `internal/tracing/` (41 lines)
- wave 11: `os/gtimer/` (636 lines)
- wave 11: `test/gtest/` (450 lines)
- wave 11: `util/gpage/` (222 lines)
- wave 11: `util/guid/` (126 lines)
- wave 11: `util/gutil/` (1269 lines)
- wave 12: `container/garray/` (4253 lines)
- wave 12: `container/gpool/` (225 lines)
- wave 12: `container/gvar/` (592 lines)
- wave 12: `net/gtrace/internal/provider/` (55 lines)
- wave 12: `os/grpool/` (365 lines)
- wave 13: `container/gtree/` (2737 lines)
- wave 13: `os/gbuild/` (77 lines)
- wave 13: `os/genv/` (142 lines)
- wave 13: `util/gmeta/` (39 lines)
- wave 13: `util/gvalid/internal/builtin/` (2725 lines)
- wave 14: `container/gmap/` (3669 lines)
- wave 15: `database/gredis/` (790 lines)
- wave 15: `internal/instance/` (71 lines)
- wave 15: `net/gudp/` (487 lines)
- wave 15: `os/gmlock/` (193 lines)
- wave 16: `contrib/nosql/redis/redis.go` (106 lines)
- wave 16: `contrib/nosql/redis/redis_group_generic.go` (337 lines)
- wave 16: `contrib/nosql/redis/redis_group_hash.go` (175 lines)
- wave 16: `contrib/nosql/redis/redis_group_list.go` (241 lines)
- wave 16: `contrib/nosql/redis/redis_group_pubsub.go` (74 lines)
- wave 16: `contrib/nosql/redis/redis_group_script.go` (105 lines)
- wave 16: `contrib/nosql/redis/redis_group_set.go` (232 lines)
- wave 16: `contrib/nosql/redis/redis_group_sorted_set.go` (238 lines)
- wave 16: `contrib/nosql/redis/redis_group_string.go` (229 lines)
- wave 16: `contrib/nosql/redis/redis_operation.go` (78 lines)
- wave 16: `os/gcache/` (1727 lines)
- wave 17: `os/gfsnotify/` (784 lines)
- wave 18: `os/gfile/` (1833 lines)
- wave 18: `os/gfpool/` (234 lines)
- wave 19: `contrib/config/kubecm/kubecm_kube.go` (53 lines)
- wave 19: `encoding/gcompress/` (448 lines)
- wave 19: `encoding/gjson/` (1452 lines)
- wave 19: `net/gtcp/` (1145 lines)
- wave 19: `os/gsession/` (1306 lines)
- wave 19: `os/gspath/` (338 lines)
- wave 19: `util/gmode/` (80 lines)
- wave 20: `net/gtrace/` (364 lines)
- wave 20: `os/gmetric/` (1217 lines)
- wave 20: `os/gres/` (864 lines)
- wave 21: `cmd/gf/internal/packed/` (27 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric.go` (27 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_callback.go` (47 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_counter_performer.go` (57 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_histogram_performer.go` (76 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_observable_counter_performer.go` (50 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_observable_gauge_performer.go` (50 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_observable_updown_counter_performer.go` (51 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_performer.go` (129 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_meter_updown_counter_performer.go` (61 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_metric_callback.go` (50 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_provider.go` (134 lines)
- wave 21: `contrib/metric/otelmetric/otelmetric_util.go` (150 lines)
- wave 21: `contrib/nosql/redis/redis_conn.go` (265 lines)
- wave 21: `net/ghttp/internal/swaggerui/` (21 lines)
- wave 21: `os/gctx/` (118 lines)
- wave 22: `i18n/gi18n/` (396 lines)
- wave 22: `os/gcfg/` (1697 lines)
- wave 22: `os/glog/` (2545 lines)
- wave 23: `contrib/config/apollo/apollo_adapter_ctx.go` (114 lines)
- wave 23: `contrib/config/consul/consul_adapter_ctx.go` (82 lines)
- wave 23: `contrib/config/kubecm/kubecm_adapter_ctx.go` (114 lines)
- wave 23: `contrib/config/nacos/nacos_adapter_ctx.go` (113 lines)
- wave 23: `contrib/config/polaris/polaris_adapter_ctx.go` (111 lines)
- wave 23: `os/gcron/` (1062 lines)
- wave 23: `os/gproc/` (989 lines)
- wave 23: `util/gvalid/` (1556 lines)
- wave 24: `net/ghttp/internal/graceful/` (343 lines)
- wave 24: `net/goai/` (2166 lines)
- wave 24: `os/gcmd/` (1380 lines)
- wave 25: `cmd/gf/internal/utility/allyes/` (23 lines)
- wave 25: `cmd/gf/internal/utility/mlog/` (62 lines)
- wave 25: `database/gdb/` (11941 lines)
- wave 25: `net/gsvc/` (509 lines)
- wave 25: `os/gview/` (1200 lines)
- wave 26: `cmd/gf/internal/cmd/geninit/` (1049 lines)
- wave 26: `cmd/gf/internal/utility/utils/` (660 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse.go` (48 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_convert.go` (533 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_do_commit.go` (21 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_do_delete.go` (16 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_do_filter.go` (347 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_do_insert.go` (374 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_do_update.go` (28 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_local_type.go` (204 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_open.go` (42 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_table_fields.go` (125 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_tables.go` (30 lines)
- wave 26: `contrib/drivers/clickhouse/clickhouse_transaction.go` (28 lines)
- wave 26: `contrib/drivers/dm/dm_do_commit.go` (94 lines)
- wave 26: `contrib/drivers/dm/dm_do_exec.go` (27 lines)
- wave 26: `contrib/drivers/dm/dm_do_filter.go` (245 lines)
- wave 26: `contrib/drivers/dm/dm_do_insert.go` (547 lines)
- wave 26: `contrib/drivers/dm/dm_local_type.go` (109 lines)
- wave 26: `contrib/drivers/dm/dm_open.go` (92 lines)
- wave 26: `contrib/drivers/dm/dm_table_fields.go` (200 lines)
- wave 26: `contrib/drivers/dm/dm_tables.go` (34 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb.go` (41 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_dialect.go` (22 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_do_exec.go` (85 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_do_filter.go` (55 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_do_insert.go` (629 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_local_type.go` (235 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_open.go` (64 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_table_fields.go` (100 lines)
- wave 26: `contrib/drivers/gaussdb/gaussdb_tables.go` (92 lines)
- wave 26: `contrib/drivers/mariadb/mariadb_table_fields.go` (76 lines)
- wave 26: `contrib/drivers/mssql/mssql.go` (45 lines)
- wave 26: `contrib/drivers/mssql/mssql_convert.go` (167 lines)
- wave 26: `contrib/drivers/mssql/mssql_do_exec.go` (203 lines)
- wave 26: `contrib/drivers/mssql/mssql_do_filter.go` (495 lines)
- wave 26: `contrib/drivers/mssql/mssql_do_insert.go` (311 lines)
- wave 26: `contrib/drivers/mssql/mssql_lock.go` (97 lines)
- wave 26: `contrib/drivers/mssql/mssql_open.go` (113 lines)
- wave 26: `contrib/drivers/mssql/mssql_table_fields.go` (237 lines)
- wave 26: `contrib/drivers/mssql/mssql_tables.go` (32 lines)
- wave 26: `contrib/drivers/mysql/mysql_do_filter.go` (16 lines)
- wave 26: `contrib/drivers/mysql/mysql_open.go` (55 lines)
- wave 26: `contrib/drivers/mysql/mysql_table_fields.go` (97 lines)
- wave 26: `contrib/drivers/mysql/mysql_tables.go` (29 lines)
- wave 26: `contrib/drivers/oracle/oracle.go` (39 lines)
- wave 26: `contrib/drivers/oracle/oracle_convert.go` (250 lines)
- wave 26: `contrib/drivers/oracle/oracle_do_commit.go` (25 lines)
- wave 26: `contrib/drivers/oracle/oracle_do_exec.go` (116 lines)
- wave 26: `contrib/drivers/oracle/oracle_do_filter.go` (467 lines)
- wave 26: `contrib/drivers/oracle/oracle_do_insert.go` (321 lines)
- wave 26: `contrib/drivers/oracle/oracle_open.go` (57 lines)
- wave 26: `contrib/drivers/oracle/oracle_table_fields.go` (174 lines)
- wave 26: `contrib/drivers/oracle/oracle_tables.go` (34 lines)
- wave 26: `contrib/drivers/pgsql/pgsql.go` (41 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_dialect.go` (23 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_do_exec.go` (85 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_do_filter.go` (51 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_do_insert.go` (78 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_format_upsert.go` (87 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_local_type.go` (205 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_open.go` (64 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_table_fields.go` (99 lines)
- wave 26: `contrib/drivers/pgsql/pgsql_tables.go` (91 lines)
- wave 26: `contrib/drivers/sqlite/sqlite.go` (41 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_do_filter.go` (225 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_do_insert.go` (63 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_format_upsert.go` (83 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_local_type.go` (50 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_open.go` (58 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_table_fields.go` (45 lines)
- wave 26: `contrib/drivers/sqlite/sqlite_tables.go` (34 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo.go` (43 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_do_filter.go` (225 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_do_insert.go` (63 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_format_upsert.go` (83 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_local_type.go` (50 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_open.go` (65 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_table_fields.go` (51 lines)
- wave 26: `contrib/drivers/sqlitecgo/sqlitecgo_tables.go` (34 lines)
- wave 26: `contrib/registry/consul/consul.go` (165 lines)
- wave 26: `contrib/registry/consul/consul_discovery.go` (79 lines)
- wave 26: `contrib/registry/consul/consul_watcher.go` (181 lines)
- wave 26: `contrib/registry/etcd/etcd_discovery.go` (55 lines)
- wave 26: `contrib/registry/etcd/etcd_registrar.go` (106 lines)
- wave 26: `contrib/registry/etcd/etcd_service.go` (57 lines)
- wave 26: `contrib/registry/etcd/etcd_watcher.go` (69 lines)
- wave 26: `contrib/registry/file/file.go` (36 lines)
- wave 26: `contrib/registry/file/file_registrar.go` (55 lines)
- wave 26: `contrib/registry/file/file_service.go` (57 lines)
- wave 26: `contrib/registry/file/file_watcher.go` (37 lines)
- wave 26: `contrib/registry/nacos/nacos_discovery.go` (82 lines)
- wave 26: `contrib/registry/nacos/nacos_register.go` (71 lines)
- wave 26: `contrib/registry/nacos/nacos_service.go` (50 lines)
- wave 26: `contrib/registry/nacos/nacos_watcher.go` (65 lines)
- wave 26: `contrib/registry/polaris/polaris_discovery.go` (139 lines)
- wave 26: `contrib/registry/polaris/polaris_registry.go` (104 lines)
- wave 26: `contrib/registry/polaris/polaris_service.go` (29 lines)
- wave 26: `contrib/registry/polaris/polaris_watcher.go` (166 lines)
- wave 26: `contrib/registry/zookeeper/zookeeper.go` (62 lines)
- wave 26: `contrib/registry/zookeeper/zookeeper_discovery.go` (92 lines)
- wave 26: `contrib/registry/zookeeper/zookeeper_func.go` (43 lines)
- wave 26: `contrib/registry/zookeeper/zookeeper_registrar.go` (131 lines)
- wave 26: `contrib/registry/zookeeper/zookeeper_watcher.go` (131 lines)
- wave 26: `net/ghttp/` (8808 lines)
- wave 26: `net/gsel/` (351 lines)
- wave 27: `contrib/rpc/grpcx/grpcx_grpc_client.go` (101 lines)
- wave 27: `net/gclient/` (2511 lines)
- wave 28: `contrib/sdk/httpclient/httpclient.go` (85 lines)
- wave 28: `contrib/sdk/httpclient/httpclient_config.go` (18 lines)
- wave 28: `frame/gins/` (536 lines)
- wave 29: `frame/g/` (270 lines)
- wave 30: `cmd/gf/internal/cmd/genctrl/` (1140 lines)
- wave 30: `cmd/gf/internal/cmd/gendao/` (1485 lines)
- wave 30: `cmd/gf/internal/cmd/genenums/` (215 lines)
- wave 30: `cmd/gf/internal/cmd/genpb/` (475 lines)
- wave 30: `cmd/gf/internal/cmd/genpbentity/` (558 lines)
- wave 30: `cmd/gf/internal/cmd/genservice/` (1048 lines)
- wave 30: `cmd/gf/internal/service/` (227 lines)
- wave 30: `contrib/config/apollo/apollo.go` (179 lines)
- wave 30: `contrib/config/consul/consul.go` (201 lines)
- wave 30: `contrib/config/kubecm/kubecm.go` (203 lines)
- wave 30: `contrib/config/nacos/nacos.go` (151 lines)
- wave 30: `contrib/config/polaris/polaris.go` (185 lines)
- wave 30: `contrib/drivers/dm/dm.go` (47 lines)
- wave 30: `contrib/drivers/gaussdb/gaussdb_convert.go` (224 lines)
- wave 30: `contrib/drivers/mysql/mysql.go` (46 lines)
- wave 30: `contrib/drivers/pgsql/pgsql_convert.go` (224 lines)
- wave 30: `contrib/metric/otelmetric/otelmetric_prometheus.go` (28 lines)
- wave 30: `contrib/registry/etcd/etcd.go` (149 lines)
- wave 30: `contrib/registry/file/file_discovery.go` (132 lines)
- wave 30: `contrib/registry/nacos/nacos.go` (161 lines)
- wave 30: `contrib/registry/polaris/polaris.go` (132 lines)
- wave 30: `contrib/rpc/grpcx/grpcx_grpc_server.go` (300 lines)
- wave 30: `contrib/rpc/grpcx/grpcx_grpc_server_config.go` (97 lines)
- wave 30: `contrib/rpc/grpcx/grpcx_registry_file.go` (32 lines)
- wave 30: `contrib/rpc/grpcx/internal/balancer/` (203 lines)
- wave 30: `contrib/rpc/grpcx/internal/grpcctx/` (105 lines)
- wave 30: `contrib/rpc/grpcx/internal/resolver/` (215 lines)
- wave 30: `contrib/sdk/httpclient/httpclient_handler.go` (61 lines)
- wave 30: `contrib/trace/otlpgrpc/otlpgrpc.go` (97 lines)
- wave 30: `contrib/trace/otlphttp/otlphttp.go` (94 lines)
- wave 31: `cmd/gf/internal/cmd/` (2612 lines)
- wave 31: `contrib/drivers/mariadb/mariadb.go` (54 lines)
- wave 31: `contrib/drivers/oceanbase/oceanbase.go` (43 lines)
- wave 31: `contrib/drivers/tidb/tidb.go` (54 lines)
- wave 31: `contrib/rpc/grpcx/grpcx.go` (34 lines)
- wave 31: `contrib/rpc/grpcx/internal/tracing/` (502 lines)
- wave 32: `cmd/gf/gfcmd/` (106 lines)
- wave 32: `contrib/rpc/grpcx/grpcx_interceptor_client.go` (43 lines)
- wave 32: `contrib/rpc/grpcx/grpcx_interceptor_server.go` (91 lines)
- wave 33: `cmd/gf/main.go` (26 lines)

### Decisions for the user

**D1** (shape): The app is a cli, entered through gf (cmd/gf). Port every entry point, in that shape?

> Yes: a clap front door (`app front-door`) takes every command over first

**D2** (dependency go.opentelemetry.io/otel): Which crate replaces go.opentelemetry.io/otel (used by 12 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel.md before porting what uses it

**D3** (dependency go.opentelemetry.io/otel/trace): Which crate replaces go.opentelemetry.io/otel/trace (used by 12 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-trace.md before porting what uses it

**D4** (dependency go.opentelemetry.io/otel/metric): Which crate replaces go.opentelemetry.io/otel/metric (used by 10 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-metric.md before porting what uses it

**D5** (dependency go.opentelemetry.io/otel/attribute): Which crate replaces go.opentelemetry.io/otel/attribute (used by 9 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-attribute.md before porting what uses it

**D6** (dependency go.opentelemetry.io/otel/propagation): Which crate replaces go.opentelemetry.io/otel/propagation (used by 8 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-propagation.md before porting what uses it

**D7** (dependency github.com/polarismesh/polaris-go): Which crate replaces github.com/polarismesh/polaris-go (used by 5 units)?

> Research it against primary sources into rust/research/github.com-polarismesh-polaris-go.md before porting what uses it

**D8** (dependency go.opentelemetry.io/otel/codes): Which crate replaces go.opentelemetry.io/otel/codes (used by 5 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-codes.md before porting what uses it

**D9** (dependency github.com/hashicorp/consul/api): Which crate replaces github.com/hashicorp/consul/api (used by 4 units)?

> Research it against primary sources into rust/research/github.com-hashicorp-consul-api.md before porting what uses it

**D10** (dependency github.com/nacos-group/nacos-sdk-go/v2/vo): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/vo (used by 4 units)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-vo.md before porting what uses it

**D11** (dependency github.com/olekukonko/tablewriter): Which crate replaces github.com/olekukonko/tablewriter (used by 4 units)?

> Research it against primary sources into rust/research/github.com-olekukonko-tablewriter.md before porting what uses it

**D12** (dependency github.com/olekukonko/tablewriter/renderer): Which crate replaces github.com/olekukonko/tablewriter/renderer (used by 4 units)?

> Research it against primary sources into rust/research/github.com-olekukonko-tablewriter-renderer.md before porting what uses it

**D13** (dependency github.com/olekukonko/tablewriter/tw): Which crate replaces github.com/olekukonko/tablewriter/tw (used by 4 units)?

> Research it against primary sources into rust/research/github.com-olekukonko-tablewriter-tw.md before porting what uses it

**D14** (dependency github.com/polarismesh/polaris-go/pkg/model): Which crate replaces github.com/polarismesh/polaris-go/pkg/model (used by 4 units)?

> Research it against primary sources into rust/research/github.com-polarismesh-polaris-go-pkg-model.md before porting what uses it

**D15** (dependency go.etcd.io/etcd/client/v3): Which crate replaces go.etcd.io/etcd/client/v3 (used by 4 units)?

> Research it against primary sources into rust/research/go.etcd.io-etcd-client-v3.md before porting what uses it

**D16** (dependency gitee.com/chunanyong/dm): Which crate replaces gitee.com/chunanyong/dm (used by 3 units)?

> Research it against primary sources into rust/research/gitee.com-chunanyong-dm.md before porting what uses it

**D17** (dependency github.com/go-zookeeper/zk): Which crate replaces github.com/go-zookeeper/zk (used by 3 units)?

> Research it against primary sources into rust/research/github.com-go-zookeeper-zk.md before porting what uses it

**D18** (dependency github.com/nacos-group/nacos-sdk-go/v2/model): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/model (used by 3 units)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-model.md before porting what uses it

**D19** (dependency github.com/sijms/go-ora/v2): Which crate replaces github.com/sijms/go-ora/v2 (used by 3 units)?

> Research it against primary sources into rust/research/github.com-sijms-go-ora-v2.md before porting what uses it

**D20** (dependency go.opentelemetry.io/otel/sdk/metric): Which crate replaces go.opentelemetry.io/otel/sdk/metric (used by 3 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk-metric.md before porting what uses it

**D21** (dependency go.opentelemetry.io/otel/sdk/resource): Which crate replaces go.opentelemetry.io/otel/sdk/resource (used by 3 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk-resource.md before porting what uses it

**D22** (dependency go.opentelemetry.io/otel/sdk/trace): Which crate replaces go.opentelemetry.io/otel/sdk/trace (used by 3 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk-trace.md before porting what uses it

**D23** (dependency go.opentelemetry.io/otel/semconv/v1.24.0): Which crate replaces go.opentelemetry.io/otel/semconv/v1.24.0 (used by 3 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-semconv-v1.24.0.md before porting what uses it

**D24** (dependency gitee.com/opengauss/openGauss-connector-go-pq): Which crate replaces gitee.com/opengauss/openGauss-connector-go-pq (used by 2 units)?

> Research it against primary sources into rust/research/gitee.com-opengauss-openGauss-connector-go-pq.md before porting what uses it

**D25** (dependency github.com/charmbracelet/lipgloss): Which crate replaces github.com/charmbracelet/lipgloss (used by 2 units)?

> Research it against primary sources into rust/research/github.com-charmbracelet-lipgloss.md before porting what uses it

**D26** (dependency github.com/gorilla/websocket): Which crate replaces github.com/gorilla/websocket (used by 2 units)?

> Research it against primary sources into rust/research/github.com-gorilla-websocket.md before porting what uses it

**D27** (dependency github.com/hashicorp/consul/api/watch): Which crate replaces github.com/hashicorp/consul/api/watch (used by 2 units)?

> Research it against primary sources into rust/research/github.com-hashicorp-consul-api-watch.md before porting what uses it

**D28** (dependency github.com/microsoft/go-mssqldb): Which crate replaces github.com/microsoft/go-mssqldb (used by 2 units)?

> Research it against primary sources into rust/research/github.com-microsoft-go-mssqldb.md before porting what uses it

**D29** (dependency github.com/nacos-group/nacos-sdk-go/v2/clients): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/clients (used by 2 units)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-clients.md before porting what uses it

**D30** (dependency github.com/nacos-group/nacos-sdk-go/v2/common/constant): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/common/constant (used by 2 units)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-common-constant.md before porting what uses it

**D31** (dependency go.opentelemetry.io/otel/baggage): Which crate replaces go.opentelemetry.io/otel/baggage (used by 2 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-baggage.md before porting what uses it

**D32** (dependency go.opentelemetry.io/otel/exporters/otlp/otlptrace): Which crate replaces go.opentelemetry.io/otel/exporters/otlp/otlptrace (used by 2 units)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-exporters-otlp-otlptrace.md before porting what uses it

**D33** (dependency golang.org/x/tools/go/packages): Which crate replaces golang.org/x/tools/go/packages (used by 2 units)?

> Research it against primary sources into rust/research/golang.org-x-tools-go-packages.md before porting what uses it

**D34** (dependency k8s.io/client-go/kubernetes): Which crate replaces k8s.io/client-go/kubernetes (used by 2 units)?

> Research it against primary sources into rust/research/k8s.io-client-go-kubernetes.md before porting what uses it

**D35** (dependency k8s.io/client-go/rest): Which crate replaces k8s.io/client-go/rest (used by 2 units)?

> Research it against primary sources into rust/research/k8s.io-client-go-rest.md before porting what uses it

**D36** (dependency github.com/ClickHouse/clickhouse-go/v2): Which crate replaces github.com/ClickHouse/clickhouse-go/v2 (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-ClickHouse-clickhouse-go-v2.md before porting what uses it

**D37** (dependency github.com/ClickHouse/clickhouse-go/v2/lib/driver): Which crate replaces github.com/ClickHouse/clickhouse-go/v2/lib/driver (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-ClickHouse-clickhouse-go-v2-lib-driver.md before porting what uses it

**D38** (dependency github.com/apolloconfig/agollo/v4): Which crate replaces github.com/apolloconfig/agollo/v4 (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-apolloconfig-agollo-v4.md before porting what uses it

**D39** (dependency github.com/apolloconfig/agollo/v4/env/config): Which crate replaces github.com/apolloconfig/agollo/v4/env/config (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-apolloconfig-agollo-v4-env-config.md before porting what uses it

**D40** (dependency github.com/apolloconfig/agollo/v4/storage): Which crate replaces github.com/apolloconfig/agollo/v4/storage (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-apolloconfig-agollo-v4-storage.md before porting what uses it

**D41** (dependency github.com/charmbracelet/bubbles/key): Which crate replaces github.com/charmbracelet/bubbles/key (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-charmbracelet-bubbles-key.md before porting what uses it

**D42** (dependency github.com/charmbracelet/bubbletea): Which crate replaces github.com/charmbracelet/bubbletea (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-charmbracelet-bubbletea.md before porting what uses it

**D43** (dependency github.com/charmbracelet/huh): Which crate replaces github.com/charmbracelet/huh (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-charmbracelet-huh.md before porting what uses it

**D44** (dependency github.com/clbanning/mxj/v2): Which crate replaces github.com/clbanning/mxj/v2 (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-clbanning-mxj-v2.md before porting what uses it

**D45** (dependency github.com/emirpasic/gods/v2/trees/avltree): Which crate replaces github.com/emirpasic/gods/v2/trees/avltree (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-emirpasic-gods-v2-trees-avltree.md before porting what uses it

**D46** (dependency github.com/emirpasic/gods/v2/trees/btree): Which crate replaces github.com/emirpasic/gods/v2/trees/btree (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-emirpasic-gods-v2-trees-btree.md before porting what uses it

**D47** (dependency github.com/emirpasic/gods/v2/trees/redblacktree): Which crate replaces github.com/emirpasic/gods/v2/trees/redblacktree (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-emirpasic-gods-v2-trees-redblacktree.md before porting what uses it

**D48** (dependency github.com/fatih/color): Which crate replaces github.com/fatih/color (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-fatih-color.md before porting what uses it

**D49** (dependency github.com/fsnotify/fsnotify): Which crate replaces github.com/fsnotify/fsnotify (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-fsnotify-fsnotify.md before porting what uses it

**D50** (dependency github.com/glebarez/go-sqlite): Which crate replaces github.com/glebarez/go-sqlite (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-glebarez-go-sqlite.md before porting what uses it

**D51** (dependency github.com/go-sql-driver/mysql): Which crate replaces github.com/go-sql-driver/mysql (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-go-sql-driver-mysql.md before porting what uses it

**D52** (dependency github.com/gogf/selfupdate): Which crate replaces github.com/gogf/selfupdate (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-gogf-selfupdate.md before porting what uses it

**D53** (dependency github.com/grokify/html-strip-tags-go): Which crate replaces github.com/grokify/html-strip-tags-go (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-grokify-html-strip-tags-go.md before porting what uses it

**D54** (dependency github.com/magiconair/properties): Which crate replaces github.com/magiconair/properties (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-magiconair-properties.md before porting what uses it

**D55** (dependency github.com/nacos-group/nacos-sdk-go/v2/clients/config_client): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/clients/config_client (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-clients-config_client.md before porting what uses it

**D56** (dependency github.com/nacos-group/nacos-sdk-go/v2/clients/naming_client): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2/clients/naming_client (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2-clients-naming_client.md before porting what uses it

**D57** (dependency github.com/polarismesh/polaris-go/api): Which crate replaces github.com/polarismesh/polaris-go/api (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-polarismesh-polaris-go-api.md before porting what uses it

**D58** (dependency github.com/polarismesh/polaris-go/pkg/config): Which crate replaces github.com/polarismesh/polaris-go/pkg/config (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-polarismesh-polaris-go-pkg-config.md before porting what uses it

**D59** (dependency github.com/schollz/progressbar/v3): Which crate replaces github.com/schollz/progressbar/v3 (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-schollz-progressbar-v3.md before porting what uses it

**D60** (dependency github.com/shopspring/decimal): Which crate replaces github.com/shopspring/decimal (used by 1 unit)?

> Research it against primary sources into rust/research/github.com-shopspring-decimal.md before porting what uses it

**D61** (dependency go.opentelemetry.io/contrib/instrumentation/runtime): Which crate replaces go.opentelemetry.io/contrib/instrumentation/runtime (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-contrib-instrumentation-runtime.md before porting what uses it

**D62** (dependency go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc): Which crate replaces go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-exporters-otlp-otlptrace-otlptracegrpc.md before porting what uses it

**D63** (dependency go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp): Which crate replaces go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-exporters-otlp-otlptrace-otlptracehttp.md before porting what uses it

**D64** (dependency go.opentelemetry.io/otel/sdk/instrumentation): Which crate replaces go.opentelemetry.io/otel/sdk/instrumentation (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk-instrumentation.md before porting what uses it

**D65** (dependency go.opentelemetry.io/otel/sdk/metric/exemplar): Which crate replaces go.opentelemetry.io/otel/sdk/metric/exemplar (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk-metric-exemplar.md before porting what uses it

**D66** (dependency go.opentelemetry.io/otel/semconv/v1.18.0): Which crate replaces go.opentelemetry.io/otel/semconv/v1.18.0 (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-semconv-v1.18.0.md before porting what uses it

**D67** (dependency go.opentelemetry.io/otel/semconv/v1.7.0): Which crate replaces go.opentelemetry.io/otel/semconv/v1.7.0 (used by 1 unit)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-semconv-v1.7.0.md before porting what uses it

**D68** (dependency go.yaml.in/yaml/v3): Which crate replaces go.yaml.in/yaml/v3 (used by 1 unit)?

> Research it against primary sources into rust/research/go.yaml.in-yaml-v3.md before porting what uses it

**D69** (dependency golang.org/x/mod/modfile): Which crate replaces golang.org/x/mod/modfile (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-mod-modfile.md before porting what uses it

**D70** (dependency golang.org/x/mod/semver): Which crate replaces golang.org/x/mod/semver (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-mod-semver.md before porting what uses it

**D71** (dependency golang.org/x/net/proxy): Which crate replaces golang.org/x/net/proxy (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-net-proxy.md before porting what uses it

**D72** (dependency golang.org/x/tools/go/ast/astutil): Which crate replaces golang.org/x/tools/go/ast/astutil (used by 1 unit)?

> Research it against primary sources into rust/research/golang.org-x-tools-go-ast-astutil.md before porting what uses it

**D73** (dependency k8s.io/apimachinery/pkg/apis/meta/v1): Which crate replaces k8s.io/apimachinery/pkg/apis/meta/v1 (used by 1 unit)?

> Research it against primary sources into rust/research/k8s.io-apimachinery-pkg-apis-meta-v1.md before porting what uses it

**D74** (dependency k8s.io/apimachinery/pkg/watch): Which crate replaces k8s.io/apimachinery/pkg/watch (used by 1 unit)?

> Research it against primary sources into rust/research/k8s.io-apimachinery-pkg-watch.md before porting what uses it

**D75** (dependency k8s.io/client-go/tools/clientcmd): Which crate replaces k8s.io/client-go/tools/clientcmd (used by 1 unit)?

> Research it against primary sources into rust/research/k8s.io-client-go-tools-clientcmd.md before porting what uses it

**D76** (dependency modernc.org/sqlite/lib): Which crate replaces modernc.org/sqlite/lib (used by 1 unit)?

> Research it against primary sources into rust/research/modernc.org-sqlite-lib.md before porting what uses it

**D77** (dependency github.com/charmbracelet/bubbles): Which crate replaces github.com/charmbracelet/bubbles (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-charmbracelet-bubbles.md before porting what uses it

**D78** (dependency github.com/emirpasic/gods/v2): Which crate replaces github.com/emirpasic/gods/v2 (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-emirpasic-gods-v2.md before porting what uses it

**D79** (dependency github.com/hashicorp/go-cleanhttp): Which crate replaces github.com/hashicorp/go-cleanhttp (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-hashicorp-go-cleanhttp.md before porting what uses it

**D80** (dependency github.com/nacos-group/nacos-sdk-go/v2): Which crate replaces github.com/nacos-group/nacos-sdk-go/v2 (declared, no unit imports it)?

> Research it against primary sources into rust/research/github.com-nacos-group-nacos-sdk-go-v2.md before porting what uses it

**D81** (dependency go.opentelemetry.io/otel/exporters/prometheus): Which crate replaces go.opentelemetry.io/otel/exporters/prometheus (declared, no unit imports it)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-exporters-prometheus.md before porting what uses it

**D82** (dependency go.opentelemetry.io/otel/sdk): Which crate replaces go.opentelemetry.io/otel/sdk (declared, no unit imports it)?

> Research it against primary sources into rust/research/go.opentelemetry.io-otel-sdk.md before porting what uses it

**D83** (dependency golang.org/x/mod): Which crate replaces golang.org/x/mod (declared, no unit imports it)?

> Research it against primary sources into rust/research/golang.org-x-mod.md before porting what uses it

**D84** (dependency golang.org/x/net): Which crate replaces golang.org/x/net (declared, no unit imports it)?

> Research it against primary sources into rust/research/golang.org-x-net.md before porting what uses it

**D85** (dependency golang.org/x/tools): Which crate replaces golang.org/x/tools (declared, no unit imports it)?

> Research it against primary sources into rust/research/golang.org-x-tools.md before porting what uses it

**D86** (dependency k8s.io/api): Which crate replaces k8s.io/api (declared, no unit imports it)?

> Research it against primary sources into rust/research/k8s.io-api.md before porting what uses it

**D87** (dependency k8s.io/apimachinery): Which crate replaces k8s.io/apimachinery (declared, no unit imports it)?

> Research it against primary sources into rust/research/k8s.io-apimachinery.md before porting what uses it

**D88** (dependency k8s.io/client-go): Which crate replaces k8s.io/client-go (declared, no unit imports it)?

> Research it against primary sources into rust/research/k8s.io-client-go.md before porting what uses it

**D89** (dependency modernc.org/sqlite): Which crate replaces modernc.org/sqlite (declared, no unit imports it)?

> Research it against primary sources into rust/research/modernc.org-sqlite.md before porting what uses it

**D90** (dependency golang.org/x/sync): golang.org/x/sync maps to tokio, rayon only in part (used by 2 units). Use it?

> Use tokio, rayon, and record cases for the behavior it does not cover

**D91** (dependency golang.org/x/text): golang.org/x/text maps to unicode-normalization, icu only in part (used by 2 units). Use it?

> Use unicode-normalization, icu, and record cases for the behavior it does not cover

