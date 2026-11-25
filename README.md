# gf-rust

GoFrame 的 Rust 版本。

[GoFrame](https://github.com/gogf/gf)（`github.com/gogf/gf/v2`）是一套成熟的 Go
应用开发框架，提供容器、编码、文本处理、错误、OS 工具、校验、网络等基础能力。
`gf-rust` 把这些能力按 Go 包逐个移植到 Rust：**同名模块、同样的 API 语义、
同样的输出格式**，并把 GoFrame 的单元测试翻译成 Rust 测试，作为行为等价的验收标准。

- 跟踪版本：GoFrame `v2.10.5`
- 语言/工具链：Rust 2024 edition，最低 1.88
- 默认只依赖通用开源 crate（`serde`、`chrono`、`regex`、`zip`、`notify` 等），不引入框架级运行时

## 设计原则

1. **一个 Go 包对应一个 Rust 模块**。`container/garray` 是
   `gf_container::garray`，`os/gfile` 是 `gf_os::gfile`；`gf` 这个 facade crate
   再按 Go 的包路径重新导出（`gf::os::gfile`、`gf::encoding::gjson`）。
2. **`any` 由统一的 `Value` 承担**，`gvar::Var` 在其上提供 GoFrame 风格的取值方法
   （`string()`、`int64()`、`map()` 等）。
3. **错误统一为 `gerror::Error`**，带 `gcode` 错误码与堆栈语义，与 Go 侧
   `gerror.NewCode`/`Wrap` 对应。
4. **行为对齐优先**。格式化、JSON 转义、路径处理、大小计算、日志布局、配置查找
   顺序等细节都以 Go 实现与测试为准，而不是"看起来差不多"。
5. **无 `unsafe`**（`os/genv` 的少量平台调用除外），公共 API 全量文档注释。

## 功能

| 领域 | 内容 |
|---|---|
| 容器 | `garray`（数组/有序数组）、`glist`、`gmap`（并发 map）、`gset`、`gtree`（红黑树/BKV 树）、`gqueue`、`gring`、`gpool`（带 TTL 的对象池）、`gtype`、`gvar` |
| 编码 | `gjson`（JSON/YAML/TOML/INI/XML/properties 互转与路径取值）、`gxml`、`gyaml`、`gtoml`、`gini`、`gproperties`、`gbase64`、`gcompress`（gzip/zlib/zip）、`gbinary`、`gcharset`、`ghash`、`ghtml`、`gurl` |
| 文本 | `gstr`（与 GoFrame 一致的字符串工具）、`gregex` |
| 转换与工具 | `gconv`（结构体/标量/容器互转）、`gutil`、`gpage`、`guid`、`grand`、`gmode`、`gtag` |
| 校验 | `gvalid`：全部 67 条内置规则，map/value/tag 序列车道 |
| 错误 | `gerror`、`gcode`（错误码、包装、堆栈过滤） |
| DB | `gredis`（RESP2/RESP3 客户端、Sentinel 主节点发现、Cluster 槽路由/副本读、ACL 认证、连接池：字符串/哈希/列表/集合/有序集合/PUBLISH、pipeline、MULTI/EXEC 事务、PUBLISH 订阅、EVAL/SCRIPT 脚本）、`gdb` 值层、SQL 构建器、SQLite 执行与 Model（Record/Result/SqlResult、Where/Order/Group/Limit、Select/Insert/Update/Delete 渲染、Db 查询/写入、链式 Model、事务、连接池、`#[derive(FromRecord)]` 结构体扫描/`#[derive(ToRecord)]` 写入、表字段内省、自动时间字段/软删除、Model 结果缓存、聚合函数、分页 `chunk`、表分片） |
| NET | `gipv4`、`gipv6`、`gtrace`（追踪载体/显式追踪上下文/内容裁剪）、`gudp`（UDP 连接与服务端）、`gtcp`（TCP 连接、包协议、连接池、服务端）、`gsvc`（服务定义/注册接口）、`gsel`（负载均衡选择器）、`ghttp/internal/response`（响应写入器抽象）、`ghttp` HTTP/1.1 消息模型与路由/服务端（Header/Cookie/Query/Request/Response、Router、Group（verb 绑定/ALL/多方法）、中间件、Server、静态文件服务（ETag/Last-Modified/304）、请求客户端 IP/URL/AJAX 辅助、重定向、路由表列出、请求体大小限制、全局/按路由 hooks）、keep-alive/状态处理、Cookie 与会话、OpenAPI/Swagger 服务、HTTPS 服务端、CORS、gzip 压缩、JSON body/统一响应中间件、W3C 链路追踪、`gtcp` TLS 客户端/服务端、`gclient`（HTTP/1.1/HTTPS 客户端 + 重定向）、`goai`（OpenAPI 3.0 文档模型） |
| I18N | `i18n/gi18n`（文件/打包资源加载、自动/手动重载、Accept-Language 协商、语言目录、`{#key}` 翻译、格式动词、命名实例） |
| Test | `test/gtest`（断言助手） |
| Frame | `frame/g` 门面（server/tcp_server/udp_server/client/log/cfg/i18n/view/validator、常用类型别名）与 `frame/gins` 实例容器 |
| OS | `gtime`、`genv`、`gbuild`、`gmutex`、`gfile`（含缓存车道）、`gspath`（直查 + 索引缓存）、`gfsnotify`（基于 `notify`）、`gres`（资源打包/解包）、`gcfg`（内容/文件适配器、watcher、泛型 Loader）、`glog`（处理器链、JSON/结构化输出、文件轮转）、`gcmd`（选项解析、命令树、帮助、`#[derive(CommandObject)]` 结构体车道）、`gcache`（TTL/LRU 缓存、适配器抽象）、`gmlock`（按 key 的读写锁）、`grpool`（可限流工作池）、`gfpool`（文件句柄池）、`gtimer`（tick 定时器）、`gcron`（crontab 定时任务）、`gsession`（会话管理、内存/文件存储）、`gmetric`（指标 API，OTel 形态）、`gproc`（shell 执行、进程信息、进程管理器）、`gview`（模板引擎，内置 Go 模板子集 + define/template/block、printf/len/index/slice 等标准内建、变量赋值、i18n 结果翻译） |
| 网络 | `gipv4`、`gipv6` |
| 调试 | `gdebug`（调用点、堆栈、版本信息） |
| 加密 | `gaes`、`gdes`、`grsa`、`gmd5`、`gsha1`、`gsha256`、`gcrc32` |
| 框架内部 | `consts`、`command`、`empty`、`deepcopy`、`fileinfo`、`intlog`、`utils`、`internal/json` 等 |

## crate 结构

| crate | 对应 GoFrame 路径 | 说明 |
|---|---|---|
| `gf` | 全部 | facade，按 Go 包路径重新导出 |
| `gf-core` | — | `Value`、浮点格式化、版本号 |
| `gf-container` | `container/*` | `garray`、`gmap`、`gtree` 等 |
| `gf-encoding` | `encoding/*` | `gjson`、`gxml`、`gcompress` 等 |
| `gf-json` | `internal/json` | 与 Go `encoding/json` 逐字节一致的编解码 |
| `gf-text` | `text/*` | `gstr`、`gregex` |
| `gf-util` | `util/*` | `gconv`、`gpage`、`guid`、`gutil` 等 |
| `gf-valid` | `util/gvalid`、`gf-valid-derive` | 校验器、内置规则与 `#[derive(Validate)]`（独立 crate，避免依赖环） |
| `gf-errors` | `errors/*` | `gerror`、`gcode` |
| `gf-internal` | `internal/*` | 框架内部工具 |
| `gf-debug` | `debug/gdebug` | 调用点与堆栈 |
| `gf-crypto` | `crypto/*` | 对称/非对称加密与摘要 |
| `gf-database` | `database/gredis`、`database/gdb`（值层 + SQL 构建器 + SQLite 执行） | Redis RESP2/RESP3 客户端、DB 行类型、SQL 渲染与执行 |
| `gf-net` | `net/gipv4`、`net/gipv6`、`net/gtrace`、`net/gudp`、`net/gtcp`、`net/gsvc`、`net/gsel`、`net/goai`、`net/ghttp/internal/response` | IP、追踪原语、UDP、TCP、服务注册/负载均衡、OpenAPI、HTTP 响应写入器 |
| `gf-os` | `os/*` | 时间、文件、配置、日志、命令等 |
| `gf-bench` | — | Go vs Rust 基准对比程序 |

## 快速开始

在 `Cargo.toml` 中按路径引入 facade：

```toml
[dependencies]
gf = { path = "crates/gf" }
```

```rust
use gf::Value;
use gf::container::gvar::Var;

fn main() {
    // gvar：统一的取值入口
    let value = Var::new(Value::String("goframe".into()));
    assert_eq!(value.string(), "goframe");

    // 编码：按内容自动识别 JSON/YAML/TOML/INI/XML
    let json = gf::encoding::gjson::Json::new_from(br#"{"app":{"name":"gf-rust","port":8080}}"#)
        .expect("valid content");
    assert_eq!(json.get_string("app.name"), "gf-rust");
    assert_eq!(json.get_int64("app.port"), 8080);

    // 文件：与 GoFrame 一致的内容/路径/扫描/复制/替换工具
    let path = gf::os::gfile::join(&["config", "config.toml"]);
    if gf::os::gfile::exists(&path) {
        let content = gf::os::gfile::get_contents(&path);
        println!("{content}");
    }

    // 日志：级别、前缀、文件输出与轮转
    let logger = gf::os::glog::Logger::new();
    logger.set_stdout_print(true);
    logger.info("service started");
}
```

配置读取（文件缺失时不报错，按 GoFrame 的搜索路径查找 `config.toml` 等）：

```rust
use gf::os::gcfg;

let config = gcfg::instance(None).expect("config instance");
if config.available(None) {
    let name = config.get("app.name").expect("read config");
    println!("{}", name.map(|value| value.string()).unwrap_or_default());
}
```

命令行：

```rust
use std::sync::Arc;
use gf::os::gcmd::Command;

let root = Arc::new(Command::new("app"));
let mut serve = Command::new("serve");
serve.run = Some(Arc::new(|parser| {
    let port = parser
        .get_opt("port", None)
        .map(|value| value.int64())
        .unwrap_or(8080);
    println!("serving on {port}");
    Ok(())
}));
root.add_command(Arc::new(serve)).expect("add command");
root.run();
```

## 构建与测试

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

当前状态：工作区全部测试通过（**1034** 个测试），格式与 clippy 检查干净。

## 尚未移植

- **`database/gdb`**：MySQL/PostgreSQL 等驱动（当前提供 SQLite 执行、链式 Model、
  事务、连接池与 `#[derive(FromRecord)]` 结构体扫描/`#[derive(ToRecord)]` 写入、表字段内省、自动时间字段/软删除、Model 结果缓存、聚合函数、分页 `chunk`、表分片）、分库与数据库级分片。
- **`net/gredis`**：集群副本的轮询选择与读写隔离（RESP2/RESP3、Sentinel、Cluster 槽路由/副本读、连接池、pipeline、MULTI/EXEC、订阅与脚本已移植）。
- **`net/ghttp`**：graceful restart（跨进程 fd 传递，Windows 不适用）。
- **`i18n/gi18n`**：context 语言（改用 `*_with_language`）与 gres 监控；打包资源（ADR-101）与自动重载（ADR-115）已移植。
- **`os/gctx`、`os/gstructs`**：按 ADR-052 记录为不移植；`gcmd` 结构体车道只覆盖
  字段映射（方法自动成为子命令需要反射，未移植）。
- **Go 专属能力**：`gfile.MainPkgPath`、`glog` 的 `log/slog` 桥、
  `gres.PackToGoFile` 等不移植。

## 与 GoFrame 的差异

移植以"语义等价"为目标，以下差异是刻意的：

- **不使用 `context.Context`**。Go 侧用于传递日志字段、解析器、配置操作上下文的
  context，在 Rust 侧改为显式参数或结构体（如 `gcfg::OperationContext`）。
- **`any` 统一为 `Value`**，类型判断与转换走 `gconv`，避免运行期反射。
- **结构体校验走派生宏**：`#[derive(Validate)]` + `#[gf(rule = "...")]`（ADR-092），
  替代 Go 的 struct tag；`gcmd` 的结构体对象车道改为 `#[derive(CommandObject)]`（ADR-108），\n  方法自动注册子命令未移植。
- **Go 专属能力不移植**：`gfile.MainPkgPath`（依赖 Go 运行时帧）、`glog` 的
  `log/slog` 桥、`gres.PackToGoFile`（生成 Go 源码）等。
- 文件句柄、定时器等宿主能力使用 Rust 生态实现（如 `notify` 替代 `fsnotify`），
  对外 API 与行为保持一致。

## 许可证

MIT，与 [GoFrame](https://github.com/gogf/gf) 相同。GoFrame 的包结构、API 设计与
文档是本项目的基础，特此致谢。
