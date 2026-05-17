//! 钉钉告警通知模块
//!
//! 当检测到 HANG 时，通过钉钉 Webhook 发送告警消息

use std::env;
use tracing;


const DINGTALK_WEBHOOK: &str = "https://oapi.dingtalk.com/robot/send?access_token=f573c7f5bcd6085ccce705e839027da213f2d954d68c5ca0eddb29fa2af4789e";

/// 发送 HANG 告警到钉钉
///
/// 若环境变量 `USER_DINGBOT` 存在，则在发送完主通知后，也向该 URL 发送同内容的通知。
pub async fn send_hang_alert(analysis_summary: Option<&str>) {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] 训练任务 HANG 告警", job_name);
    let text = build_hang_alert_markdown(&job_name, analysis_summary);

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

    send_to_webhook(&client, DINGTALK_WEBHOOK, &body, "主通知").await;

    if let Ok(user_dingbot_url) = env::var("USER_DINGBOT") {
        let url = user_dingbot_url.trim().to_string();
        if !url.is_empty() {
            send_to_webhook(&client, &url, &body, "USER_DINGBOT").await;
        }
    }
}

/// 向指定 Webhook URL 发送钉钉消息
async fn send_to_webhook(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
    label: &str,
) {
    match client.post(url).json(body).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(resp_body) => tracing::warn!(
                    "钉钉告警响应[{}]: status={}, body={}",
                    label,
                    status,
                    resp_body
                ),
                Err(_) => tracing::warn!("钉钉告警响应[{}]: status={}", label, status),
            }
        }
        Err(e) => {
            tracing::warn!("DingTalk[{}]: 发送告警失败: {}", label, e);
        }
    }
}

fn build_hang_alert_markdown(job_name: &str, analysis_summary: Option<&str>) -> String {
    let mut text = format!("### [{}] 检测到 HANG", job_name);

    if let Some(summary) = analysis_summary.map(str::trim).filter(|s| !s.is_empty()) {
        text.push_str("\n\n**分析结果可能是：**\n");
        text.push_str(summary);
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_alert_markdown_includes_analysis_summary() {
        let text =
            build_hang_alert_markdown("test-job", Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）"));

        assert!(text.contains("### [test-job] 检测到 HANG"));
        assert!(text.contains("**分析结果可能是：**"));
        assert!(text.contains("Rank 3"));
    }

    #[test]
    fn build_alert_markdown_without_analysis_summary() {
        let text = build_hang_alert_markdown("test-job", None);

        assert_eq!(text, "### [test-job] 检测到 HANG");
    }

    #[test]
    fn build_alert_markdown_contains_job_name() {
        let text = build_hang_alert_markdown("my-job", None);
        assert!(text.contains("my-job"));
    }

    /// 手动测试：向真实钉钉机器人发送一条测试告警
    ///
    /// 运行方式：
    ///   JOB_NAME="test-job" \
    ///   cargo test -p app --features ssr test_send_dingtalk_alert -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_send_dingtalk_alert() {
        send_hang_alert(Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）")).await;
        println!("消息已发送，请检查钉钉群");
    }

    /// 手动测试：同时向 USER_DINGBOT 机器人发送告警
    ///
    /// 运行方式：
    ///   JOB_NAME="test-job" \
    ///   USER_DINGBOT="https://oapi.dingtalk.com/robot/send?access_token=YOUR_TOKEN" \
    ///   cargo test -p app --features ssr test_send_user_dingbot_alert -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_send_user_dingbot_alert() {
        send_hang_alert(Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）")).await;
        println!("消息已发送，请检查主群和 USER_DINGBOT 群");
    }
}
