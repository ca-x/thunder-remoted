use std::{future::Future, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    extract::{rejection::JsonRejection, rejection::QueryRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use thunder_task_rs::{types::CreateTaskRequest, types::Phase, Error, LocalNas};
use tokio::sync::Mutex;
use tower_http::{compression::CompressionLayer, services::ServeDir};
use url::Url;

type ApiResult = Result<Json<Value>, ApiError>;

struct ApiError(StatusCode, &'static str, String);

impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error": {"code": self.1, "message": self.2}})),
        )
            .into_response()
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        let status = match &error {
            Error::Http(_) => StatusCode::SERVICE_UNAVAILABLE,
            Error::Api { code, .. } if code.as_deref() == Some("404") => StatusCode::NOT_FOUND,
            Error::Api { code, .. } if code.as_deref() == Some("402") => StatusCode::CONFLICT,
            _ => StatusCode::BAD_GATEWAY,
        };
        Self(status, "nas_error", format!("NAS 请求失败：{error}"))
    }
}

#[derive(Clone)]
struct AppState {
    base: Url,
    nas: Arc<Mutex<Option<LocalNas>>>,
    creation: Arc<Mutex<()>>,
}

fn retryable(error: &Error) -> bool {
    match error {
        Error::Http(_) => true,
        Error::Api { code, .. } => matches!(code.as_deref(), Some("401" | "402")),
        _ => false,
    }
}

impl AppState {
    async fn call<T, F, Fut>(&self, operation: F) -> Result<T, ApiError>
    where
        F: Fn(LocalNas) -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            let mut session = self.nas.lock().await;
            if session.is_none() {
                *session = Some(match LocalNas::connect(self.base.clone()).await {
                    Err(error) if retryable(&error) => LocalNas::connect(self.base.clone()).await?,
                    result => result?,
                });
            }
            let client = session.as_ref().expect("session initialized").clone();
            match operation(client).await {
                Err(error) if retryable(&error) => {
                    *session = None;
                    let client = LocalNas::connect(self.base.clone()).await?;
                    *session = Some(client.clone());
                    let result = operation(client).await;
                    if result.as_ref().is_err_and(retryable) {
                        *session = None;
                    }
                    result
                }
                result => result,
            }
        })
        .await;
        match result {
            Ok(result) => result.map_err(ApiError::from),
            Err(_) => {
                if let Ok(mut session) = self.nas.try_lock() {
                    *session = None;
                }
                Err(ApiError(
                    StatusCode::GATEWAY_TIMEOUT,
                    "nas_timeout",
                    "NAS 响应超时".into(),
                ))
            }
        }
    }
}

pub async fn run(
    addr: Option<String>,
    base: Option<String>,
    web_dir: PathBuf,
) -> anyhow::Result<()> {
    let addr = addr
        .or_else(|| std::env::var("THUNDER_BIND").ok())
        .unwrap_or_else(|| "0.0.0.0:3600".into());
    let base = base
        .or_else(|| std::env::var("THUNDER_NAS_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:2345".into());
    let base = Url::parse(&base)?;
    anyhow::ensure!(
        matches!(base.scheme(), "http" | "https")
            && base.host_str().is_some()
            && base.username().is_empty()
            && base.password().is_none(),
        "NAS base must be an HTTP(S) URL without credentials"
    );
    let state = AppState {
        base,
        nas: Arc::new(Mutex::new(None)),
        creation: Arc::new(Mutex::new(())),
    };
    let api = Router::new()
        .route("/health", get(health))
        .route("/device", get(device))
        .route("/quota", get(quota))
        .route("/tasks", get(tasks).post(create).delete(delete))
        .route("/analyze", post(analyze))
        .route("/tasks/{id}", patch(operate))
        .fallback(|| async {
            ApiError(
                StatusCode::NOT_FOUND,
                "not_found",
                "API endpoint not found".into(),
            )
        })
        .method_not_allowed_fallback(|| async {
            ApiError(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed".into(),
            )
        });
    let app = Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(web_dir))
        .layer(CompressionLayer::new())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "thunder-remoted listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let connected = state
        .call(|nas| async move { nas.device_info().await })
        .await
        .is_ok();
    Json(json!({"ok":true, "nas":state.base.as_str(), "connected":connected}))
}

async fn device(State(state): State<AppState>) -> ApiResult {
    Ok(Json(
        state
            .call(|nas| async move { nas.device_info().await })
            .await?,
    ))
}

async fn quota(State(state): State<AppState>) -> ApiResult {
    let quota = state.call(|nas| async move { nas.quota().await }).await?;
    Ok(Json(
        json!({"limit":quota.limit,"usage":quota.usage,"remaining":quota.remaining()}),
    ))
}

#[derive(Deserialize)]
struct TaskQuery {
    limit: Option<u32>,
    phase: Option<String>,
}

async fn tasks(
    State(state): State<AppState>,
    query: Result<Query<TaskQuery>, QueryRejection>,
) -> ApiResult {
    let Query(query) = query.map_err(|error| ApiError::bad(error.body_text()))?;
    if query.limit.is_some_and(|limit| limit == 0 || limit > 10000) {
        return Err(ApiError::bad("limit must be between 1 and 10000"));
    }
    let tasks = state
        .call(|nas| async move { nas.tasks(None).await })
        .await?;
    let tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| {
            query
                .phase
                .as_ref()
                .map_or(true, |phase| task.phase.as_str() == phase)
        })
        .take(query.limit.unwrap_or(u32::MAX) as usize)
        .collect();
    Ok(Json(json!(tasks)))
}

#[derive(Deserialize)]
struct Link {
    url: String,
}

fn body<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|error| ApiError::bad(error.body_text()))
}

fn validate_url(value: &str) -> Result<(), ApiError> {
    if value.starts_with("ed2k://|file|") && value.ends_with("|/") {
        let fields: Vec<_> = value.split('|').collect();
        if fields.len() >= 6
            && !fields[2].is_empty()
            && fields[3].parse::<u64>().is_ok()
            && !fields[4].is_empty()
        {
            return Ok(());
        }
        return Err(ApiError::bad("无效的 ed2k 文件链接"));
    }
    let url = Url::parse(value).map_err(|_| ApiError::bad("请输入有效的下载链接"))?;
    if !matches!(
        url.scheme(),
        "magnet" | "http" | "https" | "ftp" | "ed2k" | "thunder"
    ) {
        return Err(ApiError::bad("不支持的下载链接类型"));
    }
    Ok(())
}

async fn analyze(
    State(state): State<AppState>,
    payload: Result<Json<Link>, JsonRejection>,
) -> ApiResult {
    let link = body(payload)?;
    validate_url(&link.url)?;
    Ok(Json(
        state
            .call(|nas| {
                let url = link.url.clone();
                async move { nas.analyze(&url).await }
            })
            .await?,
    ))
}

#[derive(Deserialize)]
struct Create {
    url: String,
    name: Option<String>,
    parent_folder_id: Option<String>,
    sub_file_index: Option<Vec<u32>>,
}

fn resource_id(value: &Value) -> Option<&str> {
    value
        .get("file_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            ["resources", "list", "resource"].iter().find_map(|key| {
                let child = value.get(key)?;
                resource_id(if child.is_array() {
                    child.get(0)?
                } else {
                    child
                })
            })
        })
}

async fn create(
    State(state): State<AppState>,
    payload: Result<Json<Create>, JsonRejection>,
) -> ApiResult {
    let input = body(payload)?;
    validate_url(&input.url)?;
    if input.sub_file_index.as_ref().is_some_and(Vec::is_empty) {
        return Err(ApiError::bad("请至少选择一个文件"));
    }
    let _guard = state.creation.lock().await;
    let quota = state.call(|nas| async move { nas.quota().await }).await?;
    if quota.remaining() == Some(0) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "quota_exhausted",
            format!(
                "今日免费额度已用完（{}/{}），已使用 {} 次",
                quota.usage, quota.limit, quota.usage
            ),
        ));
    }
    let mut request = if let Some(indexes) = input.sub_file_index {
        let resource = state
            .call(|nas| {
                let url = input.url.clone();
                async move { nas.analyze(&url).await }
            })
            .await?;
        let id = resource_id(&resource)
            .ok_or_else(|| ApiError::bad("NAS 未返回资源 file_id，无法选择子文件；未创建任务"))?;
        CreateTaskRequest::from_file(id)
            .with_total_file_count(indexes.len() as u64)
            .with_sub_file_index(indexes)
    } else {
        CreateTaskRequest::new(input.url)
    };
    request = request.with_space("");
    if let Some(name) = input.name {
        request = request.with_name(name);
    }
    if let Some(folder) = input.parent_folder_id {
        request = request.with_parent_folder_id(folder);
    }
    Ok(Json(
        state
            .call(|nas| {
                let request = request.clone();
                async move { nas.create_task(request).await }
            })
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    Pause,
    Resume,
}
#[derive(Deserialize)]
struct Operation {
    action: Action,
}

async fn operate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Result<Json<Operation>, JsonRejection>,
) -> ApiResult {
    let operation = body(payload)?;
    validate_id(&id)?;
    let tasks = state
        .call(|nas| async move { nas.tasks(None).await })
        .await?;
    let task = tasks
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "task_not_found",
                "找不到该任务".into(),
            )
        })?;
    let phase = match operation.action {
        Action::Pause => Phase::Paused,
        Action::Resume => Phase::Running,
    };
    let space = task.space.ok_or_else(|| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "missing_space",
            "NAS 未返回任务 space".into(),
        )
    })?;
    state
        .call(|nas| {
            let (id, space, phase) = (id.clone(), space.clone(), phase.clone());
            async move { nas.operate_task(&id, &space, phase).await }
        })
        .await?;
    Ok(Json(json!({"ok":true})))
}

fn validate_id(id: &str) -> Result<(), ApiError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ApiError::bad("Invalid task ID"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct Delete {
    ids: Vec<String>,
}

async fn delete(
    State(state): State<AppState>,
    payload: Result<Json<Delete>, JsonRejection>,
) -> ApiResult {
    let input = body(payload)?;
    if input.ids.is_empty() {
        return Err(ApiError::bad("请选择要删除的任务"));
    }
    for id in &input.ids {
        validate_id(id)?;
    }
    state
        .call(|nas| {
            let ids = input.ids.clone();
            async move { nas.delete_tasks(&ids).await }
        })
        .await?;
    Ok(Json(json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_links_and_ids() {
        assert!(validate_url("magnet:?xt=urn:btih:abc").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("ed2k://|file|fixture|1|abc|/").is_ok());
        assert!(validate_url("ed2k://|file||bad|abc|/").is_err());
        assert!(validate_id("task-01_abc").is_ok());
        assert!(validate_id("../task").is_err());
    }

    #[test]
    fn resolves_analyzed_resource_id() {
        assert_eq!(
            resource_id(&json!({"resources":[{"file_id":"torrent-1"}]})),
            Some("torrent-1")
        );
        assert_eq!(resource_id(&json!({"resources":[]})), None);
    }

    #[test]
    fn only_retries_auth_and_transport_failures() {
        for code in ["401", "402"] {
            assert!(retryable(&Error::Api {
                code: Some(code.into()),
                message: String::new()
            }));
        }
        assert!(!retryable(&Error::Api {
            code: Some("500".into()),
            message: String::new()
        }));
    }
}
