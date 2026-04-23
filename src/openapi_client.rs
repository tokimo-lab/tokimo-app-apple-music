//! 调用主 server `/openapi/*` 的回环 HTTP 客户端。
//!
//! ## 鉴权模型（由 server 端约定）
//!
//! 所有 `/openapi/*` 路由复用 `/api/*` 的 cookie `SESSION_ID` 鉴权。
//! 子进程 app 处理浏览器请求时，必须把入站请求的 `Cookie` header 原样
//! 转发给 server，server 才能识别出 user。
//!
//! ## 调用方式
//!
//! ```ignore
//! ctx.openapi
//!     .get(&cookies, "/openapi/user/preferences/component/apple-music-auth")
//!     .send_json::<Value>().await?;
//! ```

use std::time::Duration;

use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use tracing::debug;

use crate::error::AppError;

/// 主 server 的 base URL，从 env `TOKIMO_SERVER_URL` 读取。
/// 由 server `bus/app_loader.rs` 在 spawn 子进程时注入。
pub struct OpenApiClient {
    base_url: String,
    client: Client,
}

impl OpenApiClient {
    pub fn from_env() -> anyhow::Result<Self> {
        let base_url = std::env::var("TOKIMO_SERVER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:5678".to_string())
            .trim_end_matches('/')
            .to_string();

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("tokimo-app-apple-music/0.1.0")
            .build()
            .map_err(|e| anyhow::anyhow!("reqwest build: {e}"))?;

        debug!(base_url = %base_url, "openapi: client ready");
        Ok(Self { base_url, client })
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{path}", self.base_url)
        } else {
            format!("{}/{path}", self.base_url)
        }
    }

    /// 入口：构造一个带 cookie 的 builder。
    pub fn request(&self, method: Method, cookie_header: &str, path: &str) -> RequestBuilder {
        let mut b = self.client.request(method, self.url(path));
        if !cookie_header.is_empty() {
            b = b.header(reqwest::header::COOKIE, cookie_header);
        }
        b
    }
}

/// 把 builder send 完后转成 `Result<T, AppError>`，非 2xx 包装成 `Upstream`。
pub async fn send_json<T: DeserializeOwned>(req: RequestBuilder) -> Result<T, AppError> {
    let resp = req.send().await?;
    let status = resp.status();
    if status.is_success() {
        Ok(resp.json::<T>().await?)
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Upstream { status, body })
    }
}

/// 与 `send_json` 相同，但允许 404 返回 `Ok(None)`（用于"读 user_preference 时
/// 该 scope/key 不存在"的场景）。
pub async fn send_json_optional<T: DeserializeOwned>(req: RequestBuilder) -> Result<Option<T>, AppError> {
    let resp = req.send().await?;
    let status = resp.status();
    if status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if status.is_success() {
        Ok(Some(resp.json::<T>().await?))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Upstream { status, body })
    }
}

// ── 高层封装：user_preferences ─────────────────────────────────────────────────
//
// server `GET /openapi/user/preferences/{scope}/{scope_id}` 的响应包是
// `{ success: true, data: <value-json> }`。当条目不存在时 server 返回 `data: {}`
// （空对象，**不是 404**），这里把空对象同样视作"无值"以便上层用 `Option` 语义。

#[derive(serde::Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<T>,
    #[serde(default)]
    error: Option<String>,
}

impl OpenApiClient {
    /// GET `/openapi/user/preferences/{scope}/{scope_id}` → JSON value
    /// 返回 `Ok(None)` 当：404 / envelope `success=false` / `data` 是空对象。
    pub async fn pref_get(
        &self,
        cookie_header: &str,
        scope: &str,
        scope_id: &str,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let path = format!("/openapi/user/preferences/{scope}/{scope_id}");
        let req = self.request(Method::GET, cookie_header, &path);
        let env: Option<Envelope<serde_json::Value>> = send_json_optional(req).await?;
        let Some(env) = env else { return Ok(None) };
        if !env.success {
            debug!(error = ?env.error, scope, scope_id, "openapi: pref envelope success=false");
            return Ok(None);
        }
        Ok(env.data.filter(|v| !is_empty_object(v)))
    }

    /// PUT `/openapi/user/preferences/{scope}/{scope_id}` body `{ "value": ... }`
    pub async fn pref_put(
        &self,
        cookie_header: &str,
        scope: &str,
        scope_id: &str,
        value: impl Serialize,
    ) -> Result<(), AppError> {
        let path = format!("/openapi/user/preferences/{scope}/{scope_id}");
        let body = serde_json::json!({ "value": value });
        let req = self.request(Method::PUT, cookie_header, &path).json(&body);
        let _: serde_json::Value = send_json(req).await?;
        Ok(())
    }

    /// DELETE `/openapi/user/preferences/{scope}/{scope_id}`
    pub async fn pref_delete(&self, cookie_header: &str, scope: &str, scope_id: &str) -> Result<(), AppError> {
        let path = format!("/openapi/user/preferences/{scope}/{scope_id}");
        let req = self.request(Method::DELETE, cookie_header, &path);
        let _: serde_json::Value = send_json(req).await?;
        Ok(())
    }
}

fn is_empty_object(v: &serde_json::Value) -> bool {
    v.as_object().is_some_and(serde_json::Map::is_empty)
}
