//! Redis-backed session storage, porting `gsession.StorageRedis` (ADR-175).
//!
//! One Redis string per session holds the JSON data map under `prefix +
//! session_id`; TTLs use `SET ... PX` and `PEXPIRE`. GoFrame batches TTL
//! refresh through a timer; the port issues `PEXPIRE` immediately.

use std::time::Duration;

use gf_core::Value;
use gf_errors::gerror::{self, Error};
use gf_os::gsession::{Storage, new_session_id};
use gf_util::gconv;
use indexmap::IndexMap;

use crate::gredis::{Client, Config, Reply};

/// Session storage backed by Redis, like `StorageRedis`.
pub struct RedisStorage {
    client: Client,
    prefix: String,
}

impl RedisStorage {
    /// Wraps `client`, prefixing keys with `prefix`.
    pub fn new(client: Client, prefix: Option<&str>) -> RedisStorage {
        RedisStorage {
            client,
            prefix: prefix.unwrap_or_default().to_owned(),
        }
    }

    /// Connects with `config`, prefixing keys with `prefix`.
    pub fn with_config(config: Config, prefix: Option<&str>) -> Result<RedisStorage, Error> {
        Ok(RedisStorage::new(Client::new(config)?, prefix))
    }

    fn command(&self, args: Vec<String>) -> Result<Reply, Error> {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.client.command(&borrowed)
    }

    fn redis_key(&self, session_id: &str) -> String {
        format!("{}{session_id}", self.prefix)
    }

    fn load(&self, session_id: &str) -> Result<Option<IndexMap<String, Value>>, Error> {
        let reply = self.command(vec!["GET".to_owned(), self.redis_key(session_id)])?;
        let Some(text) = reply.bulk() else {
            return Ok(None);
        };
        let value = gf_json::unmarshal_value(text.as_bytes())?;
        Ok(Some(gconv::map(&value)))
    }

    fn store_with_ttl(
        &self,
        session_id: &str,
        data: &IndexMap<String, Value>,
        ttl_millis: Option<u64>,
    ) -> Result<(), Error> {
        let text = gf_json::marshal_value(&Value::Map(data.clone()))?;
        let mut args = vec!["SET".to_owned(), self.redis_key(session_id), text];
        if let Some(millis) = ttl_millis {
            args.push("PX".to_owned());
            args.push(millis.max(1).to_string());
        }
        self.command(args)?;
        Ok(())
    }

    fn store(
        &self,
        session_id: &str,
        data: &IndexMap<String, Value>,
        ttl: Duration,
    ) -> Result<(), Error> {
        self.store_with_ttl(session_id, data, Some(ttl.as_millis() as u64))
    }
}

impl Storage for RedisStorage {
    fn new_session(&self, _ttl: Duration) -> Result<String, Error> {
        Ok(new_session_id())
    }

    fn remove_all(&self, session_id: &str) -> Result<(), Error> {
        self.command(vec!["DEL".to_owned(), self.redis_key(session_id)])?;
        Ok(())
    }

    fn get_session(
        &self,
        session_id: &str,
        _ttl: Duration,
    ) -> Result<Option<IndexMap<String, Value>>, Error> {
        self.load(session_id)
    }

    fn set_session(
        &self,
        session_id: &str,
        data: &IndexMap<String, Value>,
        ttl: Duration,
    ) -> Result<(), Error> {
        if data.is_empty() {
            return self.remove_all(session_id);
        }
        self.store(session_id, data, ttl)
    }

    fn update_ttl(&self, session_id: &str, ttl: Duration) -> Result<(), Error> {
        self.command(vec![
            "PEXPIRE".to_owned(),
            self.redis_key(session_id),
            ttl.as_millis().max(1).to_string(),
        ])?;
        Ok(())
    }
}
/// Creates a Redis session storage, like `NewStorageRedis`.
pub fn new_storage_redis(config: Config, prefix: Option<&str>) -> Result<RedisStorage, Error> {
    RedisStorage::with_config(config, prefix)
        .map_err(|error| gerror::new(format!("create redis session storage failed: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;
    use gf_os::gsession::Manager;

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
        let count: usize = line.trim().trim_start_matches('*').parse().ok()?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            let mut length_line = String::new();
            reader.read_line(&mut length_line).ok()?;
            let length: usize = length_line.trim().trim_start_matches('$').parse().ok()?;
            let mut buffer = vec![0u8; length + 2];
            std::io::Read::read_exact(reader, &mut buffer).ok()?;
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

    fn dispatch(state: &Store, args: &[String]) -> Vec<u8> {
        let mut map = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match args[0].to_ascii_uppercase().as_str() {
            "SET" => {
                let mut expires = None;
                if args.len() > 4 && args[3].eq_ignore_ascii_case("PX") {
                    let millis: u64 = args[4].parse().unwrap_or(1);
                    expires = Some(Instant::now() + Duration::from_millis(millis));
                }
                map.insert(args[1].clone(), (args[2].clone(), expires));
                simple("OK")
            }
            "GET" => match map.get(&args[1]) {
                Some((value, _)) => bulk(value),
                None => nil(),
            },
            "DEL" => {
                let removed = map.remove(&args[1]).is_some() as i64;
                integer(removed)
            }
            "EXISTS" => integer(map.contains_key(&args[1]) as i64),
            "PEXPIRE" => {
                let millis: u64 = args[2].parse().unwrap_or(1);
                match map.get_mut(&args[1]) {
                    Some(entry) => {
                        entry.1 = Some(Instant::now() + Duration::from_millis(millis));
                        integer(1)
                    }
                    None => integer(0),
                }
            }
            "PTTL" => match map.get(&args[1]) {
                None => integer(-2),
                Some((_, None)) => integer(-1),
                Some((_, Some(expires))) => integer(
                    expires
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .max(1) as i64,
                ),
            },
            _ => simple("OK"),
        }
    }

    #[test]
    fn redis_session_round_trip() {
        let (address, store) = spawn_redis();
        let storage = Arc::new(
            RedisStorage::with_config(Config::new(&address), Some("sess:")).expect("storage"),
        );
        let manager = Arc::new(
            Manager::new(
                Duration::from_secs(60),
                Some(Arc::clone(&storage) as Arc<dyn Storage>),
            )
            .expect("manager"),
        );
        let mut session = manager.new_session(None);
        session
            .set("user", Value::String("gf".to_owned()))
            .expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");
        assert!(
            store.lock().unwrap().contains_key(&format!("sess:{id}")),
            "the session must be stored under the configured prefix"
        );

        let mut restored = manager.new_session(Some(&id));
        assert_eq!(
            restored
                .get("user", None)
                .expect("get")
                .expect("value")
                .string(),
            "gf"
        );
        restored.remove(&["user"]).expect("remove key");
        assert!(restored.get("user", None).expect("get").is_none());
        restored.remove_all().expect("remove all");
        assert!(!store.lock().unwrap().contains_key(&format!("sess:{id}")));
    }

    #[test]
    fn redis_session_update_ttl() {
        let (address, store) = spawn_redis();
        let storage =
            Arc::new(RedisStorage::with_config(Config::new(&address), None).expect("storage"));
        let manager = Arc::new(
            Manager::new(
                Duration::from_secs(60),
                Some(Arc::clone(&storage) as Arc<dyn Storage>),
            )
            .expect("manager"),
        );
        let mut session = manager.new_session(None);
        session.set("k", Value::Int(1)).expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");

        let mut again = manager.new_session(Some(&id));
        again.close().expect("close again");
        let store = store.lock().unwrap();
        let (_, expires) = store.get(&id).expect("entry");
        assert!(expires.is_some(), "update_ttl must keep a TTL");
    }
}
