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
//! Not ported: cluster/sentinel, RESP3, pipelining, transactions, pub/sub
//! subscriptions, scripting and the `go-redis` option surface.

use std::sync::Mutex;
use std::time::Duration;

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
}

impl Default for Config {
    fn default() -> Config {
        Config {
            address: DEFAULT_ADDRESS.to_owned(),
            password: String::new(),
            db: 0,
            timeout: DEFAULT_TIMEOUT,
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
        }
    }

    /// Returns the bulk value, or `None` for nil/non-bulk replies.
    pub fn bulk(&self) -> Option<String> {
        match self {
            Reply::Bulk(Some(bytes)) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        }
    }

    /// Converts the reply into an integer (0 when not an integer/bulk number).
    pub fn int(&self) -> i64 {
        match self {
            Reply::Integer(value) => *value,
            Reply::Simple(value) => value.parse::<i64>().unwrap_or(0),
            Reply::Bulk(Some(bytes)) => String::from_utf8_lossy(bytes).parse::<i64>().unwrap_or(0),
            _ => 0,
        }
    }

    /// Converts an array reply to strings (strings are empty on nil).
    pub fn strings(&self) -> Vec<String> {
        match self {
            Reply::Array(Some(items)) => items.iter().map(Reply::string).collect(),
            _ => Vec::new(),
        }
    }

    /// Converts a flat array reply to `(field, value)` pairs.
    pub fn pairs(&self) -> Vec<(String, String)> {
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
}
