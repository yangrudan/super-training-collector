//! 训练平台任务信息查询客户端
//!
//! 通过两步 API 调用获取任务创建者和资源信息，用于丰富 HANG 告警内容：
//! 1. POST /api/open/credentials  → accessToken
//! 2. GET  /api/open/dros-ic-platform/open-job/getJobDetail → 任务详情

use std::time::Duration;
use tracing;

/// 从平台 API 获取到的任务关键信息
#[derive(Debug, Clone)]
pub struct JobInfo {
    /// 创建者工号
    pub creator: String,
    /// 创建者姓名
    pub creator_name: String,
    /// 任务名称（name 字段）
    pub name: String,
    /// GPU 型号
    pub gpu_type: String,
    /// 每节点 GPU 数量
    pub gpu_num: u64,
    /// Worker（节点）数量
    pub worker_num: u64,
}

/// 获取任务详情
///
/// 若任意步骤失败则返回 `None`，调用方应降级为不含平台信息的基础告警。
pub async fn fetch_job_info(
    api_url: &str,
    app_key: &str,
    app_secret: &str,
    user_id: &str,
    job_id: &str,
) -> Option<JobInfo> {
    if api_url.is_empty() || app_key.is_empty() || app_secret.is_empty() {
        tracing::debug!("job_info_client: platform API not configured, skipping");
        return None;
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("job_info_client: 创建 HTTP client 失败: {}", e);
            return None;
        }
    };

    // Step 1: 获取 accessToken
    let token = get_access_token(&client, api_url, app_key, app_secret).await?;

    // Step 2: 获取任务详情
    get_job_detail(&client, api_url, &token, user_id, job_id).await
}

/// POST /api/open/credentials 获取 accessToken
async fn get_access_token(
    client: &reqwest::Client,
    api_url: &str,
    app_key: &str,
    app_secret: &str,
) -> Option<String> {
    let url = format!("{}/api/open/credentials", api_url);
    let body = serde_json::json!({
        "appKey": app_key,
        "appSecret": app_secret,
    });

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("job_info_client: 获取 accessToken 失败: {}", e);
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("job_info_client: 解析 accessToken 响应失败: {}", e);
            return None;
        }
    };

    let token = json
        .get("data")
        .and_then(|d| d.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    if token.is_none() {
        tracing::warn!("job_info_client: 响应中未找到 accessToken: {}", json);
    }
    token
}

/// GET /api/open/dros-ic-platform/open-job/getJobDetail 获取任务详情
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

    let resp = match client
        .get(&url)
        .header("accessToken", token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("job_info_client: 获取任务详情失败: {}", e);
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("job_info_client: 解析任务详情响应失败: {}", e);
            return None;
        }
    };

    let data = json.get("data")?;

    let creator = data
        .get("creator")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let creator_name = data
        .get("creatorName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let resource = data.get("resource");
    let gpu_type = resource
        .and_then(|r| r.get("gpuTypeName"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let gpu_num = resource
        .and_then(|r| r.get("gpuNum"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let worker_num = resource
        .and_then(|r| r.get("workerNum"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Some(JobInfo {
        creator,
        creator_name,
        name,
        gpu_type,
        gpu_num,
        worker_num,
    })
}
