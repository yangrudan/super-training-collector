//! 训练平台任务信息查询客户端
//!
//! 通过两步 API 调用获取任务创建者和资源信息，用于丰富 HANG 告警内容：
//! 1. POST /api/open/credentials  → accessToken
//! 2. GET  /api/open/dros-ic-platform/open-job/getJobDetail → 任务详情

use crate::state::JobInfo;
use std::time::Duration;

/// 获取任务详情
///
/// 若任意步骤失败则返回 `None`，调用方应降级处理。
pub async fn fetch_job_info(
    api_url: &str,
    app_key: &str,
    app_secret: &str,
    user_id: &str,
    job_id: &str,
) -> Option<JobInfo> {
    if api_url.is_empty() || app_key.is_empty() || app_secret.is_empty() || job_id.is_empty() {
        tracing::debug!("[job_info] 平台 API 未配置或 job_id 为空，跳过查询");
        return None;
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[job_info] 创建 HTTP client 失败: {}", e);
            return None;
        }
    };

    let token = get_access_token(&client, api_url, app_key, app_secret).await?;
    get_job_detail(&client, api_url, &token, user_id, job_id).await
}

async fn get_access_token(
    client: &reqwest::Client,
    api_url: &str,
    app_key: &str,
    app_secret: &str,
) -> Option<String> {
    let url = format!("{}/api/open/credentials", api_url);
    let body = serde_json::json!({ "appKey": app_key, "appSecret": app_secret });

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[job_info] 获取 accessToken 失败: {}", e);
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[job_info] 解析 accessToken 响应失败: {}", e);
            return None;
        }
    };

    let token = json
        .get("data")
        .and_then(|d| d.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    if token.is_none() {
        tracing::warn!("[job_info] 响应中未找到 accessToken: {}", json);
    }
    token
}

async fn get_job_detail(
    client: &reqwest::Client,
    api_url: &str,
    token: &str,
    user_id: &str,
    job_id: &str,
) -> Option<JobInfo> {
    let url = format!(
        "{}/api/open/dros-ic-platform/open-job/getJobDetail?userId={}&bizType=DROS&jobId={}&jobType=",
        api_url, user_id, job_id
    );

    let resp = match client.get(&url).header("accessToken", token).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[job_info] 获取任务详情失败: {}", e);
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[job_info] 解析任务详情响应失败: {}", e);
            return None;
        }
    };

    let data = json.get("data")?;

    let str_field = |key: &str| {
        data.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    let resource = data.get("resource");
    Some(JobInfo {
        creator: str_field("creator"),
        creator_name: str_field("creatorName"),
        name: str_field("name"),
        gpu_type: resource
            .and_then(|r| r.get("gpuTypeName"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        gpu_num: resource
            .and_then(|r| r.get("gpuNum"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        worker_num: resource
            .and_then(|r| r.get("workerNum"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}
