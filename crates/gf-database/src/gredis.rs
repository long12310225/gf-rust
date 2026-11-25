//! Port of `database/gredis` — a minimal RESP2 Redis client.
//!
//! GoFrame's `gredis` wraps `go-redis` (a full-featured cluster/sentinel
//! client). This port implements the RESP2 protocol over [`gf_net::gtcp`]
//! directly and exposes the core command groups (ADR-070):
//!
//! - [`Reply`] decoding for simple strings, errors, integers, bulk strings,
//!   nil and (nested) arrays;
//! - [`Client`] with `AUTH`/`SELECT` on connect plus PING, string, hash, list,
//!   set, sorted-set and PUBLISH commands;
//! - [`Config`] with address, password, database and timeout.
//!
//! Beyond the initial RESP2 core (ADR-070), the port now covers pipelines and
//! transactions (ADR-097), pub/sub (ADR-098), scripting (ADR-099), RESP3
//! replies and `HELLO` (ADR-111), Sentinel master discovery (ADR-114) and a
//! slot-routing cluster client (ADR-116).
//!
//! Not ported: automatic protocol negotiation, ASK-migrating slot semantics
//! (redirections refresh the slot table), cluster replicas/read splitting and
//! the full `go-redis` option surface.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_net::gtcp::Conn;

/// The default client timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// The default Redis address.
pub const DEFAULT_ADDRESS: &str = "127.0.0.1:6379";

/// The client configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// The server address (`host:port`).
    pub address: String,
    /// The password, when set.
    pub password: String,
    /// The database index (0 keeps the default).
    pub db: i32,
    /// The dial/read timeout.
    pub timeout: Duration,
    /// The Redis Sentinel master name (empty disables sentinel mode).
    pub master_name: String,
    /// The username used to authenticate against sentinels.
    pub sentinel_user: String,
    /// The password used to authenticate against sentinels.
    pub sentinel_pass: String,
    /// Enables Redis Cluster slot routing (ADR-116).
    pub cluster: bool,
    /// The minimum number of idle pooled connections.
    pub min_idle: usize,
    /// The maximum number of idle pooled connections.
    pub max_idle: usize,
    /// The maximum number of open connections (0 = unlimited).
    pub max_active: usize,
    /// The maximum lifetime of a pooled connection.
    pub max_conn_lifetime: Duration,
    /// The maximum idle time of a pooled connection.
    pub idle_timeout: Duration,
    /// How long `get` waits for a free connection (0 = no wait).
    pub wait_timeout: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            address: DEFAULT_ADDRESS.to_owned(),
            password: String::new(),
            db: 0,
            timeout: DEFAULT_TIMEOUT,
            master_name: String::new(),
            sentinel_user: String::new(),
            sentinel_pass: String::new(),
            cluster: false,
            min_idle: 0,
            max_idle: 10,
            max_active: 0,
            max_conn_lifetime: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(10),
            wait_timeout: Duration::ZERO,
        }
    }
}

impl Config {
    /// Creates a configuration for `address`.
    pub fn new(address: &str) -> Config {
        Config {
            address: address.to_owned(),
            ..Config::default()
        }
    }

    /// Sets the password.
    pub fn with_password(mut self, password: &str) -> Config {
        self.password = password.to_owned();
        self
    }

    /// Sets the database index.
    pub fn with_db(mut self, db: i32) -> Config {
        self.db = db;
        self
    }

    /// Sets the timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Config {
        self.timeout = timeout;
        self
    }

    /// Enables Sentinel mode with `master_name`; `address` then holds one or
    /// more comma-separated sentinels (ADR-114).
    pub fn with_master_name(mut self, master_name: &str) -> Config {
        self.master_name = master_name.to_owned();
        self
    }

    /// Sets the credentials used to authenticate against sentinels.
    pub fn with_sentinel_auth(mut self, user: &str, password: &str) -> Config {
        self.sentinel_user = user.to_owned();
        self.sentinel_pass = password.to_owned();
        self
    }

    /// Enables Redis Cluster mode; `address` holds comma-separated nodes.
    pub fn with_cluster(mut self, cluster: bool) -> Config {
        self.cluster = cluster;
        self
    }

    /// Sets the pool sizes, like GoFrame's `MinIdle`/`MaxIdle`/`MaxActive`.
    pub fn with_pool(mut self, min_idle: usize, max_idle: usize, max_active: usize) -> Config {
        self.min_idle = min_idle;
        self.max_idle = max_idle;
        self.max_active = max_active;
        self
    }

    /// Sets the connection lifetime and idle timeout.
    pub fn with_pool_timeouts(mut self, lifetime: Duration, idle: Duration) -> Config {
        self.max_conn_lifetime = lifetime;
        self.idle_timeout = idle;
        self
    }

    /// Sets how long `RedisPool::get` waits for a free connection.
    pub fn with_wait_timeout(mut self, wait: Duration) -> Config {
        self.wait_timeout = wait;
        self
    }
}

/// One RESP2 reply, like GoFrame's `gredis` call result.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// A `+` simple string.
    Simple(String),
    /// A `-` error.
    Error(String),
    /// A `:` integer.
    Integer(i64),
    /// A `$` bulk string; `None` is the nil bulk.
    Bulk(Option<Vec<u8>>),
    /// A `*` array; `None` is the nil array.
    Array(Option<Vec<Reply>>),
    /// A `%` map (RESP3), stored as key/value reply pairs.
    Map(Vec<(Reply, Reply)>),
    /// A `~` set (RESP3); `None` is the nil set.
    Set(Option<Vec<Reply>>),
    /// A `,` double (RESP3).
    Double(f64),
    /// A `#` boolean (RESP3).
    Boolean(bool),
    /// A `(` big number (RESP3), kept as text.
    BigNumber(String),
    /// A `>` push message (RESP3).
    Push(Vec<Reply>),
    /// A `_` null (RESP3).
    Null,
}

impl Reply {
    /// Reports whether this is an error reply.
    pub fn is_error(&self) -> bool {
        matches!(self, Reply::Error(_))
    }

    /// The error text, when this is an error reply.
    pub fn error_text(&self) -> Option<&str> {
        match self {
            Reply::Error(message) => Some(message),
            _ => None,
        }
    }

    /// Converts the reply into a string (`""` for nil).
    pub fn string(&self) -> String {
        match self {
            Reply::Simple(value) => value.clone(),
            Reply::Error(value) => value.clone(),
            Reply::Integer(value) => value.to_string(),
            Reply::Bulk(value) => value
                .as_ref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default(),
            Reply::Array(_) => String::new(),
            Reply::Map(_) => String::new(),
            Reply::Set(items) => items
                .as_ref()
                .map(|items| {
                    items
                        .iter()
                        .map(Reply::string)
                        .collect::<Vec<String>>()
                        .join(",")
                })
                .unwrap_or_default(),
            Reply::Double(value) => gf_core::float::format_g(*value),
            Reply::Boolean(value) => value.to_string(),
            Reply::BigNumber(value) => value.clone(),
            Reply::Push(items) => items
                .iter()
                .map(Reply::string)
                .collect::<Vec<_>>()
                .join(","),
            Reply::Null => String::new(),
        }
    }

    /// Returns the bulk value, or `None` for nil/non-bulk replies.
    pub fn bulk(&self) -> Option<String> {
        match self {
            Reply::Bulk(Some(bytes)) => Some(String::from_utf8_lossy(bytes).into_owned()),
            Reply::Double(value) => Some(gf_core::float::format_g(*value)),
            Reply::Boolean(value) => Some(value.to_string()),
            Reply::BigNumber(value) => Some(value.clone()),
            Reply::Set(items) => items.as_ref().map(|items| {
                items
                    .iter()
                    .map(Reply::string)
                    .collect::<Vec<String>>()
                    .join(",")
            }),
            _ => None,
        }
    }

    /// Converts the reply into an integer (0 when not an integer/bulk number).
    pub fn int(&self) -> i64 {
        match self {
            Reply::Integer(value) => *value,
            Reply::Simple(value) => value.parse::<i64>().unwrap_or(0),
            Reply::Bulk(Some(bytes)) => String::from_utf8_lossy(bytes).parse::<i64>().unwrap_or(0),
            Reply::Double(value) => *value as i64,
            Reply::Boolean(value) => i64::from(*value),
            Reply::BigNumber(value) => value.parse::<i64>().unwrap_or(0),
            _ => 0,
        }
    }

    /// Converts an array reply to strings (strings are empty on nil).
    pub fn strings(&self) -> Vec<String> {
        match self {
            Reply::Array(Some(items)) => items.iter().map(Reply::string).collect(),
            Reply::Set(Some(items)) => items.iter().map(Reply::string).collect(),
            _ => Vec::new(),
        }
    }

    /// Converts a flat array reply to `(field, value)` pairs.
    pub fn pairs(&self) -> Vec<(String, String)> {
        if let Reply::Map(entries) = self {
            return entries
                .iter()
                .map(|(key, value)| (key.string(), value.string()))
                .collect();
        }
        let items = self.strings();
        items
            .chunks(2)
            .filter(|chunk| chunk.len() == 2)
            .map(|chunk| (chunk[0].clone(), chunk[1].clone()))
            .collect()
    }
}

/// Encodes one command as a RESP array of bulk strings.
pub fn encode_command(args: &[&str]) -> Vec<u8> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
    for argument in args {
        buffer.extend_from_slice(format!("${}\r\n", argument.len()).as_bytes());
        buffer.extend_from_slice(argument.as_bytes());
        buffer.extend_from_slice(b"\r\n");
    }
    buffer
}

/// The result of a decode attempt.
enum DecodeAttempt {
    /// A complete reply and the consumed byte count.
    Complete(Reply, usize),
    /// The buffer does not hold a complete reply yet.
    Incomplete,
}

/// Decodes one reply from `buffer`, returning it and the consumed bytes.
pub fn decode_reply(buffer: &[u8]) -> Result<(Reply, usize), Error> {
    match try_decode(buffer, 0) {
        Ok(DecodeAttempt::Complete(reply, consumed)) => Ok((reply, consumed)),
        Ok(DecodeAttempt::Incomplete) => Err(gerror::new("incomplete RESP reply")),
        Err(error) => Err(error),
    }
}

/// Parses one reply starting at `position`.
fn try_decode(buffer: &[u8], position: usize) -> Result<DecodeAttempt, Error> {
    let Some(prefix) = buffer.get(position) else {
        return Ok(DecodeAttempt::Incomplete);
    };
    match prefix {
        b'+' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            Ok(DecodeAttempt::Complete(
                Reply::Simple(line),
                next - position,
            ))
        }
        b'-' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            Ok(DecodeAttempt::Complete(Reply::Error(line), next - position))
        }
        b':' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let value = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP integer \"{line}\"")))?;
            Ok(DecodeAttempt::Complete(
                Reply::Integer(value),
                next - position,
            ))
        }
        b'$' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let length = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP bulk length \"{line}\"")))?;
            if length < 0 {
                return Ok(DecodeAttempt::Complete(Reply::Bulk(None), next - position));
            }
            let length = length as usize;
            let end = next + length;
            if buffer.len() < end + 2 {
                return Ok(DecodeAttempt::Incomplete);
            }
            let value = buffer[next..end].to_vec();
            Ok(DecodeAttempt::Complete(
                Reply::Bulk(Some(value)),
                end + 2 - position,
            ))
        }
        b'*' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let count = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP array length \"{line}\"")))?;
            if count < 0 {
                return Ok(DecodeAttempt::Complete(Reply::Array(None), next - position));
            }
            let mut items = Vec::with_capacity(count as usize);
            let mut cursor = next;
            for _ in 0..count {
                match try_decode(buffer, cursor)? {
                    DecodeAttempt::Incomplete => return Ok(DecodeAttempt::Incomplete),
                    DecodeAttempt::Complete(item, consumed) => {
                        items.push(item);
                        cursor += consumed;
                    }
                }
            }
            Ok(DecodeAttempt::Complete(
                Reply::Array(Some(items)),
                cursor - position,
            ))
        }
        b'_' => {
            let Some((_line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            Ok(DecodeAttempt::Complete(Reply::Null, next - position))
        }
        b',' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let value = line
                .parse::<f64>()
                .map_err(|_| gerror::new(format!("invalid RESP double \"{line}\"")))?;
            Ok(DecodeAttempt::Complete(
                Reply::Double(value),
                next - position,
            ))
        }
        b'#' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            match line.as_str() {
                "t" => Ok(DecodeAttempt::Complete(
                    Reply::Boolean(true),
                    next - position,
                )),
                "f" => Ok(DecodeAttempt::Complete(
                    Reply::Boolean(false),
                    next - position,
                )),
                _ => Err(gerror::new(format!("invalid RESP boolean \"{line}\""))),
            }
        }
        b'(' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            Ok(DecodeAttempt::Complete(
                Reply::BigNumber(line),
                next - position,
            ))
        }
        b'%' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let count = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP map length \"{line}\"")))?;
            if count < 0 {
                return Ok(DecodeAttempt::Complete(
                    Reply::Map(Vec::new()),
                    next - position,
                ));
            }
            let mut entries = Vec::with_capacity(count as usize);
            let mut cursor = next;
            for _ in 0..count {
                let (key, consumed) = match try_decode(buffer, cursor)? {
                    DecodeAttempt::Incomplete => return Ok(DecodeAttempt::Incomplete),
                    DecodeAttempt::Complete(key, consumed) => (key, consumed),
                };
                cursor += consumed;
                let (value, consumed) = match try_decode(buffer, cursor)? {
                    DecodeAttempt::Incomplete => return Ok(DecodeAttempt::Incomplete),
                    DecodeAttempt::Complete(value, consumed) => (value, consumed),
                };
                cursor += consumed;
                entries.push((key, value));
            }
            Ok(DecodeAttempt::Complete(
                Reply::Map(entries),
                cursor - position,
            ))
        }
        b'~' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let count = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP set length \"{line}\"")))?;
            if count < 0 {
                return Ok(DecodeAttempt::Complete(Reply::Set(None), next - position));
            }
            let mut items = Vec::with_capacity(count as usize);
            let mut cursor = next;
            for _ in 0..count {
                match try_decode(buffer, cursor)? {
                    DecodeAttempt::Incomplete => return Ok(DecodeAttempt::Incomplete),
                    DecodeAttempt::Complete(item, consumed) => {
                        items.push(item);
                        cursor += consumed;
                    }
                }
            }
            Ok(DecodeAttempt::Complete(
                Reply::Set(Some(items)),
                cursor - position,
            ))
        }
        b'>' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let count = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP push length \"{line}\"")))?;
            let mut items = Vec::with_capacity(count.max(0) as usize);
            let mut cursor = next;
            for _ in 0..count.max(0) {
                match try_decode(buffer, cursor)? {
                    DecodeAttempt::Incomplete => return Ok(DecodeAttempt::Incomplete),
                    DecodeAttempt::Complete(item, consumed) => {
                        items.push(item);
                        cursor += consumed;
                    }
                }
            }
            Ok(DecodeAttempt::Complete(
                Reply::Push(items),
                cursor - position,
            ))
        }
        b'!' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let length = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP blob error length \"{line}\"")))?;
            if length < 0 {
                return Ok(DecodeAttempt::Complete(
                    Reply::Error(String::new()),
                    next - position,
                ));
            }
            let length = length as usize;
            let end = next + length;
            if buffer.len() < end + 2 {
                return Ok(DecodeAttempt::Incomplete);
            }
            let value = String::from_utf8_lossy(&buffer[next..end]).into_owned();
            Ok(DecodeAttempt::Complete(
                Reply::Error(value),
                end + 2 - position,
            ))
        }
        b'=' => {
            let Some((line, next)) = read_line(buffer, position + 1) else {
                return Ok(DecodeAttempt::Incomplete);
            };
            let length = line
                .parse::<i64>()
                .map_err(|_| gerror::new(format!("invalid RESP verbatim length \"{line}\"")))?;
            if length < 0 {
                return Ok(DecodeAttempt::Complete(Reply::Bulk(None), next - position));
            }
            let length = length as usize;
            let end = next + length;
            if buffer.len() < end + 2 {
                return Ok(DecodeAttempt::Incomplete);
            }
            let value = buffer[next..end].to_vec();
            Ok(DecodeAttempt::Complete(
                Reply::Bulk(Some(value)),
                end + 2 - position,
            ))
        }
        other => Err(gerror::new(format!(
            "unknown RESP reply type \"{}\"",
            *other as char
        ))),
    }
}

/// Reads a CRLF-terminated line starting at `position`.
fn read_line(buffer: &[u8], position: usize) -> Option<(String, usize)> {
    let relative = buffer[position..]
        .windows(2)
        .position(|window| window == b"\r\n")?;
    let end = position + relative;
    Some((
        String::from_utf8_lossy(&buffer[position..end]).into_owned(),
        end + 2,
    ))
}

/// The Redis client, like GoFrame's client object.
pub struct Client {
    config: Config,
    connection: Mutex<Option<Conn>>,
}

impl Client {
    /// Creates a client and runs `AUTH`/`SELECT` when configured, like
    /// `New`.
    pub fn new(config: Config) -> Result<Client, Error> {
        let config = if config.master_name.is_empty() {
            config
        } else {
            resolve_sentinel_master(&config)?
        };
        let connection = open_connection(&config)?;
        Ok(Client {
            config,
            connection: Mutex::new(Some(connection)),
        })
    }

    /// Creates a client for `address` with the default configuration.
    pub fn with_address(address: &str) -> Result<Client, Error> {
        Client::new(Config::new(address))
    }

    /// The configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Sends one command and returns its reply.
    pub fn command(&self, args: &[&str]) -> Result<Reply, Error> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let connection = guard
            .as_mut()
            .ok_or_else(|| gerror::new("Redis connection is closed"))?;
        let reply = round_trip(connection, args)?;
        reply.into_result()
    }

    /// Negotiates a RESP protocol version with `HELLO`, like `HELLO`.
    pub fn hello(&self, protocol: i64) -> Result<Reply, Error> {
        self.command(&["HELLO", &protocol.to_string()])
    }

    /// Negotiates RESP3, returning the `HELLO 3` map reply (ADR-111).
    pub fn resp3(&self) -> Result<Reply, Error> {
        self.hello(3)
    }

    /// Sends several commands in one write and returns their replies in order,
    /// like a client pipeline (ADR-097).
    pub fn pipeline(&self, commands: &[Vec<String>]) -> Result<Vec<Reply>, Error> {
        let mut payload = Vec::new();
        for command in commands {
            let args: Vec<&str> = command.iter().map(String::as_str).collect();
            payload.extend_from_slice(&encode_command(&args));
        }
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let connection = guard
            .as_mut()
            .ok_or_else(|| gerror::new("Redis connection is closed"))?;
        connection.send(&payload, None)?;
        read_replies(connection, commands.len())
    }

    /// WATCHes `keys` on the client connection, like `WATCH`.
    pub fn watch(&self, keys: &[&str]) -> Result<(), Error> {
        let mut args = vec!["WATCH"];
        args.extend_from_slice(keys);
        self.command(&args).map(|_| ())
    }

    /// UNWATCHes the keys watched on the client connection.
    pub fn unwatch(&self) -> Result<(), Error> {
        self.command(&["UNWATCH"]).map(|_| ())
    }

    /// Runs `commands` inside `MULTI`/`EXEC`, returning the `EXEC` replies.
    ///
    /// A queueing error aborts the transaction; a `nil` `EXEC` reply (a watched
    /// key changed) is reported as an error, like `go-redis`.
    pub fn transaction(&self, commands: &[Vec<String>]) -> Result<Vec<Reply>, Error> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let connection = guard
            .as_mut()
            .ok_or_else(|| gerror::new("Redis connection is closed"))?;
        run_transaction(connection, commands)
    }

    /// WATCHes `keys` and runs `commands` in one transaction on the same
    /// connection, like `Watch` + `TxPipelined`.
    pub fn watch_transaction(
        &self,
        keys: &[&str],
        commands: &[Vec<String>],
    ) -> Result<Vec<Reply>, Error> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let connection = guard
            .as_mut()
            .ok_or_else(|| gerror::new("Redis connection is closed"))?;
        let mut args = vec!["WATCH"];
        args.extend_from_slice(keys);
        round_trip(connection, &args)?.into_result()?;
        run_transaction(connection, commands)
    }

    /// Sends a command given as owned strings.
    fn command_owned(&self, args: &[String]) -> Result<Reply, Error> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.command(&refs)
    }

    /// PINGs the server.
    pub fn ping(&self) -> Result<String, Error> {
        self.command(&["PING"]).map(|reply| reply.string())
    }

    /// GETs `key`.
    pub fn get(&self, key: &str) -> Result<Option<String>, Error> {
        self.command(&["GET", key]).map(|reply| reply.bulk())
    }

    /// SETs `key` to `value`.
    pub fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        self.command(&["SET", key, value]).map(|_| ())
    }

    /// SETs `key` with an expiry in seconds (SETEX).
    pub fn set_ex(&self, key: &str, value: &str, seconds: u64) -> Result<(), Error> {
        let seconds = seconds.to_string();
        self.command(&["SETEX", key, &seconds, value]).map(|_| ())
    }

    /// DELs `keys` and returns the removed count.
    pub fn del(&self, keys: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["DEL"];
        args.extend_from_slice(keys);
        self.command(&args).map(|reply| reply.int())
    }

    /// Returns how many of `keys` exist.
    pub fn exists(&self, keys: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["EXISTS"];
        args.extend_from_slice(keys);
        self.command(&args).map(|reply| reply.int())
    }

    /// Sets an expiry in seconds.
    pub fn expire(&self, key: &str, seconds: u64) -> Result<bool, Error> {
        let seconds = seconds.to_string();
        self.command(&["EXPIRE", key, &seconds])
            .map(|reply| reply.int() == 1)
    }

    /// Returns the remaining TTL in seconds.
    pub fn ttl(&self, key: &str) -> Result<i64, Error> {
        self.command(&["TTL", key]).map(|reply| reply.int())
    }

    /// INCRs `key`.
    pub fn incr(&self, key: &str) -> Result<i64, Error> {
        self.command(&["INCR", key]).map(|reply| reply.int())
    }

    /// DECRs `key`.
    pub fn decr(&self, key: &str) -> Result<i64, Error> {
        self.command(&["DECR", key]).map(|reply| reply.int())
    }

    /// HGETs one hash field.
    pub fn hget(&self, key: &str, field: &str) -> Result<Option<String>, Error> {
        self.command(&["HGET", key, field])
            .map(|reply| reply.bulk())
    }

    /// HSETs one hash field.
    pub fn hset(&self, key: &str, field: &str, value: &str) -> Result<i64, Error> {
        self.command(&["HSET", key, field, value])
            .map(|reply| reply.int())
    }

    /// HGETALLs a hash as pairs.
    pub fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, Error> {
        self.command(&["HGETALL", key]).map(|reply| reply.pairs())
    }

    /// HDELs hash fields.
    pub fn hdel(&self, key: &str, fields: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["HDEL", key];
        args.extend_from_slice(fields);
        self.command(&args).map(|reply| reply.int())
    }

    /// LPUSHes values.
    pub fn lpush(&self, key: &str, values: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["LPUSH", key];
        args.extend_from_slice(values);
        self.command(&args).map(|reply| reply.int())
    }

    /// RPUSHes values.
    pub fn rpush(&self, key: &str, values: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["RPUSH", key];
        args.extend_from_slice(values);
        self.command(&args).map(|reply| reply.int())
    }

    /// LPOPs one value.
    pub fn lpop(&self, key: &str) -> Result<Option<String>, Error> {
        self.command(&["LPOP", key]).map(|reply| reply.bulk())
    }

    /// RPOPs one value.
    pub fn rpop(&self, key: &str) -> Result<Option<String>, Error> {
        self.command(&["RPOP", key]).map(|reply| reply.bulk())
    }

    /// Returns the list length.
    pub fn llen(&self, key: &str) -> Result<i64, Error> {
        self.command(&["LLEN", key]).map(|reply| reply.int())
    }

    /// LRANGEs `start..=stop`.
    pub fn lrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>, Error> {
        self.command(&["LRANGE", key, &start.to_string(), &stop.to_string()])
            .map(|reply| reply.strings())
    }

    /// SADDs members.
    pub fn sadd(&self, key: &str, members: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["SADD", key];
        args.extend_from_slice(members);
        self.command(&args).map(|reply| reply.int())
    }

    /// SMEMBERS of a set.
    pub fn smembers(&self, key: &str) -> Result<Vec<String>, Error> {
        self.command(&["SMEMBERS", key])
            .map(|reply| reply.strings())
    }

    /// SREMs members.
    pub fn srem(&self, key: &str, members: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["SREM", key];
        args.extend_from_slice(members);
        self.command(&args).map(|reply| reply.int())
    }

    /// SCARD of a set.
    pub fn scard(&self, key: &str) -> Result<i64, Error> {
        self.command(&["SCARD", key]).map(|reply| reply.int())
    }

    /// ZADDs one member with its score.
    pub fn zadd(&self, key: &str, score: f64, member: &str) -> Result<i64, Error> {
        self.command(&["ZADD", key, &score.to_string(), member])
            .map(|reply| reply.int())
    }

    /// ZSCOREs one member.
    pub fn zscore(&self, key: &str, member: &str) -> Result<Option<String>, Error> {
        self.command(&["ZSCORE", key, member])
            .map(|reply| reply.bulk())
    }

    /// ZRANGEs `start..=stop`.
    pub fn zrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>, Error> {
        self.command(&["ZRANGE", key, &start.to_string(), &stop.to_string()])
            .map(|reply| reply.strings())
    }

    /// ZREMs members.
    pub fn zrem(&self, key: &str, members: &[&str]) -> Result<i64, Error> {
        let mut args = vec!["ZREM", key];
        args.extend_from_slice(members);
        self.command(&args).map(|reply| reply.int())
    }

    /// ZCARD of a sorted set.
    pub fn zcard(&self, key: &str) -> Result<i64, Error> {
        self.command(&["ZCARD", key]).map(|reply| reply.int())
    }

    /// PUBLISHes `message` on `channel`.
    pub fn publish(&self, channel: &str, message: &str) -> Result<i64, Error> {
        self.command(&["PUBLISH", channel, message])
            .map(|reply| reply.int())
    }

    /// Runs `script` with `keys` and `args`, like `EVAL` (ADR-099).
    pub fn eval(&self, script: &str, keys: &[&str], args: &[&str]) -> Result<Reply, Error> {
        self.command_owned(&script_args("EVAL", script, keys, args))
    }

    /// Runs a cached script by `sha`, like `EVALSHA`.
    pub fn eval_sha(&self, sha: &str, keys: &[&str], args: &[&str]) -> Result<Reply, Error> {
        self.command_owned(&script_args("EVALSHA", sha, keys, args))
    }

    /// Loads `script` into the script cache, returning its SHA1, like
    /// `SCRIPT LOAD`.
    pub fn script_load(&self, script: &str) -> Result<String, Error> {
        self.command(&["SCRIPT", "LOAD", script])
            .map(|reply| reply.string())
    }

    /// Reports which of `shas` are cached, like `SCRIPT EXISTS`.
    pub fn script_exists(&self, shas: &[&str]) -> Result<Vec<bool>, Error> {
        let mut args = vec!["SCRIPT", "EXISTS"];
        args.extend_from_slice(shas);
        self.command(&args)
            .map(|reply| reply.strings().iter().map(|value| value == "1").collect())
    }

    /// Clears the script cache, like `SCRIPT FLUSH`.
    pub fn script_flush(&self) -> Result<(), Error> {
        self.command(&["SCRIPT", "FLUSH"]).map(|_| ())
    }

    /// Closes the connection.
    pub fn close(&self) {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(connection) = guard.take() {
            let _ = connection.close();
        }
    }

    /// Opens a dedicated subscription connection and subscribes to `channels`,
    /// like `Subscribe` (ADR-098).
    pub fn subscribe(&self, channels: &[&str]) -> Result<Subscription, Error> {
        Subscription::open(&self.config, channels, false)
    }

    /// Opens a dedicated subscription connection and subscribes to
    /// `patterns`, like `PSubscribe`.
    pub fn psubscribe(&self, patterns: &[&str]) -> Result<Subscription, Error> {
        Subscription::open(&self.config, patterns, true)
    }
}

/// One pub/sub message, like `go-redis`'s `Message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscribeMessage {
    /// The pattern that matched (`PSUBSCRIBE` only).
    pub pattern: Option<String>,
    /// The channel the message was published on.
    pub channel: String,
    /// The message payload.
    pub payload: String,
}

/// A dedicated pub/sub connection, like `go-redis`'s `PubSub`.
pub struct Subscription {
    connection: Conn,
    buffer: Vec<u8>,
}

impl Subscription {
    /// Opens a connection and subscribes to `names` (channels or patterns).
    fn open(config: &Config, names: &[&str], pattern: bool) -> Result<Subscription, Error> {
        let connection = open_connection(config)?;
        let mut subscription = Subscription {
            connection,
            buffer: Vec::new(),
        };
        let mut args = vec![if pattern { "PSUBSCRIBE" } else { "SUBSCRIBE" }];
        args.extend_from_slice(names);
        subscription.connection.send(&encode_command(&args), None)?;
        subscription.read_next()?.into_result()?;
        Ok(subscription)
    }

    /// Reads one reply from the buffered pub/sub stream.
    fn read_next(&mut self) -> Result<Reply, Error> {
        loop {
            match try_decode(&self.buffer, 0)? {
                DecodeAttempt::Complete(reply, consumed) => {
                    self.buffer.drain(..consumed);
                    return Ok(reply);
                }
                DecodeAttempt::Incomplete => {
                    let mut chunk = self.connection.recv(-1, None)?;
                    self.buffer.append(&mut chunk);
                }
            }
        }
    }

    /// Waits for the next message, skipping subscribe/unsubscribe acks.
    pub fn next_message(&mut self) -> Result<SubscribeMessage, Error> {
        loop {
            let reply = self.read_next()?;
            let Reply::Array(Some(items)) = reply else {
                return Err(gerror::new(format!("unexpected pub/sub reply: {reply:?}")));
            };
            let kind = items.first().map(Reply::string).unwrap_or_default();
            match kind.as_str() {
                "message" => {
                    return Ok(SubscribeMessage {
                        pattern: None,
                        channel: items.get(1).map(Reply::string).unwrap_or_default(),
                        payload: items.get(2).map(Reply::string).unwrap_or_default(),
                    });
                }
                "pmessage" => {
                    return Ok(SubscribeMessage {
                        pattern: items.get(1).map(Reply::string),
                        channel: items.get(2).map(Reply::string).unwrap_or_default(),
                        payload: items.get(3).map(Reply::string).unwrap_or_default(),
                    });
                }
                _ => continue,
            }
        }
    }

    /// Unsubscribes from everything and reads the acknowledgement.
    pub fn unsubscribe(&mut self) -> Result<(), Error> {
        self.connection
            .send(&encode_command(&["UNSUBSCRIBE"]), None)?;
        self.read_next()?.into_result().map(|_| ())
    }

    /// Closes the subscription connection.
    pub fn close(self) -> Result<(), Error> {
        self.connection.close()
    }
}

impl Reply {
    /// Converts an error reply into an `Err`.
    fn into_result(self) -> Result<Reply, Error> {
        match self {
            Reply::Error(message) => Err(gerror::new_code(
                &gcode::CODE_INVALID_OPERATION,
                &[message.as_str()],
            )),
            other => Ok(other),
        }
    }
}

/// Writes one command and reads one complete reply.
fn round_trip(connection: &mut Conn, args: &[&str]) -> Result<Reply, Error> {
    connection.send(&encode_command(args), None)?;
    let mut buffer = Vec::new();
    loop {
        let mut chunk = connection.recv(-1, None)?;
        buffer.append(&mut chunk);
        match try_decode(&buffer, 0)? {
            DecodeAttempt::Complete(reply, _consumed) => return Ok(reply),
            DecodeAttempt::Incomplete => continue,
        }
    }
}

/// Builds an `EVAL`/`EVALSHA` command.
fn script_args(kind: &str, script: &str, keys: &[&str], args: &[&str]) -> Vec<String> {
    let mut command = vec![kind.to_owned(), script.to_owned(), keys.len().to_string()];
    command.extend(keys.iter().map(|key| (*key).to_owned()));
    command.extend(args.iter().map(|argument| (*argument).to_owned()));
    command
}

/// Asks the configured sentinels for the master of `config.master_name`
/// (ADR-114), returning the config pointed at the resolved master.
fn resolve_sentinel_master(config: &Config) -> Result<Config, Error> {
    let mut last_error = String::new();
    for address in config.address.split(',') {
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        let attempt = (|| -> Result<Config, Error> {
            let mut connection = Conn::new(address, Some(config.timeout))?;
            connection
                .set_deadline(Some(config.timeout))
                .map_err(|error| gerror::new(format!("set sentinel deadline failed: {error}")))?;
            if !config.sentinel_pass.is_empty() {
                let mut args = vec!["AUTH"];
                if !config.sentinel_user.is_empty() {
                    args.push(&config.sentinel_user);
                }
                args.push(&config.sentinel_pass);
                round_trip(&mut connection, &args)?.into_result()?;
            }
            let reply = round_trip(
                &mut connection,
                &["SENTINEL", "get-master-addr-by-name", &config.master_name],
            )?
            .into_result()?;
            let Reply::Array(Some(items)) = reply else {
                return Err(gerror::new(format!(
                    "sentinel \"{address}\" returned no master address"
                )));
            };
            if items.len() != 2 {
                return Err(gerror::new(format!(
                    "sentinel \"{address}\" returned an invalid master address"
                )));
            }
            let host = items[0]
                .bulk()
                .ok_or_else(|| gerror::new("sentinel returned an invalid master host"))?;
            let port = items[1]
                .bulk()
                .ok_or_else(|| gerror::new("sentinel returned an invalid master port"))?;
            Ok(Config {
                address: format!("{host}:{port}"),
                master_name: String::new(),
                ..config.clone()
            })
        })();
        match attempt {
            Ok(resolved) => return Ok(resolved),
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(gerror::new(format!(
        "resolve Redis sentinel master \"{}\" failed: {last_error}",
        config.master_name
    )))
}

/// Opens one connection and runs `AUTH`/`SELECT` when configured.
fn open_connection(config: &Config) -> Result<Conn, Error> {
    let mut connection = Conn::new(&config.address, Some(config.timeout))?;
    connection
        .set_deadline(Some(config.timeout))
        .map_err(|error| gerror::new(format!("set Redis deadline failed: {error}")))?;
    if !config.password.is_empty() {
        round_trip(&mut connection, &["AUTH", &config.password])?.into_result()?;
    }
    if config.db != 0 {
        let db = config.db.to_string();
        round_trip(&mut connection, &["SELECT", &db])?.into_result()?;
    }
    Ok(connection)
}

/// Reads `count` consecutive replies from `connection`.
fn read_replies(connection: &mut Conn, count: usize) -> Result<Vec<Reply>, Error> {
    let mut replies = Vec::with_capacity(count);
    let mut buffer = Vec::new();
    let mut offset = 0;
    while replies.len() < count {
        match try_decode(&buffer, offset)? {
            DecodeAttempt::Complete(reply, consumed) => {
                replies.push(reply);
                offset += consumed;
                if offset == buffer.len() {
                    buffer.clear();
                    offset = 0;
                }
            }
            DecodeAttempt::Incomplete => {
                let mut chunk = connection.recv(-1, None)?;
                buffer.append(&mut chunk);
            }
        }
    }
    Ok(replies)
}

/// Runs `MULTI` + `commands` + `EXEC` on `connection`.
fn run_transaction(connection: &mut Conn, commands: &[Vec<String>]) -> Result<Vec<Reply>, Error> {
    round_trip(connection, &["MULTI"])?.into_result()?;
    let mut queue_errors: Vec<String> = Vec::new();
    for command in commands {
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        let reply = round_trip(connection, &args)?;
        if let Some(message) = reply.error_text() {
            queue_errors.push(message.to_owned());
        }
    }
    if !queue_errors.is_empty() {
        // Reset the server state; the queueing error is the reported failure.
        let _ = round_trip(connection, &["EXEC"])?;
        let message = format!("transaction queue failed: {}", queue_errors.join("; "));
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_OPERATION,
            &[message.as_str()],
        ));
    }
    match round_trip(connection, &["EXEC"])?.into_result()? {
        Reply::Array(Some(items)) => Ok(items),
        Reply::Array(None) => Err(gerror::new("transaction aborted: a watched key changed")),
        other => Err(gerror::new(format!("unexpected EXEC reply: {other:?}"))),
    }
}

/// One Redis Cluster slot range and its serving node (ADR-116).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRange {
    /// First slot.
    pub start: u16,
    /// Last slot.
    pub end: u16,
    /// The node address (`host:port`).
    pub address: String,
}

/// A Redis Cluster client that routes commands by key slot, like GoFrame's
/// cluster config (ADR-116).
pub struct ClusterClient {
    config: Config,
    slots: Mutex<Vec<SlotRange>>,
    connections: Mutex<HashMap<String, Conn>>,
}

impl ClusterClient {
    /// Creates a cluster client, discovering the slots from `CLUSTER SLOTS`.
    pub fn new(config: Config) -> Result<ClusterClient, Error> {
        let client = ClusterClient {
            config,
            slots: Mutex::new(Vec::new()),
            connections: Mutex::new(HashMap::new()),
        };
        client.refresh_slots()?;
        Ok(client)
    }

    /// The discovered slot ranges.
    pub fn slots(&self) -> Vec<SlotRange> {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Sends `args` to the node owning the key's slot.
    pub fn command(&self, args: &[&str]) -> Result<Reply, Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let address = self.address_for(args);
            let reply = self.round_trip_to(&address, args)?;
            match reply {
                Ok(reply) => return Ok(reply),
                Err(error) => {
                    let text = error.to_string();
                    let retry = parse_redirection(&text);
                    if attempt <= 2
                        && let Some((moved, _, _)) = &retry
                        && *moved
                    {
                        self.refresh_slots()?;
                        continue;
                    }
                    if retry.is_some() && attempt <= 2 {
                        self.refresh_slots()?;
                        continue;
                    }
                    return Err(error);
                }
            }
        }
    }

    /// Reports `PING` against the first node.
    pub fn ping(&self) -> Result<String, Error> {
        self.command(&["PING"]).map(|reply| reply.string())
    }

    /// Closes every node connection.
    pub fn close(&self) {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Finds the address owning the command's key.
    fn address_for(&self, args: &[&str]) -> String {
        let Some(key) = command_key(args) else {
            let slots = self
                .slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(range) = slots.first() {
                return range.address.clone();
            }
            return self
                .config
                .address
                .split(',')
                .next()
                .unwrap_or(DEFAULT_ADDRESS)
                .trim()
                .to_owned();
        };
        let slot = key_slot(key);
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for range in slots.iter() {
            if slot >= range.start && slot <= range.end {
                return range.address.clone();
            }
        }
        self.config
            .address
            .split(',')
            .next()
            .unwrap_or(DEFAULT_ADDRESS)
            .trim()
            .to_owned()
    }

    /// Reads the slot table from a reachable node.
    fn refresh_slots(&self) -> Result<(), Error> {
        let candidates: Vec<String> = {
            let connections = self
                .connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut candidates: Vec<String> = connections.keys().cloned().collect();
            candidates.extend(
                self.config
                    .address
                    .split(',')
                    .map(|address| address.trim().to_owned()),
            );
            candidates
        };
        let mut last_error = String::new();
        for address in candidates {
            if address.is_empty() {
                continue;
            }
            if let Ok(reply) = self.round_trip_to(&address, &["CLUSTER", "SLOTS"])
                && let Ok(reply) = reply
                && let Some(slots) = parse_slot_ranges(&reply)
                && !slots.is_empty()
            {
                *self
                    .slots
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = slots;
                return Ok(());
            }
            last_error = format!("node \"{address}\" did not return CLUSTER SLOTS");
        }
        Err(gerror::new(format!(
            "discover Redis Cluster slots failed: {last_error}"
        )))
    }

    /// Runs one command against `address`, creating its connection on demand.
    fn round_trip_to(&self, address: &str, args: &[&str]) -> Result<Result<Reply, Error>, Error> {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !connections.contains_key(address) {
            let node_config = Config {
                address: address.to_owned(),
                cluster: false,
                ..self.config.clone()
            };
            connections.insert(address.to_owned(), open_connection(&node_config)?);
        }
        let connection = connections
            .get_mut(address)
            .ok_or_else(|| gerror::new("cluster connection disappeared"))?;
        let reply = round_trip(connection, args)?;
        Ok(reply.into_result())
    }
}

/// Reports whether the command's first argument is its key, and returns it.
fn command_key<'a>(args: &[&'a str]) -> Option<&'a str> {
    let name = args.first()?.to_ascii_uppercase();
    if matches!(
        name.as_str(),
        "PING"
            | "ECHO"
            | "AUTH"
            | "SELECT"
            | "HELLO"
            | "INFO"
            | "CLUSTER"
            | "COMMAND"
            | "SCRIPT"
            | "FLUSHALL"
            | "FLUSHDB"
    ) {
        return None;
    }
    args.get(1).copied()
}

/// Computes the Redis Cluster slot of `key` (`CRC16-CCITT`, `{}` hashtags).
pub fn key_slot(key: &str) -> u16 {
    let hashable = {
        let start = key.find('{');
        match start {
            Some(start) => match key[start + 1..].find('}') {
                Some(offset) if offset > 0 => &key[start + 1..start + 1 + offset],
                _ => key,
            },
            None => key,
        }
    };
    let mut crc: u16 = 0;
    for byte in hashable.bytes() {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc % 16_384
}

/// Parses `CLUSTER SLOTS` into ranges.
fn parse_slot_ranges(reply: &Reply) -> Option<Vec<SlotRange>> {
    let Reply::Array(Some(ranges)) = reply else {
        return None;
    };
    let mut slots = Vec::new();
    for range in ranges {
        let Reply::Array(Some(items)) = range else {
            continue;
        };
        if items.len() < 3 {
            continue;
        }
        let start = items[0].int();
        let end = items[1].int();
        let Reply::Array(Some(node)) = &items[2] else {
            continue;
        };
        if node.len() < 2 {
            continue;
        }
        let host = node[0].bulk().unwrap_or_default();
        let port = node[1].int();
        if host.is_empty() || port <= 0 {
            continue;
        }
        slots.push(SlotRange {
            start: start.clamp(0, 16_383) as u16,
            end: end.clamp(0, 16_383) as u16,
            address: format!("{host}:{port}"),
        });
    }
    slots.sort_by_key(|range| range.start);
    Some(slots)
}

/// Parses a `MOVED`/`ASK` redirection error into `(moved, slot, address)`.
fn parse_redirection(text: &str) -> Option<(bool, u16, String)> {
    let (moved, rest) = if let Some(rest) = text.strip_prefix("MOVED ") {
        (true, rest)
    } else {
        (false, text.strip_prefix("ASK ")?)
    };
    let mut parts = rest.split_whitespace();
    let slot = parts.next()?.parse::<u16>().ok()?;
    let address = parts.next()?.to_owned();
    Some((moved, slot, address))
}

/// One idle pooled connection.
struct IdleConnection {
    client: Arc<Client>,
    since: Instant,
    created: Instant,
}

/// A Redis connection pool, like GoFrame's go-redis pool options (ADR-120).
pub struct RedisPool {
    config: Config,
    idle: Mutex<Vec<IdleConnection>>,
    total: Mutex<usize>,
    created: std::sync::atomic::AtomicUsize,
    available: Condvar,
}

/// A client borrowed from [`RedisPool`]; returning it on drop.
pub struct PooledClient {
    pool: Arc<RedisPool>,
    client: Arc<Client>,
    created: Instant,
}

impl PooledClient {
    /// The underlying client.
    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }
}

impl std::ops::Deref for PooledClient {
    type Target = Client;

    fn deref(&self) -> &Client {
        &self.client
    }
}

impl Drop for PooledClient {
    fn drop(&mut self) {
        self.pool
            .return_client(Arc::clone(&self.client), self.created);
    }
}

impl RedisPool {
    /// Creates a pool and pre-opens `min_idle` connections.
    pub fn new(config: Config) -> Result<Arc<RedisPool>, Error> {
        let pool = Arc::new(RedisPool {
            config,
            idle: Mutex::new(Vec::new()),
            total: Mutex::new(0),
            created: std::sync::atomic::AtomicUsize::new(0),
            available: Condvar::new(),
        });
        for _ in 0..pool.config.min_idle {
            let client = pool.create_client()?;
            let now = Instant::now();
            pool.idle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(IdleConnection {
                    client,
                    since: now,
                    created: now,
                });
        }
        Ok(pool)
    }

    /// The number of idle connections.
    pub fn idle_count(&self) -> usize {
        self.idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// How many connections the pool has opened so far (diagnostics).
    pub fn created_count(&self) -> usize {
        self.created.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The number of open connections (idle and borrowed).
    pub fn total(&self) -> usize {
        *self
            .total
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Borrows a client, reusing an idle healthy connection when possible.
    pub fn get(self: &Arc<Self>) -> Result<PooledClient, Error> {
        let deadline = if self.config.wait_timeout.is_zero() {
            None
        } else {
            Some(Instant::now() + self.config.wait_timeout)
        };
        loop {
            if let Some(idle) = self.take_idle() {
                if idle.client.ping().is_ok() {
                    return Ok(PooledClient {
                        pool: Arc::clone(self),
                        client: idle.client,
                        created: idle.created,
                    });
                }
                self.dec_total();
            }
            if self.reserve_slot() {
                match Client::new(self.config.clone()) {
                    Ok(client) => {
                        self.created
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return Ok(PooledClient {
                            pool: Arc::clone(self),
                            client: Arc::new(client),
                            created: Instant::now(),
                        });
                    }
                    Err(error) => {
                        self.dec_total();
                        return Err(error);
                    }
                }
            }
            // The pool is full: wait for a return when a timeout is configured.
            let Some(deadline) = deadline else {
                return Err(gerror::new("Redis connection pool is exhausted"));
            };
            let now = Instant::now();
            if now >= deadline {
                return Err(gerror::new("wait for a Redis connection timed out"));
            }
            let total = self
                .total
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = self
                .available
                .wait_timeout(total, deadline - now)
                .map_err(|_| gerror::new("wait for a Redis connection failed"))?;
        }
    }

    /// Closes every idle connection and the pool itself.
    pub fn close(&self) {
        let mut idle = self
            .idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for entry in idle.drain(..) {
            entry.client.close();
            self.dec_total();
        }
    }

    /// Takes one usable idle connection, discarding expired entries.
    fn take_idle(&self) -> Option<IdleConnection> {
        let mut idle = self
            .idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while let Some(entry) = idle.pop() {
            if entry.since.elapsed() > self.config.idle_timeout
                || entry.created.elapsed() > self.config.max_conn_lifetime
            {
                entry.client.close();
                self.dec_total();
                continue;
            }
            return Some(entry);
        }
        None
    }

    /// Reserves one open-connection slot; false when `max_active` is reached.
    fn reserve_slot(&self) -> bool {
        let mut total = self
            .total
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.config.max_active != 0 && *total >= self.config.max_active {
            return false;
        }
        *total += 1;
        true
    }

    /// Creates a new client, honouring the configured slot limit.
    fn create_client(&self) -> Result<Arc<Client>, Error> {
        if !self.reserve_slot() {
            return Err(gerror::new("Redis connection pool is exhausted"));
        }
        match Client::new(self.config.clone()) {
            Ok(client) => {
                self.created
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Arc::new(client))
            }
            Err(error) => {
                self.dec_total();
                Err(error)
            }
        }
    }

    /// Releases one open-connection slot and wakes a waiter.
    fn dec_total(&self) {
        let mut total = self
            .total
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *total = total.saturating_sub(1);
        drop(total);
        self.available.notify_one();
    }

    /// Returns a borrowed client to the idle list (or closes it).
    fn return_client(&self, client: Arc<Client>, created: Instant) {
        if self.config.idle_timeout.is_zero() && self.config.max_idle == 0 {
            client.close();
            self.dec_total();
            return;
        }
        let mut idle = self
            .idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if idle.len() < self.config.max_idle.max(1) {
            idle.push(IdleConnection {
                client,
                since: Instant::now(),
                created,
            });
            drop(idle);
            self.available.notify_one();
        } else {
            drop(idle);
            client.close();
            self.dec_total();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    fn bulk(value: &str) -> Vec<u8> {
        format!("${}\r\n{value}\r\n", value.len()).into_bytes()
    }

    fn simple(value: &str) -> Vec<u8> {
        format!("+{value}\r\n").into_bytes()
    }

    fn integer(value: i64) -> Vec<u8> {
        format!(":{value}\r\n").into_bytes()
    }

    fn array(items: &[Vec<u8>]) -> Vec<u8> {
        let mut buffer = format!("*{}\r\n", items.len()).into_bytes();
        for item in items {
            buffer.extend_from_slice(item);
        }
        buffer
    }

    /// Reads one RESP command from `reader`.
    fn read_command(reader: &mut impl BufRead) -> Option<Vec<String>> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        let count = line.strip_prefix('*')?.parse::<usize>().ok()?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            let mut length_line = String::new();
            reader.read_line(&mut length_line).ok()?;
            let length = length_line
                .trim_end()
                .strip_prefix('$')?
                .parse::<usize>()
                .ok()?;
            let mut bytes = vec![0u8; length + 2];
            reader.read_exact(&mut bytes).ok()?;
            bytes.truncate(length);
            args.push(String::from_utf8_lossy(&bytes).into_owned());
        }
        Some(args)
    }

    /// Spawns a fake Redis server; `handler` receives the args and returns the
    /// raw reply bytes.
    fn spawn_fake(
        handler: Arc<dyn Fn(Vec<String>) -> Vec<u8> + Send + Sync>,
        received: Arc<Mutex<Vec<Vec<String>>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("addr").to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let handler = Arc::clone(&handler);
                let received = Arc::clone(&received);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut writer = stream;
                    while let Some(args) = read_command(&mut reader) {
                        received.lock().unwrap().push(args.clone());
                        let reply = handler(args);
                        if writer.write_all(&reply).is_err() {
                            break;
                        }
                        let _ = writer.flush();
                    }
                });
            }
        });
        address
    }

    fn ok_handler(args: Vec<String>) -> Vec<u8> {
        match args.first().map(String::as_str) {
            Some("AUTH") | Some("SELECT") | Some("SET") | Some("SETEX") => simple("OK"),
            Some("EXPIRE") => integer(1),
            Some("PING") => simple("PONG"),
            Some("GET") => bulk("hello"),
            Some("INCR") => integer(2),
            Some("DECR") => integer(1),
            Some("DEL") => integer(1),
            Some("EXISTS") => integer(1),
            Some("TTL") => integer(100),
            Some("HGET") => bulk("value"),
            Some("HSET") => integer(1),
            Some("HGETALL") => array(&[bulk("a"), bulk("1"), bulk("b"), bulk("2")]),
            Some("HDEL") => integer(1),
            Some("LPUSH") | Some("RPUSH") => integer(2),
            Some("LPOP") => bulk("first"),
            Some("RPOP") => bulk("last"),
            Some("LLEN") => integer(2),
            Some("LRANGE") => array(&[bulk("a"), bulk("b")]),
            Some("SADD") | Some("SREM") => integer(1),
            Some("SMEMBERS") => array(&[bulk("x"), bulk("y")]),
            Some("SCARD") => integer(2),
            Some("ZADD") => integer(1),
            Some("ZSCORE") => bulk("1.5"),
            Some("ZRANGE") => array(&[bulk("m1"), bulk("m2")]),
            Some("ZREM") => integer(1),
            Some("ZCARD") => integer(2),
            Some("PUBLISH") => integer(3),
            _ => simple("OK"),
        }
    }

    fn client_for(
        handler: Arc<dyn Fn(Vec<String>) -> Vec<u8> + Send + Sync>,
    ) -> (Client, Arc<Mutex<Vec<Vec<String>>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let address = spawn_fake(handler, Arc::clone(&received));
        let client = Client::with_address(&address).expect("client");
        (client, received)
    }

    #[test]
    fn encode_and_decode_replies() {
        assert_eq!(
            encode_command(&["SET", "k", "v"]),
            b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n"
        );
        assert_eq!(
            decode_reply(b"+OK\r\n").expect("simple"),
            (Reply::Simple("OK".to_owned()), 5)
        );
        assert_eq!(
            decode_reply(b":42\r\n").expect("integer"),
            (Reply::Integer(42), 5)
        );
        assert_eq!(
            decode_reply(b"$5\r\nhello\r\n").expect("bulk"),
            (Reply::Bulk(Some(b"hello".to_vec())), 11)
        );
        assert_eq!(
            decode_reply(b"$-1\r\n").expect("nil bulk"),
            (Reply::Bulk(None), 5)
        );
        assert_eq!(
            decode_reply(b"-ERR bad\r\n").expect("error"),
            (Reply::Error("ERR bad".to_owned()), 10)
        );
        let nested = decode_reply(b"*2\r\n$3\r\none\r\n*2\r\n:1\r\n:2\r\n").expect("nested");
        assert_eq!(
            nested.0,
            Reply::Array(Some(vec![
                Reply::Bulk(Some(b"one".to_vec())),
                Reply::Array(Some(vec![Reply::Integer(1), Reply::Integer(2)])),
            ]))
        );
        assert!(decode_reply(b"$5\r\nhel").is_err());
        assert!(decode_reply(b"!x\r\n").is_err());
        assert!(decode_reply(b":abc\r\n").is_err());
    }

    #[test]
    fn reply_helpers() {
        let pairs = Reply::Array(Some(vec![
            Reply::Bulk(Some(b"a".to_vec())),
            Reply::Integer(1),
            Reply::Bulk(Some(b"b".to_vec())),
            Reply::Bulk(Some(b"2".to_vec())),
        ]));
        assert_eq!(
            pairs.pairs(),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(pairs.strings().len(), 4);
        assert_eq!(Reply::Integer(7).int(), 7);
        assert!(Reply::Error("x".to_owned()).is_error());
    }

    #[test]
    fn ping_auth_and_select_on_connect() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let address = spawn_fake(Arc::new(ok_handler), Arc::clone(&received));
        let config = Config::new(&address).with_password("secret").with_db(2);
        let client = Client::new(config).expect("client");
        assert_eq!(client.ping().expect("ping"), "PONG");
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["AUTH".to_owned(), "secret".to_owned()]);
        assert_eq!(commands[1], vec!["SELECT".to_owned(), "2".to_owned()]);
        assert_eq!(commands[2], vec!["PING".to_owned()]);
        client.close();
    }

    #[test]
    fn string_commands() {
        let (client, _) = client_for(Arc::new(ok_handler));
        assert_eq!(client.get("k").expect("get"), Some("hello".to_owned()));
        assert!(client.set("k", "hello").is_ok());
        assert!(client.set_ex("k", "hello", 10).is_ok());
        assert_eq!(client.del(&["k"]).expect("del"), 1);
        assert_eq!(client.exists(&["k"]).expect("exists"), 1);
        assert!(client.expire("k", 5).expect("expire"));
        assert_eq!(client.ttl("k").expect("ttl"), 100);
        assert_eq!(client.incr("k").expect("incr"), 2);
        assert_eq!(client.decr("k").expect("decr"), 1);
        client.close();
    }

    #[test]
    fn hash_list_set_and_sorted_set_commands() {
        let (client, _) = client_for(Arc::new(ok_handler));
        assert_eq!(client.hset("h", "a", "1").expect("hset"), 1);
        assert_eq!(
            client.hget("h", "a").expect("hget"),
            Some("value".to_owned())
        );
        assert_eq!(
            client.hgetall("h").expect("hgetall"),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(client.hdel("h", &["a"]).expect("hdel"), 1);
        assert_eq!(client.lpush("l", &["a", "b"]).expect("lpush"), 2);
        assert_eq!(client.rpush("l", &["c"]).expect("rpush"), 2);
        assert_eq!(client.lpop("l").expect("lpop"), Some("first".to_owned()));
        assert_eq!(client.rpop("l").expect("rpop"), Some("last".to_owned()));
        assert_eq!(client.llen("l").expect("llen"), 2);
        assert_eq!(client.lrange("l", 0, -1).expect("lrange"), ["a", "b"]);
        assert_eq!(client.sadd("s", &["x"]).expect("sadd"), 1);
        assert_eq!(client.smembers("s").expect("smembers"), ["x", "y"]);
        assert_eq!(client.srem("s", &["x"]).expect("srem"), 1);
        assert_eq!(client.scard("s").expect("scard"), 2);
        assert_eq!(client.zadd("z", 1.5, "m1").expect("zadd"), 1);
        assert_eq!(
            client.zscore("z", "m1").expect("zscore"),
            Some("1.5".to_owned())
        );
        assert_eq!(client.zrange("z", 0, -1).expect("zrange"), ["m1", "m2"]);
        assert_eq!(client.zrem("z", &["m1"]).expect("zrem"), 1);
        assert_eq!(client.zcard("z").expect("zcard"), 2);
        assert_eq!(client.publish("ch", "msg").expect("publish"), 3);
        client.close();
    }

    #[test]
    fn error_replies_surface_as_errors() {
        let (client, _) = client_for(Arc::new(|args: Vec<String>| {
            if args.first().map(String::as_str) == Some("GET") {
                b"-WRONGTYPE Operation against a key holding the wrong kind of value\r\n".to_vec()
            } else {
                simple("OK")
            }
        }));
        let error = client.get("k").expect_err("error reply");
        assert!(error.to_string().contains("WRONGTYPE"), "got {error}");
        client.close();
    }

    #[test]
    fn connection_failures_surface() {
        assert!(Client::with_address("127.0.0.1:1").is_err());
    }

    #[test]
    fn nil_bulk_is_none() {
        let (client, _) = client_for(Arc::new(|args: Vec<String>| {
            if args.first().map(String::as_str) == Some("GET") {
                b"$-1\r\n".to_vec()
            } else {
                simple("OK")
            }
        }));
        assert_eq!(client.get("missing").expect("get"), None);
        client.close();
    }

    fn error_reply(message: &str) -> Vec<u8> {
        format!("-{message}\r\n").into_bytes()
    }

    /// A fake handler implementing `MULTI`/`EXEC` queueing; `abort` makes
    /// `EXEC` return a nil array (watched key changed).
    fn transaction_handler(abort: bool) -> Arc<dyn Fn(Vec<String>) -> Vec<u8> + Send + Sync> {
        let queued = Arc::new(Mutex::new(false));
        Arc::new(move |args: Vec<String>| {
            let first = args.first().map(String::as_str).unwrap_or("");
            match first {
                "MULTI" => {
                    *queued.lock().unwrap() = true;
                    simple("OK")
                }
                "EXEC" => {
                    let was_queued = std::mem::take(&mut *queued.lock().unwrap());
                    if !was_queued {
                        return error_reply("ERR EXEC without MULTI");
                    }
                    if abort {
                        return b"*-1\r\n".to_vec();
                    }
                    array(&[bulk("v"), integer(2)])
                }
                "WATCH" | "UNWATCH" => simple("OK"),
                _ if *queued.lock().unwrap() => {
                    if args.get(1).map(String::as_str) == Some("bad") {
                        error_reply("ERR bad value")
                    } else {
                        simple("QUEUED")
                    }
                }
                "GET" => bulk("value"),
                _ => simple("OK"),
            }
        })
    }

    #[test]
    fn pipeline_returns_all_replies() {
        let (client, _) = client_for(Arc::new(ok_handler));
        let replies = client
            .pipeline(&[
                vec!["GET".to_owned(), "a".to_owned()],
                vec!["INCR".to_owned(), "n".to_owned()],
            ])
            .expect("pipeline");
        assert_eq!(
            replies,
            vec![Reply::Bulk(Some(b"hello".to_vec())), Reply::Integer(2)]
        );
        client.close();
    }

    #[test]
    fn transaction_commits() {
        let (client, received) = client_for(transaction_handler(false));
        let replies = client
            .transaction(&[
                vec!["SET".to_owned(), "a".to_owned(), "1".to_owned()],
                vec!["INCR".to_owned(), "n".to_owned()],
            ])
            .expect("transaction");
        assert_eq!(
            replies,
            vec![Reply::Bulk(Some(b"v".to_vec())), Reply::Integer(2)]
        );
        let commands: Vec<String> = received
            .lock()
            .unwrap()
            .iter()
            .map(|args| args[0].clone())
            .collect();
        assert_eq!(commands, ["MULTI", "SET", "INCR", "EXEC"]);
        client.close();
    }

    #[test]
    fn transaction_queue_error_is_reported() {
        let (client, _) = client_for(transaction_handler(false));
        let error = client
            .transaction(&[vec!["SET".to_owned(), "bad".to_owned(), "1".to_owned()]])
            .expect_err("queue error");
        assert!(
            error.to_string().contains("transaction queue failed"),
            "got {error}"
        );
        client.close();
    }

    #[test]
    fn watch_transaction_aborts_on_nil_exec() {
        let (client, received) = client_for(transaction_handler(true));
        let error = client
            .watch_transaction(
                &["watched"],
                &[vec!["SET".to_owned(), "a".to_owned(), "1".to_owned()]],
            )
            .expect_err("aborted");
        assert!(error.to_string().contains("aborted"), "got {error}");
        let commands: Vec<String> = received
            .lock()
            .unwrap()
            .iter()
            .map(|args| args[0].clone())
            .collect();
        assert_eq!(commands, ["WATCH", "MULTI", "SET", "EXEC"]);
        client.close();
    }

    #[test]
    fn watch_and_unwatch() {
        let (client, received) = client_for(transaction_handler(false));
        client.watch(&["a", "b"]).expect("watch");
        client.unwatch().expect("unwatch");
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["WATCH", "a", "b"]);
        assert_eq!(commands[1], vec!["UNWATCH"]);
        client.close();
    }

    /// A fake handler for `SUBSCRIBE`/`PSUBSCRIBE` that answers the ack and
    /// pushes one message in the same write.
    fn pubsub_handler(args: Vec<String>) -> Vec<u8> {
        let name = args.get(1).cloned().unwrap_or_default();
        match args.first().map(String::as_str).unwrap_or("") {
            "SUBSCRIBE" => {
                let mut reply = array(&[bulk("subscribe"), bulk(&name), integer(1)]);
                reply.extend(array(&[bulk("message"), bulk(&name), bulk("hello")]));
                reply
            }
            "PSUBSCRIBE" => {
                let mut reply = array(&[bulk("psubscribe"), bulk(&name), integer(1)]);
                reply.extend(array(&[
                    bulk("pmessage"),
                    bulk(&name),
                    bulk("news.tech"),
                    bulk("payload"),
                ]));
                reply
            }
            "UNSUBSCRIBE" => array(&[bulk("unsubscribe"), bulk(&name), integer(0)]),
            _ => simple("OK"),
        }
    }

    #[test]
    fn subscribe_receives_messages() {
        let (client, _) = client_for(Arc::new(pubsub_handler));
        let mut subscription = client.subscribe(&["ch"]).expect("subscribe");
        let message = subscription.next_message().expect("message");
        assert_eq!(
            message,
            SubscribeMessage {
                pattern: None,
                channel: "ch".to_owned(),
                payload: "hello".to_owned(),
            }
        );
        subscription.unsubscribe().expect("unsubscribe");
        subscription.close().expect("close");
        client.close();
    }

    #[test]
    fn psubscribe_receives_pattern_messages() {
        let (client, _) = client_for(Arc::new(pubsub_handler));
        let mut subscription = client.psubscribe(&["news.*"]).expect("psubscribe");
        let message = subscription.next_message().expect("message");
        assert_eq!(message.pattern.as_deref(), Some("news.*"));
        assert_eq!(message.channel, "news.tech");
        assert_eq!(message.payload, "payload");
        subscription.close().expect("close");
        client.close();
    }

    #[test]
    fn subscription_skips_acks_and_reports_errors() {
        let (client, _) = client_for(Arc::new(|args: Vec<String>| {
            if args.first().map(String::as_str) == Some("SUBSCRIBE") {
                error_reply("ERR subscribe failed")
            } else {
                simple("OK")
            }
        }));
        assert!(client.subscribe(&["ch"]).is_err());
        client.close();
    }

    fn script_handler(args: Vec<String>) -> Vec<u8> {
        match args.first().map(String::as_str).unwrap_or("") {
            "EVAL" => {
                if args.get(1).map(String::as_str) == Some("return 1") {
                    integer(1)
                } else {
                    bulk("ok")
                }
            }
            "EVALSHA" => bulk("sha-ok"),
            "SCRIPT" => match args.get(1).map(String::as_str).unwrap_or("") {
                "LOAD" => bulk("sha1hash"),
                "EXISTS" => array(&[integer(1), integer(0)]),
                _ => simple("OK"),
            },
            _ => simple("OK"),
        }
    }

    #[test]
    fn eval_encodes_keys_and_args() {
        let (client, received) = client_for(Arc::new(script_handler));
        let reply = client
            .eval("return 1", &["k1", "k2"], &["a1"])
            .expect("eval");
        assert_eq!(reply, Reply::Integer(1));
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["EVAL", "return 1", "2", "k1", "k2", "a1"]);
        let reply = client.eval("return 'x'", &[], &[]).expect("eval no keys");
        assert_eq!(reply, Reply::Bulk(Some(b"ok".to_vec())));
        client.close();
    }

    #[test]
    fn eval_sha_runs_cached_script() {
        let (client, received) = client_for(Arc::new(script_handler));
        let reply = client
            .eval_sha("sha1hash", &["k"], &["v"])
            .expect("evalsha");
        assert_eq!(reply, Reply::Bulk(Some(b"sha-ok".to_vec())));
        assert_eq!(
            received.lock().unwrap()[0],
            vec!["EVALSHA", "sha1hash", "1", "k", "v"]
        );
        client.close();
    }

    #[test]
    fn script_management_commands() {
        let (client, received) = client_for(Arc::new(script_handler));
        assert_eq!(client.script_load("return 1").expect("load"), "sha1hash");
        assert_eq!(
            client
                .script_exists(&["sha1hash", "missing"])
                .expect("exists"),
            vec![true, false]
        );
        client.script_flush().expect("flush");
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["SCRIPT", "LOAD", "return 1"]);
        assert_eq!(commands[1], vec!["SCRIPT", "EXISTS", "sha1hash", "missing"]);
        assert_eq!(commands[2], vec!["SCRIPT", "FLUSH"]);
        client.close();
    }

    #[test]
    fn resp3_types_decode() {
        // Map: {"a": 1, "b": 2}.
        let (reply, consumed) =
            decode_reply(b"%2\r\n$1\r\na\r\n:1\r\n$1\r\nb\r\n:2\r\n").expect("map");
        assert_eq!(consumed, 26);
        assert_eq!(
            reply.pairs(),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );

        let (reply, _) = decode_reply(b"~2\r\n$1\r\na\r\n$1\r\nb\r\n").expect("set");
        assert_eq!(reply.strings(), vec!["a", "b"]);

        let (reply, _) = decode_reply(b",3.5\r\n").expect("double");
        assert_eq!(reply, Reply::Double(3.5));
        assert_eq!(reply.int(), 3);

        let (reply, _) = decode_reply(b"#t\r\n").expect("true");
        assert_eq!(reply, Reply::Boolean(true));
        let (reply, _) = decode_reply(b"#f\r\n").expect("false");
        assert_eq!(reply, Reply::Boolean(false));
        assert_eq!(reply.int(), 0);

        let (reply, _) = decode_reply(b"(12345678901234567890\r\n").expect("big");
        assert_eq!(reply.string(), "12345678901234567890");

        let (reply, _) = decode_reply(b"_\r\n").expect("null");
        assert_eq!(reply, Reply::Null);
        assert_eq!(reply.string(), "");

        let (reply, _) = decode_reply(b">2\r\n$7\r\nmessage\r\n$5\r\nhello\r\n").expect("push");
        assert_eq!(reply.string(), "message,hello");

        let (reply, _) = decode_reply(b"!9\r\nERR nope!\r\n").expect("blob error");
        assert_eq!(reply.error_text(), Some("ERR nope!"));

        let (reply, _) = decode_reply(b"=9\r\ntxt:hello\r\n").expect("verbatim");
        assert_eq!(reply.bulk().as_deref(), Some("txt:hello"));
    }

    #[test]
    fn hello_negotiates_resp3() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(|args: Vec<String>| {
            if args.first().map(String::as_str) == Some("HELLO") {
                let mut reply = Vec::new();
                reply.extend_from_slice(b"%3\r\n");
                reply.extend_from_slice(&bulk("server"));
                reply.extend_from_slice(&bulk("redis"));
                reply.extend_from_slice(&bulk("proto"));
                reply.extend_from_slice(&integer(3));
                reply.extend_from_slice(&bulk("role"));
                reply.extend_from_slice(&bulk("master"));
                reply
            } else {
                simple("OK")
            }
        });
        let address = spawn_fake(handler, Arc::clone(&received));
        let client = Client::new(Config::new(&address)).expect("client");
        let reply = client.resp3().expect("hello 3");
        let mut pairs = reply.pairs();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("proto".to_owned(), "3".to_owned()),
                ("role".to_owned(), "master".to_owned()),
                ("server".to_owned(), "redis".to_owned()),
            ]
        );
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["HELLO", "3"]);
        client.close();
    }

    #[test]
    fn sentinel_resolves_and_connects_to_master() {
        let master_received = Arc::new(Mutex::new(Vec::new()));
        let master = spawn_fake(Arc::new(ok_handler), Arc::clone(&master_received));
        let sentinel_received = Arc::new(Mutex::new(Vec::new()));
        let master_address = master.clone();
        let handler = Arc::new(move |args: Vec<String>| {
            if args.first().map(String::as_str) == Some("SENTINEL") {
                let port = master_address
                    .rsplit(':')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                array(&[bulk("127.0.0.1"), bulk(&port)])
            } else {
                simple("OK")
            }
        });
        let sentinel = spawn_fake(handler, Arc::clone(&sentinel_received));
        let client = Client::new(Config::new(&sentinel).with_master_name("mymaster"))
            .expect("sentinel client");
        assert_eq!(client.ping().expect("ping"), "PONG");
        let commands = sentinel_received.lock().unwrap().clone();
        assert_eq!(
            commands[0],
            vec!["SENTINEL", "get-master-addr-by-name", "mymaster"]
        );
        client.close();
    }

    #[test]
    fn sentinel_error_is_reported() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(|args: Vec<String>| {
            if args.first().map(String::as_str) == Some("SENTINEL") {
                b"-ERR unknown master\r\n".to_vec()
            } else {
                simple("OK")
            }
        });
        let address = spawn_fake(handler, received);
        let error = match Client::new(Config::new(&address).with_master_name("missing")) {
            Ok(_) => panic!("sentinel should fail"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("sentinel"),
            "unexpected error: {error}"
        );
    }

    /// Builds a `CLUSTER SLOTS` reply for the given ranges.
    fn cluster_slots_reply(ranges: &[(u16, u16, String)]) -> Vec<u8> {
        let mut out = format!("*{}\r\n", ranges.len()).into_bytes();
        for (start, end, address) in ranges {
            let (host, port) = address.rsplit_once(':').unwrap_or((address, "0"));
            out.extend_from_slice(b"*3\r\n");
            out.extend_from_slice(&integer(*start as i64));
            out.extend_from_slice(&integer(*end as i64));
            out.extend_from_slice(&array(&[bulk(host), bulk(port), bulk("node-id")]));
        }
        out
    }

    #[test]
    fn key_slots_match_redis() {
        assert_eq!(key_slot("foo"), 12182);
        assert_eq!(key_slot("bar"), 5061);
        assert_eq!(
            key_slot("{user1000}.following"),
            key_slot("{user1000}.followers")
        );
        assert_eq!(key_slot("{}foo"), key_slot("{}foo"));
        // Empty hashtags hash the whole key.
        assert_ne!(key_slot("{}foo"), key_slot("{}bar"));
    }

    #[test]
    fn cluster_routes_commands_by_slot() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let self_address = Arc::new(Mutex::new(String::new()));
        let cell = Arc::clone(&self_address);
        let handler = Arc::new(
            move |args: Vec<String>| match args.first().map(String::as_str) {
                Some("CLUSTER") => {
                    let address = cell.lock().unwrap().clone();
                    cluster_slots_reply(&[(0, 16_383, address)])
                }
                Some("SET") => simple("OK"),
                Some("GET") => bulk("1"),
                _ => simple("OK"),
            },
        );
        let address = spawn_fake(handler, Arc::clone(&received));
        *self_address.lock().unwrap() = address.clone();
        let client =
            ClusterClient::new(Config::new(&address).with_cluster(true)).expect("cluster client");
        assert_eq!(client.slots().len(), 1);
        assert_eq!(
            client.command(&["SET", "foo", "1"]).expect("set").string(),
            "OK"
        );
        assert_eq!(
            client
                .command(&["GET", "foo"])
                .expect("get")
                .bulk()
                .as_deref(),
            Some("1")
        );
        let commands = received.lock().unwrap().clone();
        assert_eq!(commands[0], vec!["CLUSTER", "SLOTS"]);
        assert!(commands.iter().any(|command| command[0] == "SET"));
        client.close();
    }

    #[test]
    fn cluster_follows_moved_redirection() {
        // Node B owns the upper half.
        let received_b = Arc::new(Mutex::new(Vec::new()));
        let node_b = spawn_fake(
            Arc::new(|_args: Vec<String>| simple("OK")),
            Arc::clone(&received_b),
        );
        // Node A owns everything at first, then redirects to B.
        let received_a = Arc::new(Mutex::new(Vec::new()));
        let self_address = Arc::new(Mutex::new(String::new()));
        let cell = Arc::clone(&self_address);
        let node_b_address = node_b.clone();
        let moved = Arc::new(Mutex::new(false));
        let moved_flag = Arc::clone(&moved);
        let handler = Arc::new(
            move |args: Vec<String>| match args.first().map(String::as_str) {
                Some("CLUSTER") => {
                    let address = cell.lock().unwrap().clone();
                    if *moved_flag.lock().unwrap() {
                        cluster_slots_reply(&[
                            (0, 8191, address),
                            (8192, 16_383, node_b_address.clone()),
                        ])
                    } else {
                        cluster_slots_reply(&[(0, 16_383, address)])
                    }
                }
                Some("SET") => {
                    if *moved_flag.lock().unwrap() {
                        simple("OK")
                    } else {
                        *moved_flag.lock().unwrap() = true;
                        format!("-MOVED 12182 {node_b_address}\r\n").into_bytes()
                    }
                }
                _ => simple("OK"),
            },
        );
        let address_a = spawn_fake(handler, Arc::clone(&received_a));
        *self_address.lock().unwrap() = address_a.clone();
        let client =
            ClusterClient::new(Config::new(&address_a).with_cluster(true)).expect("cluster client");
        assert_eq!(
            client
                .command(&["SET", "foo", "1"])
                .expect("moved retry")
                .string(),
            "OK"
        );
        assert!(
            received_b
                .lock()
                .unwrap()
                .iter()
                .any(|command| command[0] == "SET"),
            "the command should be retried on the redirected node"
        );
        assert_eq!(client.slots().len(), 2);
        client.close();
    }

    #[test]
    fn pool_reuses_idle_connections() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let address = spawn_fake(Arc::new(ok_handler), Arc::clone(&received));
        let config = Config::new(&address).with_pool(1, 2, 1);
        let pool = RedisPool::new(config).expect("pool");
        assert_eq!(pool.total(), 1);
        assert_eq!(pool.idle_count(), 1);

        let first = pool.get().expect("first");
        assert_eq!(first.ping().expect("ping"), "PONG");
        assert_eq!(pool.idle_count(), 0);
        let first_ptr = Arc::as_ptr(first.client());
        drop(first);
        assert_eq!(pool.idle_count(), 1);

        let second = pool.get().expect("reused");
        assert_eq!(Arc::as_ptr(second.client()), first_ptr);
        assert_eq!(pool.total(), 1);
        drop(second);
        pool.close();
    }

    #[test]
    fn pool_limits_open_connections() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let address = spawn_fake(Arc::new(ok_handler), Arc::clone(&received));
        let config = Config::new(&address)
            .with_pool(0, 2, 1)
            .with_wait_timeout(Duration::from_millis(30));
        let pool = RedisPool::new(config).expect("pool");
        let first = pool.get().expect("first");
        let error = match pool.get() {
            Ok(_) => panic!("the pool should be exhausted"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("timed out"),
            "unexpected error: {error}"
        );
        drop(first);
        let second = pool.get().expect("after return");
        assert!(second.ping().is_ok());
        drop(second);
        pool.close();
    }

    #[test]
    fn pool_expires_idle_connections() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let address = spawn_fake(Arc::new(ok_handler), Arc::clone(&received));
        let config = Config::new(&address)
            .with_pool(0, 2, 0)
            .with_pool_timeouts(Duration::from_secs(30), Duration::from_millis(20));
        let pool = RedisPool::new(config).expect("pool");
        let first = pool.get().expect("first");
        drop(first);
        assert_eq!(pool.created_count(), 1);
        std::thread::sleep(Duration::from_millis(40));
        let second = pool.get().expect("fresh");
        assert_eq!(pool.created_count(), 2);
        assert_eq!(pool.total(), 1);
        drop(second);
        pool.close();
    }
}
