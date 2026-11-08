//! Local HTTP API (OpenAPI 3.1, `api_spec.json`).
//!
//! A small blocking server (`tiny_http`) bound to `127.0.0.1` only. Every `/v1/*` route except `/v1/health` needs
//! `Authorization: Bearer <token>`; `/openapi.json` and `/docs` are open. Routes call the same `Studio` methods as the
//! Tauri commands. The request handler (`handle`) takes a plain request value, so routing and auth are tested without a
//! socket.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Response, Server};

use crate::jobs::JobRequest;
use crate::store::JobFilter;
use crate::{Error, ErrorResponse, Studio};

/// The hand-written OpenAPI document.
pub const SPEC: &str = include_str!("api_spec.json");

const MAX_BODY: u64 = 16 * 1024 * 1024;

/// Server state kept by the studio (only the app manages a background server; `tini-cli serve` runs in the
/// foreground).
#[derive(Default)]
pub struct ApiState {
    managed: bool,
    server: Option<Arc<Server>>,
    thread: Option<JoinHandle<()>>,
    port: Option<u16>,
    error: Option<String>,
}

/// What the settings screen shows (`get_api_info`).
#[derive(Debug, Clone, Serialize)]
pub struct ApiInfo {
    pub enabled: bool,
    pub port: u16,
    pub url: String,
    pub token: String,
    pub running: bool,
    pub error: Option<String>,
}

/// Response of `call_local`: the HTTP status and the JSON body (null when the body is empty or not JSON).
#[derive(Debug, Clone, Serialize)]
pub struct LocalCall {
    pub status: u16,
    pub body: Value,
}

/// Sends one request to the running local API over HTTP, exactly as an outside client would. Used by the settings
/// screen's example request, so it exercises the real server, auth and validation. Only `/v1/` paths are allowed.
pub fn call_local(
    studio: &Studio,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<LocalCall, Error> {
    if !path.starts_with("/v1/") {
        return Err(Error::Input(format!(
            "yalnız /v1/ uçları çağrılabilir: {path}"
        )));
    }
    let info = info(studio);
    if !info.running {
        return Err(Error::Config(
            info.error
                .unwrap_or_else(|| "Yerel API çalışmıyor; önce açın".into()),
        ));
    }
    let request = ureq::request(method, &format!("{}{path}", info.url))
        .set("Authorization", &format!("Bearer {}", info.token))
        .timeout(std::time::Duration::from_secs(30));
    let result = match body {
        Some(b) => request
            .set("Content-Type", "application/json")
            .send_string(&b.to_string()),
        None => request.call(),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(Error::Config(format!("Yerel API'ye ulaşılamadı: {e}"))),
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok(LocalCall {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
    })
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Binds the server to `127.0.0.1:<port>`.
pub fn bind(port: u16) -> Result<Server, Error> {
    let _spawn_guard = crate::SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Server::http(("127.0.0.1", port)).map_err(|e| {
        let in_use = e
            .downcast_ref::<std::io::Error>()
            .map(|io| io.kind() == std::io::ErrorKind::AddrInUse)
            .unwrap_or(false);
        if in_use {
            Error::Config(format!(
                "{port} numaralı port kullanımda; Ayarlar → Yerel API bölümünden başka bir port seçin"
            ))
        } else {
            Error::Config(format!("API sunucusu başlatılamadı ({port}): {e}"))
        }
    })
}

/// Serves requests until the server is unblocked (each request on its own short-lived thread).
pub fn serve(studio: &Studio, server: &Server, port: u16) {
    while let Ok(rq) = server.recv() {
        let studio = studio.clone();
        let _ = std::thread::Builder::new()
            .name("tini-api-request".into())
            .spawn(move || respond(&studio, rq, port));
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn respond(studio: &Studio, mut rq: tiny_http::Request, port: u16) {
    let method = rq.method().as_str().to_uppercase();
    let url = rq.url().to_string();
    let authorization = rq
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string());
    let mut body = Vec::new();
    let too_large = rq.body_length().map(|n| n as u64 > MAX_BODY) == Some(true);
    let response = if too_large {
        error(413, "too_large", "istek gövdesi çok büyük")
    } else {
        use std::io::Read;
        let _ = rq.as_reader().take(MAX_BODY).read_to_end(&mut body);
        handle(
            studio,
            &ApiRequest {
                method: &method,
                url: &url,
                authorization: authorization.as_deref(),
                body: &body,
                port,
            },
        )
    };
    let status = response.status;
    let _ = match response.body {
        Body::Json(v) => rq.respond(
            Response::from_data(serde_json::to_vec(&v).unwrap_or_default())
                .with_status_code(status)
                .with_header(header("Content-Type", "application/json; charset=utf-8")),
        ),
        Body::Html(h) => rq.respond(
            Response::from_data(h.into_bytes())
                .with_status_code(status)
                .with_header(header("Content-Type", "text/html; charset=utf-8")),
        ),
        Body::Empty => rq.respond(Response::empty(status)),
        Body::File(path, content_type) => match std::fs::File::open(&path) {
            Ok(f) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().replace('"', ""))
                    .unwrap_or_default();
                rq.respond(
                    Response::from_file(f)
                        .with_status_code(status)
                        .with_header(header("Content-Type", content_type))
                        .with_header(header(
                            "Content-Disposition",
                            &format!("inline; filename=\"{name}\""),
                        )),
                )
            }
            Err(_) => {
                let e = error(404, "not_found", "ses dosyası bulunamadı");
                let Body::Json(v) = e.body else { return };
                rq.respond(
                    Response::from_data(serde_json::to_vec(&v).unwrap_or_default())
                        .with_status_code(404)
                        .with_header(header("Content-Type", "application/json; charset=utf-8")),
                )
            }
        },
    };
}

// --- studio integration --------------------------------------------------------------------------------------------------

/// Called by the app at startup: from now on the server follows the settings (`api_enabled`, `api_port`).
pub(crate) fn start(studio: &Studio) {
    studio.shared.api.lock().unwrap().managed = true;
    apply(studio);
}

/// Starts, restarts or stops the background server to match the settings (only when the app manages it).
pub(crate) fn apply(studio: &Studio) {
    let settings = studio.settings();
    let mut st = studio.shared.api.lock().unwrap();
    if !st.managed {
        return;
    }
    let want = settings.api_enabled.then_some(settings.api_port);
    if want.is_some() && want == st.port && st.server.is_some() {
        return;
    }
    stop_locked(&mut st);
    let Some(port) = want else { return };
    match bind(port) {
        Ok(server) => {
            let server = Arc::new(server);
            let (srv, studio2) = (server.clone(), studio.clone());
            match std::thread::Builder::new()
                .name("tini-api".into())
                .spawn(move || serve(&studio2, &srv, port))
            {
                Ok(t) => {
                    st.server = Some(server);
                    st.thread = Some(t);
                    st.port = Some(port);
                }
                Err(e) => st.error = Some(format!("API iş parçacığı başlatılamadı: {e}")),
            }
        }
        Err(e) => st.error = Some(e.to_string()),
    }
}

fn stop_locked(st: &mut ApiState) {
    if let Some(s) = st.server.take() {
        s.unblock();
    }
    if let Some(t) = st.thread.take() {
        let _ = t.join();
    }
    // the last `Server` handle is gone now; its accept thread releases the socket asynchronously, so wait (briefly)
    // until the port is free again: a restart on the same port must not fail with "port in use"
    if let Some(port) = st.port.take() {
        for _ in 0..40 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    st.error = None;
}

/// Stops the background server (app exit).
pub(crate) fn stop(studio: &Studio) {
    stop_locked(&mut studio.shared.api.lock().unwrap());
}

pub(crate) fn info(studio: &Studio) -> ApiInfo {
    let s = studio.settings();
    let st = studio.shared.api.lock().unwrap();
    ApiInfo {
        enabled: s.api_enabled,
        port: s.api_port,
        url: url(s.api_port),
        token: s.api_token.clone(),
        running: st.server.is_some() && st.port == Some(s.api_port),
        error: st.error.clone(),
    }
}

// --- request handling ----------------------------------------------------------------------------------------------------

/// A request as the handler sees it.
pub struct ApiRequest<'a> {
    pub method: &'a str,
    /// path with optional query string
    pub url: &'a str,
    pub authorization: Option<&'a str>,
    pub body: &'a [u8],
    /// the port the server listens on (for `/docs` and the spec's `servers`)
    pub port: u16,
}

#[derive(Debug)]
pub enum Body {
    Json(Value),
    Html(String),
    File(PathBuf, &'static str),
    Empty,
}

#[derive(Debug)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Body,
}

impl ApiResponse {
    fn json(status: u16, value: Value) -> Self {
        ApiResponse {
            status,
            body: Body::Json(value),
        }
    }
}

fn error(status: u16, code: &str, message: &str) -> ApiResponse {
    ApiResponse::json(status, json!({"error": {"code": code, "message": message}}))
}

fn from_error(e: &Error) -> ApiResponse {
    let status = match e {
        Error::Input(_) | Error::Json(_) => 400,
        Error::Issues(_) => 422,
        Error::NotFound(_) => 404,
        Error::Config(_) => 503,
        Error::Cancelled => 409,
        _ => 500,
    };
    let r = ErrorResponse::from(e);
    let mut body = json!({"error": {"code": r.code, "message": r.message}});
    if !r.issues.is_empty() {
        body["error"]["issues"] = json!(r.issues);
    }
    ApiResponse::json(status, body)
}

fn reply<T: Serialize>(status: u16, r: Result<T, Error>) -> ApiResponse {
    match r.and_then(|v| Ok(serde_json::to_value(v)?)) {
        Ok(v) => ApiResponse::json(status, v),
        Err(e) => from_error(&e),
    }
}

/// Constant-time comparison (the token must not leak through response timing).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn authorized(studio: &Studio, authorization: Option<&str>) -> bool {
    let token = studio.settings().api_token;
    let given = authorization
        .and_then(|h| {
            let (scheme, value) = h.trim().split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| value.trim())
        })
        .unwrap_or("");
    !token.is_empty() && same(given.as_bytes(), token.as_bytes())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, Error> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(Error::Input("istek gövdesi boş; JSON bekleniyor".into()));
    }
    Ok(serde_json::from_slice(body)?)
}

#[derive(Deserialize)]
struct TextBody {
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    options: Option<crate::text_fix::FixOptions>,
}

/// Routes one request.
pub fn handle(studio: &Studio, rq: &ApiRequest) -> ApiResponse {
    let (path, q) = rq.url.split_once('?').unwrap_or((rq.url, ""));
    let segments: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect();
    let segs: Vec<&str> = segments.iter().map(String::as_str).collect();
    let method = rq.method;
    match (method, segs.as_slice()) {
        ("GET", ["v1", "health"]) => {
            return ApiResponse::json(
                200,
                json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}),
            )
        }
        ("GET", ["openapi.json"]) => return ApiResponse::json(200, spec_for(rq.port)),
        ("GET", ["docs"]) => {
            return ApiResponse {
                status: 200,
                body: Body::Html(docs_html(rq.port)),
            }
        }
        (_, ["v1", ..]) => {}
        _ => return not_found(method, path),
    }
    if !authorized(studio, rq.authorization) {
        return error(
            401,
            "unauthorized",
            "geçersiz ya da eksik anahtar: Authorization: Bearer <token> başlığı gerekli",
        );
    }
    match (method, &segs[1..]) {
        ("GET", ["status"]) => ApiResponse::json(200, studio.status()),
        ("GET", ["voices"]) => reply(200, studio.voices()),
        ("POST", ["jobs"]) => reply(
            201,
            parse::<JobRequest>(rq.body).and_then(|r| studio.enqueue_job(r)),
        ),
        ("GET", ["jobs"]) => {
            let mut filter = JobFilter::default();
            for (k, v) in query(q) {
                let v = Some(v).filter(|v| !v.is_empty());
                match k.as_str() {
                    "kind" => filter.kind = v,
                    "status" => filter.status = v,
                    "limit" => match v.map(|v| v.parse::<i64>()) {
                        Some(Ok(n)) if n > 0 => filter.limit = Some(n),
                        None => {}
                        _ => return error(400, "input", "limit pozitif bir tam sayı olmalı"),
                    },
                    _ => {}
                }
            }
            reply(200, studio.jobs(&filter))
        }
        ("GET", ["jobs", id]) => match studio.job(id) {
            Ok(Some(j)) => reply(200, Ok(j)),
            Ok(None) => job_not_found(id),
            Err(e) => from_error(&e),
        },
        ("POST", ["jobs", id, "cancel"]) => match studio.shared.store.job(id) {
            Ok(Some(_)) => match studio.cancel_job(id) {
                Ok(()) => ApiResponse {
                    status: 204,
                    body: Body::Empty,
                },
                Err(e) => from_error(&e),
            },
            Ok(None) => job_not_found(id),
            Err(e) => from_error(&e),
        },
        ("GET", ["jobs", id, "audio"]) => job_audio(studio, id),
        ("POST", ["text", "check"]) => match parse::<TextBody>(rq.body) {
            Ok(b) => ApiResponse::json(
                200,
                studio.check_text(&b.text, b.language.as_deref(), b.voice.as_deref()),
            ),
            Err(e) => from_error(&e),
        },
        ("POST", ["text", "fix"]) => match parse::<TextBody>(rq.body) {
            Ok(b) => ApiResponse::json(
                200,
                studio.fix_text(
                    &b.text,
                    b.language.as_deref(),
                    b.voice.as_deref(),
                    &b.options.unwrap_or_default(),
                ),
            ),
            Err(e) => from_error(&e),
        },
        _ => not_found(method, path),
    }
}

fn not_found(method: &str, path: &str) -> ApiResponse {
    error(
        404,
        "not_found",
        &format!("bilinmeyen uç nokta: {method} {path} (bkz. /docs)"),
    )
}

fn job_not_found(id: &str) -> ApiResponse {
    error(404, "not_found", &format!("üretim yok: {id}"))
}

fn job_audio(studio: &Studio, id: &str) -> ApiResponse {
    let job = match studio.shared.store.job(id) {
        Ok(Some(j)) => j,
        Ok(None) => return job_not_found(id),
        Err(e) => return from_error(&e),
    };
    if !matches!(job.kind.as_str(), "read" | "clone" | "timed") {
        return error(
            404,
            "not_found",
            "bu üretim türünün ses dosyası yok (yalnız read, clone, timed)",
        );
    }
    if job.status != "done" {
        return error(
            409,
            "conflict",
            &format!("üretim bitmedi (durum: {})", job.status),
        );
    }
    let wav = job
        .result
        .as_ref()
        .and_then(|r| r["wav"].as_str())
        .map(PathBuf::from)
        .filter(|p| p.is_file() && p.starts_with(studio.shared.paths.jobs()));
    match wav {
        Some(p) => ApiResponse {
            status: 200,
            body: Body::File(p, "audio/wav"),
        },
        None => error(404, "not_found", "üretimin ses dosyası bulunamadı"),
    }
}

// --- spec and docs -------------------------------------------------------------------------------------------------------

fn spec() -> &'static Value {
    static SPEC_VALUE: OnceLock<Value> = OnceLock::new();
    SPEC_VALUE.get_or_init(|| serde_json::from_str(SPEC).expect("api_spec.json is valid JSON"))
}

/// The spec with `servers` pointing at the real port.
fn spec_for(port: u16) -> Value {
    let mut s = spec().clone();
    s["servers"] = json!([{"url": url(port), "description": "Tını (yerel)"}]);
    s["info"]["version"] = json!(env!("CARGO_PKG_VERSION"));
    s
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn curl_example(port: u16, method: &str, path: &str, op: &Value) -> String {
    let concrete = path.replace("{id}", "JOB_ID");
    let mut parts = vec!["curl".to_string()];
    if method != "get" {
        parts.push(format!("-X {}", method.to_uppercase()));
    }
    if op.get("security").map(|s| s == &json!([])) != Some(true) {
        parts.push("-H \"Authorization: Bearer <token>\"".into());
    }
    if let Some(example) = op.pointer("/requestBody/content/application~1json/example") {
        parts.push("-H \"Content-Type: application/json\"".into());
        parts.push(format!(
            "-d '{}'",
            serde_json::to_string(example).unwrap_or_default()
        ));
    }
    if op.pointer("/responses/200/content/audio~1wav").is_some() {
        parts.push("-o okuma.wav".into());
    }
    let q = if concrete == "/v1/jobs" && method == "get" {
        "?kind=read&limit=10"
    } else {
        ""
    };
    parts.push(format!("\"{}{concrete}{q}\"", url(port)));
    parts.join(" ")
}

/// Self-contained HTML reference built from the spec (no CDN).
pub fn docs_html(port: u16) -> String {
    let s = spec();
    let mut rows = String::new();
    if let Some(paths) = s["paths"].as_object() {
        for (path, item) in paths {
            let Some(ops) = item.as_object() else {
                continue;
            };
            for (method, op) in ops {
                if !["get", "post", "put", "delete", "patch"].contains(&method.as_str()) {
                    continue;
                }
                let open = op.get("security").map(|s| s == &json!([])) == Some(true);
                rows.push_str(&format!(
                    "<section class=\"op\"><h3><span class=\"m {m}\">{M}</span> <code>{p}</code>{lock}</h3>\
                     <p class=\"summary\">{summary}</p><p>{desc}</p><pre>{curl}</pre></section>\n",
                    m = method,
                    M = method.to_uppercase(),
                    p = escape(path),
                    lock = if open {
                        " <span class=\"open\">anahtar gerekmez</span>"
                    } else {
                        ""
                    },
                    summary = escape(op["summary"].as_str().unwrap_or("")),
                    desc = escape(op["description"].as_str().unwrap_or("")),
                    curl = escape(&curl_example(port, method, path, op)),
                ));
            }
        }
    }
    let base = url(port);
    format!(
        r#"<!doctype html>
<html lang="tr"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Tını API</title>
<style>
:root {{ --bg:#fafaf9; --fg:#1c1917; --muted:#57534e; --card:#fff; --line:#e7e5e4; --code:#f5f5f4; --accent:#0f766e; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#141414; --fg:#f5f5f4; --muted:#a8a29e; --card:#1c1c1c; --line:#2e2e2e; --code:#232323; --accent:#2dd4bf; }} }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--fg); font:15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width:880px; margin:0 auto; padding:32px 16px 64px; }}
h1 {{ margin:0 0 4px; font-size:26px; }} h2 {{ margin-top:36px; font-size:18px; }}
p {{ color:var(--muted); margin:6px 0; }}
a {{ color:var(--accent); }}
code, pre {{ font:13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }}
pre {{ background:var(--code); border:1px solid var(--line); border-radius:8px; padding:10px 12px; overflow-x:auto; white-space:pre-wrap; word-break:break-all; color:var(--fg); }}
.op {{ background:var(--card); border:1px solid var(--line); border-radius:10px; padding:14px 16px; margin:12px 0; }}
.op h3 {{ margin:0 0 4px; font-size:15px; display:flex; gap:8px; align-items:center; flex-wrap:wrap; }}
.summary {{ color:var(--fg); font-weight:500; }}
.m {{ font:600 11px/1 ui-monospace, Menlo, monospace; padding:4px 7px; border-radius:5px; color:#fff; }}
.get {{ background:#2563eb; }} .post {{ background:#16a34a; }} .delete {{ background:#dc2626; }}
.open {{ font-size:12px; font-weight:400; color:var(--muted); border:1px solid var(--line); border-radius:999px; padding:1px 8px; }}
</style></head>
<body><main>
<h1>Tını API</h1>
<p>Yerel HTTP API (yalnız bu Mac'ten erişilir): <code>{base}</code> · OpenAPI 3.1 belgesi: <a href="/openapi.json">/openapi.json</a></p>
<h2>Kimlik doğrulama</h2>
<p><code>/v1/health</code>, <code>/openapi.json</code> ve <code>/docs</code> dışındaki her istek şu başlığı ister. Anahtar Ayarlar → API bölümündedir.</p>
<pre>Authorization: Bearer &lt;token&gt;</pre>
<p>Hatalar şu biçimde döner: <code>{{"error": {{"code": "...", "message": "..."}}}}</code></p>
<h2>Uç noktalar</h2>
{rows}
</main></body></html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn studio() -> (tempfile::TempDir, Studio) {
        std::env::set_var(crate::studio::NO_WORKER_ENV, "1");
        let dir = tempfile::tempdir().unwrap();
        let res = dir.path().join("res");
        std::fs::create_dir_all(&res).unwrap();
        let s = Studio::open_at(
            dir.path().join("data"),
            res,
            Default::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        (dir, s)
    }

    fn call(s: &Studio, method: &str, url: &str, auth: Option<&str>, body: &str) -> ApiResponse {
        handle(
            s,
            &ApiRequest {
                method,
                url,
                authorization: auth,
                body: body.as_bytes(),
                port: 7860,
            },
        )
    }

    fn json_of(r: &ApiResponse) -> &Value {
        match &r.body {
            Body::Json(v) => v,
            other => panic!("not json: {other:?}"),
        }
    }

    #[test]
    fn auth_and_routing() {
        // `/v1/status` spawns `sysctl`
        let _serial = crate::PROCESS_AND_SOCKET_TESTS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_d, s) = studio();
        let token = s.settings().api_token;
        assert_eq!(token.len(), 32);
        let bearer = format!("Bearer {token}");
        let auth = Some(bearer.as_str());

        let r = call(&s, "GET", "/v1/health", None, "");
        assert_eq!(r.status, 200);
        assert_eq!(json_of(&r)["status"], "ok");

        let r = call(&s, "GET", "/v1/voices", None, "");
        assert_eq!(r.status, 401);
        assert_eq!(json_of(&r)["error"]["code"], "unauthorized");
        assert_eq!(
            call(&s, "GET", "/v1/voices", Some("Bearer wrong"), "").status,
            401
        );
        let r = call(&s, "GET", "/v1/voices", auth, "");
        assert_eq!(r.status, 200);
        assert!(json_of(&r).is_array());

        assert_eq!(call(&s, "GET", "/v1/status", auth, "").status, 200);
        let r = call(&s, "GET", "/v1/jobs/nope", auth, "");
        assert_eq!(r.status, 404);
        assert_eq!(json_of(&r)["error"]["code"], "not_found");
        assert_eq!(
            call(&s, "POST", "/v1/jobs/nope/cancel", auth, "").status,
            404
        );
        assert_eq!(call(&s, "GET", "/v1/jobs/nope/audio", auth, "").status, 404);
        assert_eq!(call(&s, "GET", "/v1/unknown", auth, "").status, 404);
        assert_eq!(call(&s, "GET", "/nothing", None, "").status, 404);

        let r = call(&s, "GET", "/v1/jobs?kind=design&limit=5", auth, "");
        assert_eq!(r.status, 200);
        assert_eq!(json_of(&r).as_array().unwrap().len(), 0);
        assert_eq!(call(&s, "GET", "/v1/jobs?limit=x", auth, "").status, 400);

        // invalid body / validation errors
        let r = call(&s, "POST", "/v1/jobs", auth, "{not json");
        assert_eq!(r.status, 400);
        assert_eq!(json_of(&r)["error"]["code"], "json");
        let r = call(
            &s,
            "POST",
            "/v1/jobs",
            auth,
            r#"{"kind":"design","name":"","description":"","text":""}"#,
        );
        assert_eq!(r.status, 422);
        assert_eq!(json_of(&r)["error"]["code"], "issues");

        // a valid design request is queued (no worker in tests)
        let r = call(
            &s,
            "POST",
            "/v1/jobs",
            auth,
            r#"{"kind":"design","name":"Deniz","description":"A calm adult male voice","text":"Merhaba, bugün hava çok güzel."}"#,
        );
        assert_eq!(r.status, 201, "{:?}", r.body);
        let id = json_of(&r)["id"].as_str().unwrap().to_string();
        let r = call(&s, "GET", &format!("/v1/jobs/{id}"), auth, "");
        assert_eq!(r.status, 200);
        assert_eq!(json_of(&r)["status"], "queued");
        assert_eq!(
            call(&s, "GET", &format!("/v1/jobs/{id}/audio"), auth, "").status,
            404
        );
        let r = call(&s, "POST", &format!("/v1/jobs/{id}/cancel"), auth, "");
        assert_eq!(r.status, 204);
        assert_eq!(s.job(&id).unwrap().unwrap().status, "cancelled".to_string());

        let r = call(
            &s,
            "POST",
            "/v1/text/check",
            auth,
            r#"{"text":"Saat 5'te başlıyoruz."}"#,
        );
        assert_eq!(r.status, 200);
        assert_eq!(json_of(&r)["language"], "tr");
        let r = call(
            &s,
            "POST",
            "/v1/text/fix",
            auth,
            r#"{"text":"Saat 5'te başlıyoruz.","language":"tr"}"#,
        );
        assert_eq!(r.status, 200);
        assert!(json_of(&r)["text"].is_string());
        assert_eq!(call(&s, "POST", "/v1/text/fix", auth, "").status, 400);

        // open documents
        let r = call(&s, "GET", "/openapi.json", None, "");
        assert_eq!(r.status, 200);
        assert_eq!(json_of(&r)["servers"][0]["url"], "http://127.0.0.1:7860");
        let r = call(&s, "GET", "/docs", None, "");
        assert_eq!(r.status, 200);
        match r.body {
            Body::Html(h) => {
                assert!(h.contains("http://127.0.0.1:7860/v1/voices"));
                assert!(h.contains("Authorization: Bearer &lt;token&gt;"));
                assert!(h.contains("/openapi.json"));
                assert!(!h.contains(&token));
            }
            other => panic!("not html: {other:?}"),
        }
    }

    #[test]
    fn server_follows_settings_and_reports_busy_port() {
        let _serial = crate::PROCESS_AND_SOCKET_TESTS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_d, s) = studio();
        let free = || {
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let port = free();
        s.start_api();
        assert!(!s.api_info().running);
        let mut next = s.settings();
        next.api_enabled = true;
        next.api_port = port;
        s.save_settings(next.clone()).unwrap();
        let info = s.api_info();
        assert!(info.running, "{:?}", info.error);
        let health = ureq::get(&format!("{}/v1/health", info.url))
            .call()
            .unwrap();
        assert_eq!(health.status(), 200);
        match ureq::get(&format!("{}/v1/voices", info.url)).call() {
            Err(ureq::Error::Status(code, _)) => assert_eq!(code, 401),
            other => panic!("expected 401, got {other:?}"),
        }
        let voices = ureq::get(&format!("{}/v1/voices", info.url))
            .set("Authorization", &format!("Bearer {}", info.token))
            .call()
            .unwrap();
        assert_eq!(voices.status(), 200);

        // the settings screen's example request goes through the same server, with the token
        let call = s.call_local_api("GET", "/v1/voices", None).unwrap();
        assert_eq!(call.status, 200);
        assert!(call.body.is_array());
        let invalid = s
            .call_local_api(
                "POST",
                "/v1/jobs",
                Some(&json!({"kind": "read", "voice": "nope", "text": ""})),
            )
            .unwrap();
        // an unknown voice is a JSON error, passed through with its status
        assert_eq!(invalid.status, 404);
        assert!(invalid.body["error"]["message"].is_string());
        assert!(s.call_local_api("GET", "/openapi.json", None).is_err());

        // disabling frees the port
        next.api_enabled = false;
        s.save_settings(next.clone()).unwrap();
        assert!(!s.api_info().running);
        assert!(s.call_local_api("GET", "/v1/voices", None).is_err());

        // the port is free right away; another program taking it is reported and the studio keeps working
        let busy = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        next.api_enabled = true;
        s.save_settings(next.clone()).unwrap();
        let info = s.api_info();
        assert!(!info.running);
        assert!(info.error.unwrap().contains("kullanımda"));
        drop(busy);
        // saving again retries
        s.save_settings(next).unwrap();
        let info = s.api_info();
        assert!(info.running, "{:?}", info.error);
        s.stop_api();
        assert!(!s.api_info().running);
    }

    #[test]
    fn spec_lists_every_route() {
        let s: Value = serde_json::from_str(SPEC).unwrap();
        assert_eq!(s["openapi"].as_str().unwrap().get(..3), Some("3.1"));
        let routes = [
            ("get", "/v1/health"),
            ("get", "/v1/status"),
            ("get", "/v1/voices"),
            ("post", "/v1/jobs"),
            ("get", "/v1/jobs"),
            ("get", "/v1/jobs/{id}"),
            ("post", "/v1/jobs/{id}/cancel"),
            ("get", "/v1/jobs/{id}/audio"),
            ("post", "/v1/text/check"),
            ("post", "/v1/text/fix"),
            ("get", "/openapi.json"),
            ("get", "/docs"),
        ];
        for (m, p) in routes {
            assert!(s["paths"][p][m].is_object(), "missing {m} {p}");
        }
        assert!(s["components"]["securitySchemes"]["bearerAuth"].is_object());
        for schema in [
            "VoiceCard",
            "Job",
            "JobRequest",
            "TextCheck",
            "FixResult",
            "Error",
        ] {
            assert!(
                s["components"]["schemas"][schema].is_object(),
                "missing schema {schema}"
            );
        }
        assert_eq!(
            s["components"]["schemas"]["JobRequest"]["discriminator"]["propertyName"],
            "kind"
        );
        // every $ref resolves
        fn refs(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(o) => {
                    if let Some(r) = o.get("$ref").and_then(|r| r.as_str()) {
                        out.push(r.to_string());
                    }
                    o.values().for_each(|x| refs(x, out));
                }
                Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
                _ => {}
            }
        }
        let mut all = Vec::new();
        refs(&s, &mut all);
        for r in all {
            let pointer = r.trim_start_matches('#');
            assert!(s.pointer(pointer).is_some(), "unresolved {r}");
        }
    }

    #[test]
    fn query_and_percent_decoding() {
        assert_eq!(
            query("kind=read&status=&x=a%20b+c"),
            vec![
                ("kind".to_string(), "read".to_string()),
                ("status".to_string(), String::new()),
                ("x".to_string(), "a b c".to_string())
            ]
        );
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%C3%A7"), "ç");
    }
}
