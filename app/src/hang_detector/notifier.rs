//! 钉钉告警通知模块
//!
//! 当检测到 HANG 时，通过钉钉 Webhook 发送告警消息

use std::env;
use tracing;

use super::job_info_client::fetch_job_info;
use crate::flamegraph::{get_config_path, load_collector_config};

const DINGTALK_WEBHOOK: &str = "https://oapi.dingtalk.com/robot/send?access_token=f573c7f5bcd6085ccce705e839027da213f2d954d68c5ca0eddb29fa2af4789e";

/// 发送 HANG 告警到钉钉
pub async fn send_hang_alert() {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] 训练任务 HANG 告警", job_name);

    // 尝试从平台 API 获取任务/用户信息
    let job_info = {
        match load_collector_config(&get_config_path()) {
            Ok(cfg) if !cfg.job_platform_api_url.is_empty() => {
                fetch_job_info(
                    &cfg.job_platform_api_url,
                    &cfg.job_platform_app_key,
                    &cfg.job_platform_app_secret,
                    &cfg.job_platform_user_id,
                    &job_name,
                )
                .await
            }
            _ => None,
        }
    };

    // 构建 Markdown 消息体
    let text = if let Some(info) = job_info {
        format!(
            "## ⚠️ {}\n\
            \n\
            | 字段 | 值 |\n\
            |---|---|\n\
            | **任务名** | {} |\n\
            | **任务ID** | {} |\n\
            | **创建者** | {}（{}） |\n\
            | **GPU型号** | {} |\n\
            | **GPU数/节点** | {} |\n\
            | **节点数** | {} |",
            title,
            info.name,
            job_name,
            info.creator_name,
            info.creator,
            info.gpu_type,
            info.gpu_num,
            info.worker_num,
        )
    } else {
        format!("[{}] 检测到HANG", job_name)
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
        send_hang_alert().await;
        println!("消息已发送，请检查钉钉群");
    }
}
