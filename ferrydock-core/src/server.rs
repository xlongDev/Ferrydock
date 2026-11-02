use crate::error::Error;
use crate::router::Engine;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::TryStreamExt;
use tower_http::cors::{Any, CorsLayer};
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::sync::Arc;

type EngineState = Arc<Engine>;

pub async fn serve(engine: Arc<Engine>) -> crate::error::Result<()> {
    let app = build_router(engine.clone());
    let addr = engine.addr().await;
    tracing::info!("Ferrydock listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn build_router(engine: Arc<Engine>) -> Router {
    // Permissive CORS: the gateway is local-only (127.0.0.1) and the desktop
    // UI is served from a different origin (tauri://localhost / localhost dev).
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/chat/completions", post(chat_handler))
        .route("/admin/status", get(status_handler))
        .route("/admin/import", post(import_handler))
        .route("/admin/config", get(get_config_handler).post(set_config_handler))
        .layer(cors)
        .with_state(engine)
}

async fn health_handler(State(engine): State<EngineState>) -> Json<Value> {
    Json(engine.health().await)
}

async fn models_handler(State(engine): State<EngineState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &engine) {
        return unauthorized();
    }
    let inner = engine.inner.read().await;
    let data: Vec<Value> = inner
        .catalog
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "object": "model",
                "created": 0,
                "owned_by": "ferrydock",
                "name": e.name,
                "supports_images": e.supports_images,
                "supports_reasoning": e.supports_reasoning,
            })
        })
        .collect();
    drop(inner);
    Json(json!({ "object": "list", "data": data })).into_response()
}

async fn chat_handler(
    State(engine): State<EngineState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&headers, &engine) {
        return unauthorized();
    }
    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    match engine.chat(body).await {
        Ok(resp) => {
            let status = resp.status();
            if stream {
                let ct = resp.headers().get(CONTENT_TYPE).cloned();
                let s = resp
                    .bytes_stream()
                    .map_err(|e| std::io::Error::new(ErrorKind::Other, e.to_string()));
                let body = Body::from_stream(s);
                let mut r = Response::new(body);
                *r.status_mut() = StatusCode::OK;
                if let Some(ct) = ct {
                    r.headers_mut().insert(CONTENT_TYPE, ct);
                }
                r
            } else {
                match resp.bytes().await {
                    Ok(b) => match serde_json::from_slice::<Value>(&b) {
                        Ok(v) => {
                            let mut r = Json(v).into_response();
                            *r.status_mut() = status;
                            r
                        }
                        Err(_) => {
                            let mut r = Response::new(Body::from(b));
                            *r.status_mut() = status;
                            r
                        }
                    },
                    Err(e) => error_resp(StatusCode::BAD_GATEWAY, &format!("read upstream: {e}")),
                }
            }
        }
        Err(e) => map_error(e),
    }
}

async fn status_handler(State(engine): State<EngineState>) -> Json<Value> {
    Json(engine.status().await)
}

async fn import_handler(State(engine): State<EngineState>) -> Response {
    match engine.sync_now().await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => error_resp(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_config_handler(State(engine): State<EngineState>) -> Json<Value> {
    let cur = engine.redacted_config().await;
    Json(cur)
}

async fn set_config_handler(
    State(engine): State<EngineState>,
    Json(patch): Json<Value>,
) -> Response {
    match engine.update_config(patch).await {
        Ok(()) => Json(json!({ "ok": true, "reloaded": true })).into_response(),
        Err(e) => error_resp(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn authorized(headers: &HeaderMap, engine: &Engine) -> bool {
    let expected = &engine.api_key;
    if expected.is_empty() {
        return true;
    }
    match headers.get(axum::http::header::AUTHORIZATION) {
        Some(v) => v
            .to_str()
            .map(|s| s == format!("Bearer {expected}"))
            .unwrap_or(false),
        None => false,
    }
}

fn unauthorized() -> Response {
    let mut r = Json(json!({
        "error": { "message": "unauthorized", "type": "auth" }
    }))
    .into_response();
    *r.status_mut() = StatusCode::UNAUTHORIZED;
    r.headers_mut()
        .insert("www-authenticate", HeaderValue::from_static("Bearer"));
    r
}

fn error_resp(status: StatusCode, msg: &str) -> Response {
    let mut r = Json(json!({
        "error": { "message": msg, "type": "ferrydock_error" }
    }))
    .into_response();
    *r.status_mut() = status;
    r
}

fn map_error(e: Error) -> Response {
    let (status, msg) = match e {
        Error::NoModel(m) => (StatusCode::NOT_FOUND, m),
        Error::CircuitOpen(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
        Error::AllFailed(m) => (StatusCode::BAD_GATEWAY, m),
        Error::Upstream { status, body } => {
            let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            (code, body)
        }
        Error::Timeout => (StatusCode::GATEWAY_TIMEOUT, "upstream timeout".into()),
        other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    };
    let mut r = Json(json!({
        "error": { "message": msg, "type": "ferrydock_error" }
    }))
    .into_response();
    *r.status_mut() = status;
    r
}
