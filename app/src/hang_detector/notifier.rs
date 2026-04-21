//! 钉钉告警通知模块
//!
//! 当检测到 HANG 时，通过钉钉 Webhook 发送告警消息

use std::env;
use tracing;

const DINGTALK_WEBHOOK: &str = "https://oapi.dingtalk.com/robot/send?access_token=f573c7f5bcd6085ccce705e839027da213f2d954d68c5ca0eddb29fa2af4789e";

/// 发送 HANG 告警到钉钉
pub async fn send_hang_alert() {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] 训练任务 HANG 告警", job_name);
    let text = format!("[{}] 检测到HANG", job_name);

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
