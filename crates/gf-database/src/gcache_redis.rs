//! Redis-backed cache adapter, porting `gcache.AdapterRedis` (ADR-174).
//!
//! Keys are the cache keys converted with `gconv::string` (Go's
//! `gconv.String`), values are stored as JSON text so types survive the round
//! trip (GoFrame stores the raw value through `gredis`), and TTLs use
//! `SET ... PX` / `PEXPIRE` milliseconds.

use std::time::Duration;

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gerror::{self, Error};
use gf_os::gcache::{Adapter, Func, Key, Ttl};
use gf_util::gconv;

use crate::gredis::{Client, Config, Reply};

/// A cache adapter backed by one Redis client, like `AdapterRedis`.
pub struct RedisAdapter {
    client: Client,
}

impl RedisAdapter {
    /// Wraps an existing client.
    pub fn new(client: Client) -> RedisAdapter {
        RedisAdapter { client }
    }

    /// Connects with `config`.
    pub fn with_config(config: Config) -> Result<RedisAdapter, Error> {
        Ok(RedisAdapter::new(Client::new(config)?))
    }

    fn command(&self, args: Vec<String>) -> Result<Reply, Error> {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.client.command(&borrowed)
    }

    fn key_string(key: &Key) -> String {
        gconv::string(key.value())
    }

    fn encode(value: &Value) -> Result<String, Error> {
        gf_json::marshal_value(value)
    }

    fn decode(text: &str) -> Result<Value, Error> {
        gf_json::unmarshal_value(text.as_bytes())
    }

    fn ttl_args(ttl: &Ttl) -> Vec<String> {
        match ttl {
            Ttl::After(duration) => vec!["PX".to_owned(), duration.as_millis().max(1).to_string()],
            Ttl::Never | Ttl::Remove => Vec::new(),
        }
    }

    fn old_value(&self, key: &str) -> Result<Option<Var>, Error> {
        let reply = self.command(vec!["GET".to_owned(), key.to_owned()])?;
        let Some(text) = reply.bulk() else {
            return Ok(None);
        };
        Ok(Some(Var::new(Self::decode(&text)?)))
    }
}

impl Adapter for RedisAdapter {
    fn set(&self, key: Key, value: Value, ttl: Ttl) -> Result<(), Error> {
        let key = Self::key_string(&key);
        if matches!(ttl, Ttl::Remove) || value.is_nil() {
            self.command(vec!["DEL".to_owned(), key])?;
            return Ok(());
        }
        let mut args = vec!["SET".to_owned(), key, Self::encode(&value)?];
        args.extend(Self::ttl_args(&ttl));
        self.command(args)?;
        Ok(())
    }

    fn set_map(&self, data: Vec<(Key, Value)>, ttl: Ttl) -> Result<(), Error> {
        for (key, value) in data {
            self.set(key, value, ttl)?;
        }
        Ok(())
    }

    fn set_if_not_exist(&self, key: Key, value: Value, ttl: Ttl) -> Result<bool, Error> {
        let key = Self::key_string(&key);
        if matches!(ttl, Ttl::Remove) || value.is_nil() {
            self.command(vec!["DEL".to_owned(), key])?;
            return Ok(false);
        }
        let mut args = vec![
            "SET".to_owned(),
            key,
            Self::encode(&value)?,
            "NX".to_owned(),
        ];
        args.extend(Self::ttl_args(&ttl));
        let reply = self.command(args)?;
        Ok(matches!(reply, Reply::Simple(_) | Reply::Bulk(Some(_))))
    }

    fn set_if_not_exist_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<bool, Error> {
        if self.contains(&key)? {
            return Ok(false);
        }
        let value = function()?;
        self.set_if_not_exist(key, value, ttl)
    }

    fn get(&self, key: &Key) -> Result<Option<Var>, Error> {
        self.old_value(&Self::key_string(key))
    }

    fn get_or_set(&self, key: Key, value: Value, ttl: Ttl) -> Result<Option<Var>, Error> {
        if let Some(found) = self.get(&key)? {
            return Ok(Some(found));
        }
        if value.is_nil() || matches!(ttl, Ttl::Remove) {
            return Ok(None);
        }
        self.set(key, value.clone(), ttl)?;
        Ok(Some(Var::new(value)))
    }

    fn get_or_set_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<Option<Var>, Error> {
        if let Some(found) = self.get(&key)? {
            return Ok(Some(found));
        }
        let value = function()?;
        if value.is_nil() {
            return Ok(None);
        }
        self.set(key, value.clone(), ttl)?;
        Ok(Some(Var::new(value)))
    }

    fn contains(&self, key: &Key) -> Result<bool, Error> {
        let reply = self.command(vec!["EXISTS".to_owned(), Self::key_string(key)])?;
        Ok(reply.int() > 0)
    }

    fn get_expire(&self, key: &Key) -> Result<Option<Duration>, Error> {
        let reply = self.command(vec!["PTTL".to_owned(), Self::key_string(key)])?;
        let millis = reply.int();
        if millis > 0 {
            Ok(Some(Duration::from_millis(millis as u64)))
        } else {
            Ok(None)
        }
    }

    fn remove(&self, keys: &[Key]) -> Result<Option<Var>, Error> {
        let mut last = None;
        for key in keys {
            let text = Self::key_string(key);
            if let Some(value) = self.old_value(&text)? {
                last = Some(value);
            }
            self.command(vec!["DEL".to_owned(), text])?;
        }
        Ok(last)
    }

    fn update(&self, key: &Key, value: Value) -> Result<(Option<Var>, bool), Error> {
        let text = Self::key_string(key);
        let ttl_reply = self.command(vec!["PTTL".to_owned(), text.clone()])?;
        let millis = ttl_reply.int();
        if millis == -2 {
            return Ok((None, false));
        }
        let old = self.old_value(&text)?;
        let Some(old) = old else {
            return Ok((None, false));
        };
        let mut args = vec!["SET".to_owned(), text, Self::encode(&value)?];
        if millis > 0 {
            args.push("PX".to_owned());
            args.push(millis.to_string());
        }
        self.command(args)?;
        Ok((Some(old), true))
    }

    fn update_expire(&self, key: &Key, ttl: Ttl) -> Result<Option<Duration>, Error> {
        let text = Self::key_string(key);
        let old = self.command(vec!["PTTL".to_owned(), text.clone()])?.int();
        if old == -2 {
            return Ok(None);
        }
        let previous = if old > 0 {
            Some(Duration::from_millis(old as u64))
        } else {
            None
        };
        match ttl {
            Ttl::Remove => {
                self.command(vec!["DEL".to_owned(), Self::key_string(key)])?;
            }
            Ttl::Never => {
                self.command(vec!["PERSIST".to_owned(), Self::key_string(key)])?;
            }
            Ttl::After(duration) => {
                self.command(vec![
                    "PEXPIRE".to_owned(),
                    Self::key_string(key),
                    duration.as_millis().max(1).to_string(),
                ])?;
            }
        }
        Ok(previous)
    }

    fn size(&self) -> Result<usize, Error> {
        Ok(self.command(vec!["DBSIZE".to_owned()])?.int().max(0) as usize)
    }

    fn data(&self) -> Result<Vec<(Value, Value)>, Error> {
        let mut entries = Vec::new();
        for key in self.keys()? {
            let text = gconv::string(&key);
            if let Some(value) = self.old_value(&text)? {
                entries.push((key, value.val()));
            }
        }
        Ok(entries)
    }

    fn keys(&self) -> Result<Vec<Value>, Error> {
        let reply = self.command(vec!["KEYS".to_owned(), "*".to_owned()])?;
        let mut keys = Vec::new();
        if let Reply::Array(Some(items)) = reply {
            for item in items {
                if let Some(text) = item.bulk() {
                    keys.push(Value::String(text));
                }
            }
        }
        Ok(keys)
    }

    fn values(&self) -> Result<Vec<Value>, Error> {
        Ok(self.data()?.into_iter().map(|(_, value)| value).collect())
    }

    fn clear(&self) -> Result<(), Error> {
        self.command(vec!["FLUSHDB".to_owned()])?;
        Ok(())
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// Creates a Redis adapter from `config`, like `NewAdapterRedis`.
pub fn new_adapter_redis(config: Config) -> Result<RedisAdapter, Error> {
    RedisAdapter::with_config(config)
        .map_err(|error| gerror::new(format!("create redis cache adapter failed: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;

    type Store = Arc<Mutex<HashMap<String, (String, Option<Instant>)>>>;

    fn spawn_redis() -> (String, Store) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address").to_string();
        let store: Store = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::clone(&store);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let state = Arc::clone(&state);
                std::thread::spawn(move || serve(stream, state));
            }
        });
        (address, store)
    }

    fn serve(mut stream: TcpStream, state: Store) {
        let Ok(reader_stream) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(reader_stream);
        while let Some(args) = read_command(&mut reader) {
            if args.is_empty() {
                break;
            }
            let response = dispatch(&state, &args);
            if stream.write_all(&response).is_err() || stream.flush().is_err() {
                break;
            }
        }
    }

    fn read_command(reader: &mut impl BufRead) -> Option<Vec<String>> {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if !line.starts_with('*') {
            return None;
        }
        let count: usize = line[1..].parse().ok()?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            let mut length_line = String::new();
            reader.read_line(&mut length_line).ok()?;
            let length: usize = length_line.trim().trim_start_matches('$').parse().ok()?;
            let mut buffer = vec![0u8; length + 2];
            reader.read_exact(&mut buffer).ok()?;
            args.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
        }
        Some(args)
    }

    fn bulk(text: &str) -> Vec<u8> {
        format!("${}\r\n{}\r\n", text.len(), text).into_bytes()
    }

    fn nil() -> Vec<u8> {
        b"$-1\r\n".to_vec()
    }

    fn integer(value: i64) -> Vec<u8> {
        format!(":{value}\r\n").into_bytes()
    }

    fn simple(value: &str) -> Vec<u8> {
        format!("+{value}\r\n").into_bytes()
    }

    fn purge(map: &mut HashMap<String, (String, Option<Instant>)>, key: &str) {
        if let Some((_, Some(expires))) = map.get(key)
            && *expires <= Instant::now()
        {
            map.remove(key);
        }
    }

    fn dispatch(state: &Store, args: &[String]) -> Vec<u8> {
        let mut map = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match args[0].to_ascii_uppercase().as_str() {
            "SET" => {
                let key = args[1].clone();
                let value = args[2].clone();
                let mut expires = None;
                let mut only_if_absent = false;
                let mut index = 3;
                while index < args.len() {
                    match args[index].to_ascii_uppercase().as_str() {
                        "PX" => {
                            let millis: u64 = args[index + 1].parse().unwrap_or(1);
                            expires = Some(Instant::now() + Duration::from_millis(millis));
                            index += 2;
                        }
                        "NX" => {
                            only_if_absent = true;
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
                purge(&mut map, &key);
                if only_if_absent && map.contains_key(&key) {
                    return nil();
                }
                map.insert(key, (value, expires));
                simple("OK")
            }
            "GET" => {
                let key = &args[1];
                purge(&mut map, key);
                map.get(key)
                    .map(|(value, _)| bulk(value))
                    .unwrap_or_else(nil)
            }
            "DEL" => {
                let mut removed = 0;
                for key in &args[1..] {
                    purge(&mut map, key);
                    if map.remove(key).is_some() {
                        removed += 1;
                    }
                }
                integer(removed)
            }
            "EXISTS" => {
                let key = &args[1];
                purge(&mut map, key);
                integer(map.contains_key(key) as i64)
            }
            "PTTL" => {
                let key = &args[1];
                purge(&mut map, key);
                match map.get(key) {
                    None => integer(-2),
                    Some((_, None)) => integer(-1),
                    Some((_, Some(expires))) => integer(
                        expires
                            .saturating_duration_since(Instant::now())
                            .as_millis()
                            .max(1) as i64,
                    ),
                }
            }
            "PEXPIRE" => {
                let key = &args[1];
                let millis: u64 = args[2].parse().unwrap_or(1);
                purge(&mut map, key);
                match map.get_mut(key) {
                    Some(entry) => {
                        entry.1 = Some(Instant::now() + Duration::from_millis(millis));
                        integer(1)
                    }
                    None => integer(0),
                }
            }
            "PERSIST" => {
                let key = &args[1];
                purge(&mut map, key);
                match map.get_mut(key) {
                    Some(entry) => {
                        entry.1 = None;
                        integer(1)
                    }
                    None => integer(0),
                }
            }
            "DBSIZE" => {
                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    purge(&mut map, &key);
                }
                integer(map.len() as i64)
            }
            "KEYS" => {
                let keys: Vec<String> = map.keys().cloned().collect();
                let mut out = format!("*{}\r\n", keys.len()).into_bytes();
                for key in keys {
                    out.extend(bulk(&key));
                }
                out
            }
            "FLUSHDB" => {
                map.clear();
                simple("OK")
            }
            _ => simple("OK"),
        }
    }

    #[test]
    fn redis_adapter_round_trip() {
        let (address, _store) = spawn_redis();
        let adapter = RedisAdapter::with_config(Config::new(&address)).expect("adapter");
        let key = Key::from("user");
        adapter
            .set(key.clone(), Value::String("gf".to_owned()), Ttl::Never)
            .expect("set");
        assert!(adapter.contains(&key).expect("contains"));
        assert_eq!(
            adapter.get(&key).expect("get").expect("value").string(),
            "gf"
        );

        adapter
            .set(
                key.clone(),
                Value::Int(1),
                Ttl::After(Duration::from_secs(60)),
            )
            .expect("set ttl");
        let expire = adapter.get_expire(&key).expect("expire").expect("some");
        assert!(
            expire <= Duration::from_secs(60) && expire > Duration::from_secs(50),
            "{expire:?}"
        );
        let (old, existed) = adapter.update(&key, Value::Int(2)).expect("update");
        assert!(existed);
        assert_eq!(old.expect("old").int64(), 1);
        assert!(
            adapter
                .get_expire(&key)
                .expect("expire")
                .expect("still ttl")
                > Duration::from_secs(50)
        );
        assert!(
            adapter
                .update_expire(&key, Ttl::Never)
                .expect("update expire")
                .is_some()
        );
        assert!(adapter.get_expire(&key).expect("expire").is_none());

        let second = Key::from("name");
        adapter
            .set(second.clone(), Value::String("n".to_owned()), Ttl::Never)
            .expect("set second");
        assert_eq!(adapter.size().expect("size"), 2);
        assert_eq!(adapter.keys().expect("keys").len(), 2);
        assert_eq!(adapter.data().expect("data").len(), 2);
        assert_eq!(adapter.values().expect("values").len(), 2);

        let fresh = Key::from("fresh");
        assert!(
            adapter
                .set_if_not_exist(fresh.clone(), Value::Int(5), Ttl::Never)
                .expect("nx")
        );
        assert!(
            !adapter
                .set_if_not_exist(fresh.clone(), Value::Int(6), Ttl::Never)
                .expect("nx again")
        );

        assert!(
            adapter
                .remove(std::slice::from_ref(&key))
                .expect("remove")
                .is_some()
        );
        assert!(!adapter.contains(&key).expect("contains"));

        adapter.clear().expect("clear");
        assert_eq!(adapter.size().expect("size"), 0);
        adapter.close().expect("close");
    }

    #[test]
    fn redis_adapter_expires_entries() {
        let (address, _store) = spawn_redis();
        let adapter = RedisAdapter::with_config(Config::new(&address)).expect("adapter");
        let key = Key::from("short");
        adapter
            .set(
                key.clone(),
                Value::Int(1),
                Ttl::After(Duration::from_millis(40)),
            )
            .expect("set");
        assert!(adapter.get(&key).expect("get").is_some());
        std::thread::sleep(Duration::from_millis(90));
        assert!(adapter.get(&key).expect("get after expiry").is_none());
    }

    #[test]
    fn redis_adapter_get_or_set_func() {
        let (address, _store) = spawn_redis();
        let adapter = RedisAdapter::with_config(Config::new(&address)).expect("adapter");
        let key = Key::from("computed");
        let first = adapter
            .get_or_set_func(key.clone(), Arc::new(|| Ok(Value::Int(7))), Ttl::Never)
            .expect("first");
        assert_eq!(first.expect("value").int64(), 7);
        let second = adapter
            .get_or_set_func(key.clone(), Arc::new(|| Ok(Value::Int(8))), Ttl::Never)
            .expect("second");
        assert_eq!(second.expect("value").int64(), 7);
    }
}
