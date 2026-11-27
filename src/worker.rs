//! Talks to the Redis/Valkey server on a background thread so the window never freezes.

use crate::config::{ConnCfg, ServerKind};
use eframe::egui;
use redis::{Connection, ConnectionAddr, IntoConnectionInfo, RedisConnectionInfo, Value};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// How many items of a big collection are shown.
pub const ITEM_LIMIT: usize = 500;
/// Roughly how many keys one "load more" brings in.
const PAGE_KEYS: usize = 200;

pub type Bytes = Vec<u8>;

#[derive(Clone, Debug)]
pub struct ServerMeta {
    pub kind: &'static str,
    pub version: String,
    pub mode: String,
    pub dbsize: i64,
    pub databases: i64,
    pub mismatch: Option<String>,
}

#[derive(Clone, Debug)]
pub struct KeyRow {
    pub key: Bytes,
    pub name: String,
    pub kind: String,
    pub ttl_ms: i64,
}

#[derive(Clone, Debug)]
pub enum ValueData {
    Str(String),
    List(Vec<String>),
    Set(Vec<String>),
    ZSet(Vec<(String, f64)>),
    Hash(Vec<(String, String)>),
    Text(String),
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct KeyDetail {
    pub key: Bytes,
    pub name: String,
    pub kind: String,
    pub ttl_ms: i64,
    pub len: i64,
    pub memory: Option<i64>,
    pub value: ValueData,
}

#[derive(Clone, Debug)]
pub struct InfoSnapshot {
    pub sections: Vec<(String, Vec<(String, String)>)>,
    pub at: Instant,
}

impl InfoSnapshot {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .flat_map(|(_, kv)| kv.iter())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug)]
pub enum After {
    ReloadKey(Bytes),
    KeyDeleted(Bytes),
    KeyRenamed(Bytes),
    KeyCreated(Bytes),
}

pub enum Job {
    Connect(ConnCfg),
    Disconnect,
    Scan {
        pattern: String,
        kind: Option<String>,
        cursor: u64,
        reset: bool,
    },
    Load(Bytes),
    /// Run commands one after another; optionally refuse if a key already exists.
    Ops {
        cmds: Vec<Vec<Bytes>>,
        must_not_exist: Option<Bytes>,
        after: After,
        ok: String,
    },
    Info,
    Console(Vec<String>),
}

/// Live numbers for one saved server, read for the dashboard.
#[derive(Clone, Debug)]
pub struct ProbeStats {
    pub kind: &'static str,
    pub version: String,
    pub mode: String,
    pub used_mem: i64,
    pub max_mem: i64,
    pub keys: i64,
    pub ops: i64,
}

/// Open a short-lived connection and read a server's headline numbers.
pub fn probe(cfg: &ConnCfg) -> Result<ProbeStats, String> {
    let (mut c, meta) = connect(cfg)?;
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let s: String = redis::cmd("INFO").query(&mut c).map_err(|e| err_text(&e))?;
    let info = InfoSnapshot {
        sections: parse_info(&s),
        at: Instant::now(),
    };
    let num = |k: &str| info.get(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) as i64;
    Ok(ProbeStats {
        kind: meta.kind,
        version: meta.version,
        mode: meta.mode,
        used_mem: num("used_memory"),
        max_mem: num("maxmemory"),
        keys: meta.dbsize,
        ops: num("instantaneous_ops_per_sec"),
    })
}

pub enum Msg {
    Connected(ServerMeta, Vec<String>),
    ConnectFailed(String),
    Disconnected(String),
    Scanned {
        reset: bool,
        keys: Vec<KeyRow>,
        cursor: u64,
    },
    Key(KeyDetail),
    OpsDone {
        after: After,
        ok: String,
    },
    Failed(String),
    Info(InfoSnapshot),
    Console {
        cmd: String,
        out: String,
        err: bool,
    },
    Test(Result<String, String>),
    Probe(u64, Result<ProbeStats, String>),
}

pub struct Worker {
    tx: Sender<Job>,
    msg_tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Worker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (tx, job_rx) = channel::<Job>();
        let (msg_tx, rx) = channel::<Msg>();
        let out = msg_tx.clone();
        let c2 = ctx.clone();
        std::thread::Builder::new()
            .name("server-worker".into())
            .spawn(move || run(job_rx, out, c2))
            .expect("start worker thread");
        Self { tx, msg_tx, rx, ctx }
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    /// Read a saved server's numbers on its own thread.
    pub fn probe(&self, cfg: ConnCfg) {
        let out = self.msg_tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = probe(&cfg);
            let _ = out.send(Msg::Probe(cfg.id, r));
            ctx.request_repaint();
        });
    }

    /// Try a connection on its own thread, without touching the current one.
    pub fn test(&self, cfg: ConnCfg) {
        let out = self.msg_tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = connect(&cfg)
                .map(|(_, meta)| format!("Connected to {} {} · {} keys", meta.kind, meta.version, meta.dbsize));
            let _ = out.send(Msg::Test(r));
            ctx.request_repaint();
        });
    }
}

fn run(rx: Receiver<Job>, out: Sender<Msg>, ctx: egui::Context) {
    let mut con: Option<Connection> = None;
    while let Ok(job) = rx.recv() {
        let msg = handle(job, &mut con);
        if let Some(m) = msg
            && out.send(m).is_err()
        {
            return;
        }
        ctx.request_repaint();
    }
}

fn err_text(e: &redis::RedisError) -> String {
    let s = e.to_string();
    if e.is_connection_refusal() {
        return format!("Can't reach the server ({s}). Check the host, port and that it is running.");
    }
    if e.is_timeout() {
        return format!("The server took too long to answer ({s}).");
    }
    let low = s.to_ascii_lowercase();
    if low.contains("not valid for name") || low.contains("notvalidforname") {
        return format!(
            "The server's certificate is for a different host name ({s}). If you connect through a tunnel or port forward, turn on \"Skip the certificate host name check\" for this connection."
        );
    }
    s
}

fn handle(job: Job, con: &mut Option<Connection>) -> Option<Msg> {
    match job {
        Job::Connect(cfg) => {
            *con = None;
            match connect(&cfg) {
                Ok((mut c, meta)) => {
                    let cmds = server_commands(&mut c);
                    *con = Some(c);
                    Some(Msg::Connected(meta, cmds))
                }
                Err(e) => Some(Msg::ConnectFailed(e)),
            }
        }
        Job::Disconnect => {
            *con = None;
            None
        }
        other => {
            let Some(c) = con.as_mut() else {
                return Some(Msg::Failed("Not connected".into()));
            };
            let res = run_job(other, c);
            match res {
                Ok(m) => Some(m),
                Err(e) => {
                    if e.is_connection_dropped() || e.is_io_error() {
                        *con = None;
                        Some(Msg::Disconnected(format!("Connection lost: {}", err_text(&e))))
                    } else {
                        Some(Msg::Failed(err_text(&e)))
                    }
                }
            }
        }
    }
}

/// Connect to a server. A panic inside the connection libraries comes back as an ordinary
/// error, so the calling thread survives and the UI is told instead of waiting forever.
pub fn connect(cfg: &ConnCfg) -> Result<(Connection, ServerMeta), String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| connect_inner(cfg))) {
        Ok(result) => result,
        Err(p) => Err(format!("The connection attempt crashed: {}", panic_text(p.as_ref()))),
    }
}

/// The first line of a panic's message.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    let text = if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown error".to_string()
    };
    text.trim().lines().next().unwrap_or("unknown error").to_string()
}

fn connect_inner(cfg: &ConnCfg) -> Result<(Connection, ServerMeta), String> {
    let host = cfg.host.trim().to_string();
    if host.is_empty() {
        return Err("Enter a host".into());
    }
    let mut addr = if cfg.tls {
        ConnectionAddr::TcpTls {
            host,
            port: cfg.port,
            insecure: false,
            tls_params: None,
        }
    } else {
        ConnectionAddr::Tcp(host, cfg.port)
    };
    // Tunnels and port forwards: the host is 127.0.0.1 but the certificate names the real
    // server. The certificate is still checked against trusted authorities; only the name
    // comparison is skipped. `insecure` stays false, which would accept any certificate.
    if cfg.tls && cfg.tls_skip_host_check {
        addr.set_danger_accept_invalid_hostnames(true);
    }
    let mut rinfo = RedisConnectionInfo::default().set_db(cfg.db);
    if !cfg.username.trim().is_empty() {
        rinfo = rinfo.set_username(cfg.username.trim());
    }
    if !cfg.password.is_empty() {
        rinfo = rinfo.set_password(&cfg.password);
    }
    let info = addr
        .into_connection_info()
        .map_err(|e| err_text(&e))?
        .set_redis_settings(rinfo);
    let client = redis::Client::open(info).map_err(|e| err_text(&e))?;
    let timeout = cfg.timeout_secs.clamp(1, crate::config::TIMEOUT_MAX_SECS);
    let mut c = client
        .get_connection_with_timeout(Duration::from_secs(timeout as u64))
        .map_err(|e| err_text(&e))?;
    let _ = c.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = c.set_write_timeout(Some(Duration::from_secs(30)));

    let server: String = redis::cmd("INFO")
        .arg("server")
        .query(&mut c)
        .map_err(|e| err_text(&e))?;
    let kv = parse_info(&server);
    let get = |k: &str| {
        kv.iter()
            .flat_map(|(_, v)| v)
            .find(|(a, _)| a == k)
            .map(|(_, b)| b.clone())
    };
    let is_valkey = get("valkey_version").is_some()
        || get("server_name")
            .map(|s| s.eq_ignore_ascii_case("valkey"))
            .unwrap_or(false);
    let kind = if is_valkey { "Valkey" } else { "Redis" };
    let version = get("valkey_version")
        .or_else(|| get("redis_version"))
        .unwrap_or_default();
    let mode = get("redis_mode")
        .or_else(|| get("server_mode"))
        .unwrap_or_else(|| "standalone".into());
    let dbsize: i64 = redis::cmd("DBSIZE").query(&mut c).unwrap_or(0);
    let databases = redis::cmd("CONFIG")
        .arg("GET")
        .arg("databases")
        .query::<Vec<String>>(&mut c)
        .ok()
        .and_then(|v| v.get(1).and_then(|s| s.parse().ok()))
        .unwrap_or(16);

    let mut mismatch = None;
    let wanted = match cfg.kind {
        ServerKind::Auto => None,
        ServerKind::Redis => Some("Redis"),
        ServerKind::Valkey => Some("Valkey"),
    };
    if let Some(w) = wanted
        && w != kind
    {
        mismatch = Some(format!("Saved as {w}, but the server says it is {kind} {version}"));
    }
    if mismatch.is_none() && !cfg.version.trim().is_empty() && !version.starts_with(cfg.version.trim()) {
        mismatch = Some(format!(
            "Saved as version {}, but the server reports {version}",
            cfg.version.trim()
        ));
    }
    Ok((
        c,
        ServerMeta {
            kind,
            version,
            mode,
            dbsize,
            databases,
            mismatch,
        },
    ))
}

fn server_commands(c: &mut Connection) -> Vec<String> {
    let mut v: Vec<String> = redis::cmd("COMMAND")
        .arg("LIST")
        .query::<Vec<String>>(c)
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.to_uppercase())
        .collect();
    v.sort();
    v.dedup();
    v
}

fn run_job(job: Job, c: &mut Connection) -> redis::RedisResult<Msg> {
    Ok(match job {
        Job::Scan {
            pattern,
            kind,
            cursor,
            reset,
        } => {
            let pattern = if pattern.trim().is_empty() {
                "*".to_string()
            } else {
                pattern
            };
            let mut keys: Vec<Bytes> = Vec::new();
            let mut cur = cursor;
            let mut rounds = 0;
            loop {
                let mut cmd = redis::cmd("SCAN");
                cmd.arg(cur).arg("MATCH").arg(&pattern).arg("COUNT").arg(1000);
                if let Some(k) = &kind {
                    cmd.arg("TYPE").arg(k);
                }
                let (next, batch): (u64, Vec<Bytes>) = cmd.query(c)?;
                keys.extend(batch);
                cur = next;
                rounds += 1;
                if cur == 0 || keys.len() >= PAGE_KEYS || rounds >= 50 {
                    break;
                }
            }
            keys.sort();
            keys.dedup();
            let mut rows = Vec::with_capacity(keys.len());
            if !keys.is_empty() {
                let mut p = redis::pipe();
                for k in &keys {
                    p.cmd("TYPE").arg(k).cmd("PTTL").arg(k);
                }
                let vals: Vec<Value> = p.query(c)?;
                for (i, k) in keys.into_iter().enumerate() {
                    let kind = vals.get(i * 2).map(plain).unwrap_or_default();
                    let ttl = match vals.get(i * 2 + 1) {
                        Some(Value::Int(n)) => *n,
                        _ => -1,
                    };
                    rows.push(KeyRow {
                        name: String::from_utf8_lossy(&k).into_owned(),
                        key: k,
                        kind,
                        ttl_ms: ttl,
                    });
                }
            }
            Msg::Scanned {
                reset,
                keys: rows,
                cursor: cur,
            }
        }
        Job::Load(key) => Msg::Key(load_key(c, key)?),
        Job::Ops {
            cmds,
            must_not_exist,
            after,
            ok,
        } => {
            if let Some(k) = must_not_exist {
                let n: i64 = redis::cmd("EXISTS").arg(&k).query(c)?;
                if n > 0 {
                    return Ok(Msg::Failed(format!(
                        "A key named \"{}\" already exists",
                        String::from_utf8_lossy(&k)
                    )));
                }
            }
            for parts in cmds {
                let Some((name, args)) = parts.split_first() else {
                    continue;
                };
                let mut cmd = redis::cmd(&String::from_utf8_lossy(name));
                for a in args {
                    cmd.arg(a);
                }
                cmd.query::<Value>(c)?;
            }
            Msg::OpsDone { after, ok }
        }
        Job::Info => {
            let s: String = redis::cmd("INFO").query(c)?;
            Msg::Info(InfoSnapshot {
                sections: parse_info(&s),
                at: Instant::now(),
            })
        }
        Job::Console(args) => {
            let shown = args
                .iter()
                .map(|a| {
                    if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
                        format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
                    } else {
                        a.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let Some((name, rest)) = args.split_first() else {
                return Ok(Msg::Console {
                    cmd: shown,
                    out: String::new(),
                    err: false,
                });
            };
            let upper = name.to_uppercase();
            const BLOCKED: [&str; 7] = [
                "SUBSCRIBE",
                "PSUBSCRIBE",
                "SSUBSCRIBE",
                "MONITOR",
                "SYNC",
                "PSYNC",
                "QUIT",
            ];
            if BLOCKED.contains(&upper.as_str()) {
                return Ok(Msg::Console {
                    cmd: shown,
                    out: format!("{upper} keeps the connection busy and isn't supported in this box yet."),
                    err: true,
                });
            }
            let mut cmd = redis::cmd(name);
            for a in rest {
                cmd.arg(a);
            }
            match cmd.query::<Value>(c) {
                Ok(v) => Msg::Console {
                    cmd: shown,
                    out: format_value(&v, 0),
                    err: false,
                },
                Err(e) if e.is_connection_dropped() || e.is_io_error() => return Err(e),
                Err(e) => Msg::Console {
                    cmd: shown,
                    out: format!(
                        "(error) {}",
                        e.to_string()
                            .trim_start_matches("ResponseError: ")
                            .trim_end_matches(':')
                            .trim()
                    ),
                    err: true,
                },
            }
        }
        Job::Connect(_) | Job::Disconnect => unreachable!(),
    })
}

fn load_key(c: &mut Connection, key: Bytes) -> redis::RedisResult<KeyDetail> {
    let name = String::from_utf8_lossy(&key).into_owned();
    let kind: String = plain(&redis::cmd("TYPE").arg(&key).query::<Value>(c)?);
    let ttl_ms: i64 = redis::cmd("PTTL").arg(&key).query(c)?;
    let memory: Option<i64> = redis::cmd("MEMORY").arg("USAGE").arg(&key).query(c).ok();
    let lossy = |b: &Bytes| String::from_utf8_lossy(b).into_owned();
    let (len, value) = match kind.as_str() {
        "string" => {
            let len: i64 = redis::cmd("STRLEN").arg(&key).query(c)?;
            let v: Bytes = redis::cmd("GET").arg(&key).query(c)?;
            (len, ValueData::Str(lossy(&v)))
        }
        "list" => {
            let len: i64 = redis::cmd("LLEN").arg(&key).query(c)?;
            let v: Vec<Bytes> = redis::cmd("LRANGE")
                .arg(&key)
                .arg(0)
                .arg(ITEM_LIMIT as i64 - 1)
                .query(c)?;
            (len, ValueData::List(v.iter().map(lossy).collect()))
        }
        "set" => {
            let len: i64 = redis::cmd("SCARD").arg(&key).query(c)?;
            let mut items = Vec::new();
            let mut cur = 0u64;
            loop {
                let (next, batch): (u64, Vec<Bytes>) =
                    redis::cmd("SSCAN").arg(&key).arg(cur).arg("COUNT").arg(500).query(c)?;
                items.extend(batch.iter().map(lossy));
                cur = next;
                if cur == 0 || items.len() >= ITEM_LIMIT {
                    break;
                }
            }
            items.truncate(ITEM_LIMIT);
            items.sort();
            (len, ValueData::Set(items))
        }
        "zset" => {
            let len: i64 = redis::cmd("ZCARD").arg(&key).query(c)?;
            let v: Vec<(Bytes, f64)> = redis::cmd("ZRANGE")
                .arg(&key)
                .arg(0)
                .arg(ITEM_LIMIT as i64 - 1)
                .arg("WITHSCORES")
                .query(c)?;
            (len, ValueData::ZSet(v.iter().map(|(m, s)| (lossy(m), *s)).collect()))
        }
        "hash" => {
            let len: i64 = redis::cmd("HLEN").arg(&key).query(c)?;
            let mut items = Vec::new();
            let mut cur = 0u64;
            loop {
                let (next, batch): (u64, Vec<(Bytes, Bytes)>) =
                    redis::cmd("HSCAN").arg(&key).arg(cur).arg("COUNT").arg(500).query(c)?;
                items.extend(batch.iter().map(|(f, v)| (lossy(f), lossy(v))));
                cur = next;
                if cur == 0 || items.len() >= ITEM_LIMIT {
                    break;
                }
            }
            items.truncate(ITEM_LIMIT);
            items.sort();
            (len, ValueData::Hash(items))
        }
        "stream" => {
            let len: i64 = redis::cmd("XLEN").arg(&key).query(c)?;
            let v: Value = redis::cmd("XREVRANGE")
                .arg(&key)
                .arg("+")
                .arg("-")
                .arg("COUNT")
                .arg(50)
                .query(c)?;
            (len, ValueData::Text(format_value(&v, 0)))
        }
        "ReJSON-RL" => {
            let v: Value = redis::cmd("JSON.GET").arg(&key).query(c)?;
            let text = plain(&v);
            let pretty = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|j| serde_json::to_string_pretty(&j).ok())
                .unwrap_or(text);
            (0, ValueData::Text(pretty))
        }
        _ => (0, ValueData::Unsupported),
    };
    Ok(KeyDetail {
        key,
        name,
        kind,
        ttl_ms,
        len,
        memory,
        value,
    })
}

/// Turn a simple reply into text.
fn plain(v: &Value) -> String {
    match v {
        Value::SimpleString(s) => s.clone(),
        Value::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Okay => "OK".into(),
        Value::Int(n) => n.to_string(),
        Value::Nil => String::new(),
        other => format_value(other, 0),
    }
}

/// Show a reply the way a command line tool would.
pub fn format_value(v: &Value, indent: usize) -> String {
    let quote = |b: &[u8]| -> String {
        let s = String::from_utf8_lossy(b);
        let mut o = String::with_capacity(s.len() + 2);
        o.push('"');
        for ch in s.chars() {
            match ch {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\r' => o.push_str("\\r"),
                '\t' => o.push_str("\\t"),
                c if c.is_control() => o.push_str(&format!("\\x{:02x}", c as u32)),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    };
    let list = |items: Vec<&Value>, empty: &str| -> String {
        if items.is_empty() {
            return empty.to_string();
        }
        let width = items.len().to_string().len();
        let pad = " ".repeat(indent);
        items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let num = format!("{:>width$}) ", i + 1);
                let body = format_value(it, indent + num.len());
                if i == 0 {
                    format!("{num}{body}")
                } else {
                    format!("{pad}{num}{body}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    match v {
        Value::Nil => "(nil)".into(),
        Value::Int(n) => format!("(integer) {n}"),
        Value::BulkString(b) => quote(b),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => "OK".into(),
        Value::Double(d) => format!("(double) {d}"),
        Value::Boolean(b) => format!("({})", if *b { "true" } else { "false" }),
        Value::Array(a) => list(a.iter().collect(), "(empty array)"),
        Value::Set(a) => list(a.iter().collect(), "(empty set)"),
        Value::Map(m) => list(m.iter().flat_map(|(k, v)| [k, v]).collect(), "(empty map)"),
        Value::VerbatimString { text, .. } => text.clone(),
        other => format!("{other:?}"),
    }
}

pub fn parse_info(s: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            out.push((h.trim().to_string(), Vec::new()));
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if out.is_empty() {
                out.push(("Info".into(), Vec::new()));
            }
            out.last_mut().unwrap().1.push((k.to_string(), v.to_string()));
        }
    }
    out
}

/// Split a typed command into words, honouring "quotes" and 'quotes'.
pub fn split_args(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' | '\'' => {
                let q = ch;
                in_word = true;
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == q {
                        closed = true;
                        break;
                    }
                    if c == '\\' && q == '"' {
                        match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some('r') => cur.push('\r'),
                            Some(o) => cur.push(o),
                            None => {}
                        }
                    } else {
                        cur.push(c);
                    }
                }
                if !closed {
                    return Err("A quote isn't closed".into());
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_becomes_an_error() {
        let p = std::panic::catch_unwind(|| panic!("\nfirst line\nsecond line")).unwrap_err();
        assert_eq!(panic_text(p.as_ref()), "first line");
    }

    /// A secure connection must end in an error or a connection, never in a panic. Port 1 is
    /// closed, so this fails fast without reaching any server.
    #[test]
    fn tls_attempt_does_not_crash() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cfg = ConnCfg {
            port: 1,
            tls: true,
            tls_skip_host_check: true,
            timeout_secs: 1,
            ..Default::default()
        };
        let Err(err) = connect(&cfg) else {
            panic!("nothing should be listening on port 1");
        };
        assert!(!err.contains("crashed"), "{err}");
    }

    /// Runs only when CACHE_DECK_TEST_PORT points at a scratch server (it writes test keys).
    #[test]
    fn live_edits() {
        let Ok(port) = std::env::var("CACHE_DECK_TEST_PORT") else {
            return;
        };
        let cfg = ConnCfg {
            port: port.parse().unwrap(),
            db: 5,
            ..Default::default()
        };
        let (mut c, meta) = connect(&cfg).expect("connect");
        assert!(!meta.version.is_empty());
        let k = |s: &str| s.as_bytes().to_vec();
        let ops = |c: &mut Connection, cmds: Vec<Vec<Bytes>>, absent: Option<Bytes>| {
            run_job(
                Job::Ops {
                    cmds,
                    must_not_exist: absent,
                    after: After::ReloadKey(vec![]),
                    ok: String::new(),
                },
                c,
            )
            .unwrap()
        };
        let load = |c: &mut Connection, key: &str| load_key(c, key.as_bytes().to_vec()).unwrap();
        redis::cmd("FLUSHDB").query::<()>(&mut c).unwrap();

        // new key + duplicate refused
        ops(
            &mut c,
            vec![vec![k("RPUSH"), k("l"), k("a"), k("b"), k("c"), k("b")]],
            Some(k("l")),
        );
        assert!(matches!(
            ops(&mut c, vec![vec![k("SET"), k("l"), k("x")]], Some(k("l"))),
            Msg::Failed(_)
        ));
        // remove the 2nd "b" (index 3) only
        ops(
            &mut c,
            vec![
                vec![k("LSET"), k("l"), k("3"), k("__cache_deck_removed__")],
                vec![k("LREM"), k("l"), k("1"), k("__cache_deck_removed__")],
            ],
            None,
        );
        match load(&mut c, "l").value {
            ValueData::List(v) => assert_eq!(v, ["a", "b", "c"]),
            other => panic!("{other:?}"),
        }

        // string edit keeps expiry
        ops(
            &mut c,
            vec![vec![k("SET"), k("s"), k("v1")], vec![k("EXPIRE"), k("s"), k("100")]],
            None,
        );
        ops(&mut c, vec![vec![k("SET"), k("s"), k("v2"), k("KEEPTTL")]], None);
        let d = load(&mut c, "s");
        assert!(d.ttl_ms > 0);
        assert!(matches!(d.value, ValueData::Str(ref v) if v == "v2"));
        ops(&mut c, vec![vec![k("PERSIST"), k("s")]], None);
        assert_eq!(load(&mut c, "s").ttl_ms, -1);

        // hash, set, zset
        ops(
            &mut c,
            vec![
                vec![k("HSET"), k("h"), k("f"), k("1")],
                vec![k("HSET"), k("h"), k("g"), k("2")],
                vec![k("HDEL"), k("h"), k("f")],
            ],
            None,
        );
        match load(&mut c, "h").value {
            ValueData::Hash(v) => assert_eq!(v, [("g".to_string(), "2".to_string())]),
            o => panic!("{o:?}"),
        }
        ops(
            &mut c,
            vec![
                vec![k("SADD"), k("st"), k("x"), k("y")],
                vec![k("SREM"), k("st"), k("x")],
            ],
            None,
        );
        match load(&mut c, "st").value {
            ValueData::Set(v) => assert_eq!(v, ["y"]),
            o => panic!("{o:?}"),
        }
        ops(
            &mut c,
            vec![
                vec![k("ZADD"), k("z"), k("2.5"), k("m")],
                vec![k("ZADD"), k("z"), k("7"), k("m")],
            ],
            None,
        );
        match load(&mut c, "z").value {
            ValueData::ZSet(v) => assert_eq!(v, [("m".to_string(), 7.0)]),
            o => panic!("{o:?}"),
        }

        // rename, delete, scan with type filter
        ops(&mut c, vec![vec![k("RENAME"), k("z"), k("z2")]], Some(k("z2")));
        match run_job(
            Job::Scan {
                pattern: "*".into(),
                kind: Some("zset".into()),
                cursor: 0,
                reset: true,
            },
            &mut c,
        )
        .unwrap()
        {
            Msg::Scanned { keys, cursor, .. } => {
                assert_eq!(cursor, 0);
                assert_eq!(keys.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["z2"]);
            }
            _ => panic!(),
        }
        ops(&mut c, vec![vec![k("DEL"), k("z2")]], None);
        assert_eq!(load(&mut c, "z2").kind, "none");
        redis::cmd("FLUSHDB").query::<()>(&mut c).unwrap();
    }

    #[test]
    fn splits_quotes() {
        assert_eq!(
            split_args(r#"SET "a b" 'c d' e"#).unwrap(),
            vec!["SET", "a b", "c d", "e"]
        );
        assert_eq!(split_args(r#"SET k "x\"y""#).unwrap(), vec!["SET", "k", "x\"y"]);
        assert_eq!(split_args(r#"SET k """#).unwrap(), vec!["SET", "k", ""]);
        assert!(split_args(r#"SET "a"#).is_err());
    }

    #[test]
    fn formats_nested() {
        let v = Value::Array(vec![
            Value::BulkString(b"a".to_vec()),
            Value::Array(vec![Value::Int(1), Value::Nil]),
        ]);
        assert_eq!(format_value(&v, 0), "1) \"a\"\n2) 1) (integer) 1\n   2) (nil)");
    }

    #[test]
    fn parses_info() {
        let s = parse_info("# Server\r\nredis_version:7.2.4\r\n\r\n# Clients\r\nconnected_clients:3\r\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].1[0], ("connected_clients".to_string(), "3".to_string()));
    }
}
