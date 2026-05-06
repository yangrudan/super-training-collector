//! 钉钉告警通知模块
//!
//! 当检测到 HANG 时，通过钉钉 Webhook 发送告警消息

use std::env;
use tracing;

use super::job_info_client::fetch_job_info;
use crate::flamegraph::{get_config_path, load_collector_config};
use crate::hang_types::HangDetails;

const DINGTALK_WEBHOOK: &str = "https://oapi.dingtalk.com/robot/send?access_token=f573c7f5bcd6085ccce705e839027da213f2d954d68c5ca0eddb29fa2af4789e";

/// 发送 HANG 告警到钉钉
pub async fn send_hang_alert(details: &HangDetails) {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] 训练任务 HANG 告警", job_name);

    let config_path = get_config_path();
    tracing::warn!("DingTalk: 加载配置文件: {}", config_path);

    // 提前提取平台 API 配置为 owned String，使 Result<_, Box<dyn Error>> 在 .await 前被 drop
    let platform_config = match load_collector_config(&config_path) {
        Ok(cfg) => {
            tracing::warn!(
                "DingTalk: 配置加载成功, job_platform_api_url='{}'",
                cfg.job_platform_api_url
            );
            if cfg.job_platform_api_url.is_empty() {
                tracing::warn!("DingTalk: job_platform_api_url 为空，跳过平台 API 查询");
                None
            } else {
                Some((
                    cfg.job_platform_api_url,
                    cfg.job_platform_app_key,
                    cfg.job_platform_app_secret,
                    cfg.job_platform_user_id,
                ))
            }
        }
        Err(e) => {
            tracing::warn!("DingTalk: 配置文件加载失败: {}", e);
            None
        }
    };

    // 尝试从平台 API 获取任务/用户信息
    let job_info = if let Some((api_url, app_key, app_secret, user_id)) = platform_config {
        tracing::warn!(
            "DingTalk: 查询平台 API, url={}, job_id={}, user_id={}",
            api_url, job_name, user_id
        );
        let info = fetch_job_info(&api_url, &app_key, &app_secret, &user_id, &job_name).await;
        if info.is_none() {
            tracing::warn!("DingTalk: 平台 API 查询返回空，降级为基础消息");
        }
        info
    } else {
        None
    };

    // 构建 HANG 节点摘要（用于降级消息）
    let hang_nodes_str = if details.hang_nodes.is_empty() {
        "未知".to_string()
    } else {
        details.hang_nodes.join(", ")
    };
    let avg_similarity = if details.node_similarities.is_empty() {
        0.0f64
    } else {
        let sum: f64 = details.node_similarities.values().copied().sum();
        sum / details.node_similarities.len() as f64
    };

    // 构建 Markdown 消息体
    let text = if let Some(info) = job_info {
        format!(
            "## ⚠️ {title}\n\
            \n\
            | 字段 | 值 |\n\
            |---|---|\n\
            | **任务名** | {name} |\n\
            | **任务ID** | {job_id} |\n\
            | **创建者** | {creator_name}（{creator}） |\n\
            | **GPU型号** | {gpu_type} |\n\
            | **GPU数/节点** | {gpu_num} |\n\
            | **节点数** | {worker_num} |\n\
            | **HANG节点** | {hang_nodes} |\n\
            | **相似度** | {similarity:.3} |",
            title = title,
            name = info.name,
            job_id = job_name,
            creator_name = info.creator_name,
            creator = info.creator,
            gpu_type = info.gpu_type,
            gpu_num = info.gpu_num,
            worker_num = info.worker_num,
            hang_nodes = hang_nodes_str,
            similarity = avg_similarity,
        )
    } else {
        format!(
            "## ⚠️ {title}\n\
            \n\
            | 字段 | 值 |\n\
            |---|---|\n\
            | **任务ID** | {job_id} |\n\
            | **HANG节点** | {hang_nodes} |\n\
            | **相似度** | {similarity:.3} |\n\
            | **连续次数** | {consecutive} |",
            title = title,
            job_id = job_name,
            hang_nodes = hang_nodes_str,
            similarity = avg_similarity,
            consecutive = details.consecutive_high_similarity,
        )
    };

    let body = serde_json::json!({
        "msgtype": "markdown",
        "markdown": {
            "title": title,
            "text": text
        }
    });

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("DingTalk: 创建 HTTP client 失败: {}", e);
            return;
        }
    };

    match client.post(DINGTALK_WEBHOOK).json(&body).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => tracing::warn!("钉钉告警响应: status={}, body={}", status, body),
                Err(_) => tracing::warn!("钉钉告警响应: status={}", status),
            }
        }
        Err(e) => {
            tracing::warn!("DingTalk: 发送告警失败: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手动测试：向真实钉钉机器人发送一条测试告警
    ///
    /// 运行方式：
    ///   JOB_NAME="test-job" \
    ///   cargo test -p app --features ssr test_send_dingtalk_alert -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_send_dingtalk_alert() {
        send_hang_alert(&HangDetails::default()).await;
        println!("消息已发送，请检查钉钉群");
    }
}
