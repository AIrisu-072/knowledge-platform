//! A runtime-generated synthetic catalog provider on `127.0.0.1:0`.
//!
//! It speaks the registered protocol for any number of tenant Sources, each
//! under its own base path. Documents, snapshot tokens, ACLs and bodies are
//! created by the test at runtime and live only in this process' memory.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct Provenance {
    pub evidence_ref: String,
    pub direct: bool,
    pub lineage: String,
    pub predicate: String,
}

#[derive(Debug, Clone)]
pub struct Doc {
    pub id: String,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub title: String,
    pub fields: Vec<(String, String, Option<String>)>,
    pub provenance: Vec<Provenance>,
    pub readers: BTreeSet<String>,
    pub body: String,
}

impl Doc {
    /// A readable document with one verified `catalog.title` field.
    pub fn new(id: &str, title: &str, readers: &[&str]) -> Self {
        let evidence = format!("ev-{id}");
        Self {
            id: id.into(),
            version: Some("v1".into()),
            digest: Some(format!("d-{id}-1")),
            title: title.into(),
            fields: vec![("catalog.title".into(), title.into(), Some(evidence.clone()))],
            provenance: vec![Provenance {
                evidence_ref: evidence,
                direct: true,
                lineage: "catalog".into(),
                predicate: "catalog.title".into(),
            }],
            readers: readers.iter().map(|reader| (*reader).to_owned()).collect(),
            body: format!("transient body of {id}"),
        }
    }
}

/// A scripted failure for one protocol operation.
#[derive(Debug, Clone)]
pub enum Fault {
    Status(u16),
    Body(String),
    Delay(Duration),
    /// Close the connection without any response.
    Disconnect,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub tenant: String,
    pub source: String,
    pub snapshot: String,
    pub extent: String,
    pub acl_revision: u64,
    pub permission: String,
    pub page_size: usize,
    pub docs: Vec<Doc>,
    /// Inventory IDs the snapshot claims to know (including removed ones).
    pub known: Vec<String>,
    pub extra: Value,
    /// Revokes `(document, principal)` when that document's content is read.
    pub revoke_on_content: Option<(String, String)>,
}

impl Collection {
    pub fn new(tenant: &str, source: &str, docs: Vec<Doc>) -> Self {
        Self {
            tenant: tenant.into(),
            source: source.into(),
            snapshot: format!("snapshot-{source}-1"),
            extent: "partial".into(),
            acl_revision: 1,
            permission: "full_content".into(),
            page_size: 100,
            docs,
            known: vec![],
            extra: Value::Null,
            revoke_on_content: None,
        }
    }
}

#[derive(Default)]
struct State {
    collections: BTreeMap<String, Collection>,
    faults: BTreeMap<(String, String), Fault>,
    log: Vec<String>,
    bodies_served: usize,
}

#[derive(Clone)]
pub struct Catalog {
    pub addr: SocketAddr,
    state: Arc<Mutex<State>>,
}

impl Catalog {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let state = shared.clone();
                tokio::spawn(async move {
                    let Some((method, target, body)) = read_request(&mut stream).await else {
                        return;
                    };
                    let (status, payload, delay) = respond(&state, &method, &target, &body);
                    if status == 0 {
                        return;
                    }
                    if let Some(delay) = delay {
                        tokio::time::sleep(delay).await;
                    }
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let mut out = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    )
                    .into_bytes();
                    out.extend_from_slice(payload.as_bytes());
                    let _ = stream.write_all(&out).await;
                });
            }
        });
        Self { addr, state }
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Registers a tenant Source under `base_path` (e.g. `/t1/v1`).
    pub fn serve(&self, base_path: &str, collection: Collection) {
        self.state
            .lock()
            .unwrap()
            .collections
            .insert(base_path.into(), collection);
    }

    pub fn update(&self, base_path: &str, change: impl FnOnce(&mut Collection)) {
        change(
            self.state
                .lock()
                .unwrap()
                .collections
                .get_mut(base_path)
                .unwrap(),
        );
    }

    pub fn fault(&self, base_path: &str, operation: &str, fault: Fault) {
        self.state
            .lock()
            .unwrap()
            .faults
            .insert((base_path.into(), operation.into()), fault);
    }

    pub fn clear_faults(&self) {
        self.state.lock().unwrap().faults.clear();
    }

    /// `METHOD /path` of every request received, in order.
    pub fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    pub fn requests(&self) -> usize {
        self.state.lock().unwrap().log.len()
    }

    pub fn reset_log(&self) {
        self.state.lock().unwrap().log.clear();
    }

    pub fn bodies_served(&self) -> usize {
        self.state.lock().unwrap().bodies_served
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 32 * 1024 || stream.read_exact(&mut byte).await.is_err() {
            return None;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let mut first = text.lines().next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let length = text
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length.min(64 * 1024)];
    stream.read_exact(&mut body).await.ok()?;
    Some((method, target, body))
}

fn decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&segment[index + 1..index + 3], 16)
        {
            out.push(value);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hit(doc: &Doc, extra: &Value) -> Value {
    let mut value = json!({
        "id": doc.id,
        "version": doc.version,
        "digest": doc.digest,
        "kind": "knowledge",
        "title": doc.title,
        "fields": doc.fields.iter().map(|(name, value, provenance)| json!({
            "name": name, "value": value, "provenance": provenance
        })).collect::<Vec<_>>(),
    });
    if let (Value::Object(target), Value::Object(extra)) = (&mut value, extra) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    value
}

fn list(collection: &Collection, extent: &str, hits: Vec<Value>, page: Option<Value>) -> Value {
    let mut value = json!({
        "tenant": collection.tenant,
        "source": collection.source,
        "snapshot": {"token": collection.snapshot, "extent": extent, "known": collection.known},
        "status": "ok",
        "hits": hits,
    });
    if let Some(page) = page {
        value["page"] = page;
    }
    if let (Value::Object(target), Value::Object(extra)) = (&mut value, &collection.extra) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    value
}

fn matches(doc: &Doc, query: &str) -> bool {
    doc.title.contains(query) || doc.fields.iter().any(|(_, value, _)| value.contains(query))
}

fn respond(
    state: &Arc<Mutex<State>>,
    method: &str,
    target: &str,
    body: &[u8],
) -> (u16, String, Option<Duration>) {
    let mut state = state.lock().unwrap();
    state.log.push(format!("{method} {target}"));
    let Some((base, collection)) = state
        .collections
        .iter()
        .find(|(base, _)| target.starts_with(&format!("{base}/")))
        .map(|(base, collection)| (base.clone(), collection.clone()))
    else {
        return (404, "{}".into(), None);
    };
    let rest = &target[base.len() + 1..];
    let operation = rest.split(['/', '?']).next().unwrap_or_default().to_owned();
    let mut delay = None;
    match state
        .faults
        .get(&(base.clone(), operation.clone()))
        .cloned()
    {
        Some(Fault::Status(status)) => return (status, "{}".into(), None),
        Some(Fault::Body(body)) => return (200, body, None),
        Some(Fault::Delay(wait)) => delay = Some(wait),
        Some(Fault::Disconnect) => return (0, String::new(), None),
        None => {}
    }
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let docs = &collection.docs;
    let extra = &collection.extra;
    let reply = match (method, operation.as_str()) {
        ("GET", "catalog") => {
            let cursor = rest.split("cursor=").nth(1).map(decode).unwrap_or_default();
            let start = cursor
                .strip_prefix('p')
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            let end = (start + collection.page_size).min(docs.len());
            let terminal = end >= docs.len();
            let page = json!({
                "cursor": (!cursor.is_empty()).then_some(cursor),
                "next": (!terminal).then(|| format!("p{end}")),
                "terminal": terminal,
            });
            let hits = docs[start..end].iter().map(|doc| hit(doc, extra)).collect();
            list(&collection, &collection.extent, hits, Some(page))
        }
        ("POST", "search") => {
            let query = request["query"].as_str().unwrap_or_default();
            let limit = request["limit"].as_u64().unwrap_or(10) as usize;
            let hits = docs
                .iter()
                .filter(|doc| matches(doc, query))
                .take(limit)
                .map(|doc| hit(doc, extra))
                .collect();
            list(&collection, &collection.extent, hits, None)
        }
        ("POST", "lookup") => {
            let id = request["id"].as_str().unwrap_or_default();
            let hits = docs
                .iter()
                .filter(|doc| doc.id == id)
                .map(|doc| hit(doc, extra))
                .collect();
            list(&collection, &collection.extent, hits, None)
        }
        ("POST", "live") => {
            let hits = match (request["id"].as_str(), request["query"].as_str()) {
                (Some(id), _) => docs
                    .iter()
                    .filter(|doc| doc.id == id)
                    .map(|doc| hit(doc, extra))
                    .collect(),
                (None, Some(query)) => docs
                    .iter()
                    .filter(|doc| matches(doc, query))
                    .map(|doc| hit(doc, extra))
                    .collect(),
                _ => vec![],
            };
            list(&collection, &collection.extent, hits, None)
        }
        ("POST", "authorize") => {
            let principal = request["principal"].as_str().unwrap_or_default();
            let allowed = match request["id"].as_str() {
                Some(id) => docs
                    .iter()
                    .any(|doc| doc.id == id && doc.readers.contains(principal)),
                None => docs.iter().any(|doc| doc.readers.contains(principal)),
            };
            json!({
                "tenant": collection.tenant,
                "source": collection.source,
                "principal": principal,
                "id": request["id"],
                "decision": if allowed { "allowed" } else { "denied" },
                "acl_revision": collection.acl_revision,
                "permission": collection.permission,
            })
        }
        ("GET", "content") => {
            let id = decode(rest.trim_start_matches("content/"));
            let Some(doc) = docs.iter().find(|doc| doc.id == id) else {
                return (404, "{}".into(), delay);
            };
            state.bodies_served += 1;
            if let Some((target, principal)) = &collection.revoke_on_content
                && *target == id
                && let Some(stored) = state.collections.get_mut(&base)
            {
                for doc in stored.docs.iter_mut().filter(|doc| doc.id == id) {
                    doc.readers.remove(principal);
                }
            }
            json!({
                "tenant": collection.tenant,
                "source": collection.source,
                "id": doc.id,
                "version": doc.version,
                "digest": doc.digest,
                "provenance": doc.provenance.iter().map(|record| json!({
                    "ref": record.evidence_ref,
                    "direct": record.direct,
                    "lineage": record.lineage,
                    "predicate": record.predicate,
                })).collect::<Vec<_>>(),
                "body": doc.body,
            })
        }
        _ => return (404, "{}".into(), delay),
    };
    (200, reply.to_string(), delay)
}
