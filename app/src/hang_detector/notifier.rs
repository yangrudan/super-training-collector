//! 钉钉告警通知模块
//!
//! 当检测到 HANG 时，通过钉钉 Webhook 发送告警消息

use std::env;
use std::time::Duration;
use tracing;

const DINGTALK_WEBHOOK: &str = "https://oapi.dingtalk.com/robot/send?access_token=f573c7f5bcd6085ccce705e839027da213f2d954d68c5ca0eddb29fa2af4789e";
const INTRANET_ALERT_URL: &str =
    " http://compute-guard.meta-controller.nhss.zhejianglab.com/api/compute/guard/runtime/alert/events";
const INTRANET_ALERT_TOKEN: &str = "6w03zfedNsvXyDmWaEOckY7joxrURqPS";
const INTRANET_ALERT_ENABLED_ENV: &str = "INTRANET_ALERT_ENABLED";

/// 钉钉发送的最大重试次数（不含第一次尝试）
const MAX_RETRIES: usize = 2;
/// 重试退避时间
const RETRY_BACKOFFS_MS: [u64; MAX_RETRIES] = [500, 1500];
/// 单次请求超时
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// 发送 HANG 告警到钉钉
///
/// - `analysis_summary`：rank 分析结果摘要（可选）
/// - `event_id`：HANG 事件 ID（用于在 markdown 中加入唯一标识，避免钉钉服务端按相同内容去重）
/// - `duration_secs`：本次 HANG 已持续的秒数
///
/// 若环境变量 `USER_DINGBOT` 存在，则同时向该 URL 并行发送同内容的通知。
pub async fn send_hang_alert(
    analysis_summary: Option<&str>,
    event_id: Option<u64>,
    duration_secs: Option<u64>,
) -> bool {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] 训练任务 HANG 告警", job_name);
    let text = build_hang_alert_markdown(&job_name, analysis_summary, event_id, duration_secs);

    let body = serde_json::json!({
        "msgtype": "markdown",
        "markdown": {
            "title": title,
            "text": text
        }
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("DingTalk: 创建 HTTP client 失败: {}", e);
            return false;
        }
    };

    // 主通知和 USER_DINGBOT 并行发送，避免主通知超时拖慢用户群通知
    let user_dingbot = env::var("USER_DINGBOT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let intranet_body = build_enabled_intranet_alert_body(&text);

    match (user_dingbot, intranet_body) {
        (Some(url), Some(intranet_body)) => {
            let main_fut = send_with_retry(&client, DINGTALK_WEBHOOK, &body, "主通知");
            let user_fut = send_with_retry(&client, &url, &body, "USER_DINGBOT");
            let intranet_fut = send_intranet_alert_with_retry(&client, &intranet_body);
            let (main_ok, _user_ok, _intranet_ok) = tokio::join!(main_fut, user_fut, intranet_fut);
            main_ok
        }
        (Some(url), None) => {
            let main_fut = send_with_retry(&client, DINGTALK_WEBHOOK, &body, "主通知");
            let user_fut = send_with_retry(&client, &url, &body, "USER_DINGBOT");
            let (main_ok, _user_ok) = tokio::join!(main_fut, user_fut);
            main_ok
        }
        (None, Some(intranet_body)) => {
            let main_fut = send_with_retry(&client, DINGTALK_WEBHOOK, &body, "主通知");
            let intranet_fut = send_intranet_alert_with_retry(&client, &intranet_body);
            let (main_ok, _intranet_ok) = tokio::join!(main_fut, intranet_fut);
            main_ok
        }
        (None, None) => {
            let main_fut = send_with_retry(&client, DINGTALK_WEBHOOK, &body, "主通知");
            main_fut.await
        }
    }
}

/// 向指定 Webhook URL 发送钉钉消息，失败时按退避策略重试
async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
    label: &str,
) -> bool {
    let mut last_err: Option<String> = None;
    for attempt in 0..=MAX_RETRIES {
        match client.post(url).json(body).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                if status.is_success() && dingtalk_response_ok(&body_text) {
                    tracing::info!(
                        "钉钉告警发送成功[{}]: attempt={}, status={}, body={}",
                        label,
                        attempt,
                        status,
                        body_text
                    );
                    return true;
                }
                last_err = Some(format!("status={}, body={}", status, body_text));
                tracing::warn!(
                    "钉钉告警响应失败[{}]: attempt={}, {}",
                    label,
                    attempt,
                    last_err.as_deref().unwrap_or("")
                );
            }
            Err(e) => {
                last_err = Some(e.to_string());
                tracing::warn!(
                    "钉钉告警发送失败[{}]: attempt={}, err={}",
                    label,
                    attempt,
                    e
                );
            }
        }

        if attempt < MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(RETRY_BACKOFFS_MS[attempt])).await;
        }
    }

    tracing::error!(
        "钉钉告警最终失败[{}]: retries={}, last_err={}",
        label,
        MAX_RETRIES,
        last_err.unwrap_or_else(|| "unknown".to_string())
    );
    false
}

fn dingtalk_response_ok(body_text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body_text)
        .ok()
        .and_then(|v| v.get("errcode").and_then(|code| code.as_i64()))
        == Some(0)
}

fn build_hang_alert_markdown(
    job_name: &str,
    analysis_summary: Option<&str>,
    event_id: Option<u64>,
    duration_secs: Option<u64>,
) -> String {
    let mut text = format!("### [{}] 检测到 HANG", job_name);

    if let Some(id) = event_id {
        text.push_str(&format!("\n\n**事件 ID**: `{}`", id));
    }
    if let Some(secs) = duration_secs {
        text.push_str(&format!("\n\n**已持续**: {}s", secs));
    }

    if let Some(summary) = analysis_summary.map(str::trim).filter(|s| !s.is_empty()) {
        text.push_str("\n\n**分析结果可能是：**\n");
        text.push_str(summary);
    }

    text
}

fn build_enabled_intranet_alert_body(event_detail: &str) -> Option<serde_json::Value> {
    if !env_flag_enabled(INTRANET_ALERT_ENABLED_ENV) {
        return None;
    }

    let job_uuid = match env::var("JOB_NAME") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            tracing::warn!("内网后台告警已开启，但 JOB_NAME 环境变量为空，跳过发送");
            return None;
        }
    };
    let instance_uuid = match env::var("VC_MASTER_HOSTS")
        .ok()
        .and_then(|v| extract_instance_uuid_from_host(&v))
    {
        Some(v) => v,
        _ => {
            tracing::warn!(
                "内网后台告警已开启，但 VC_MASTER_HOSTS 环境变量为空或无法解析，跳过发送"
            );
            return None;
        }
    };

    Some(build_intranet_alert_body(
        job_uuid.trim(),
        &instance_uuid,
        &chrono::Utc::now().to_rfc3339(),
        event_detail,
    ))
}

fn env_flag_enabled(name: &str) -> bool {
    env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn extract_instance_uuid_from_host(hosts: &str) -> Option<String> {
    let host = hosts
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .find(|s| !s.is_empty())?;

    host.split_once("-master-")
        .or_else(|| host.split_once("-worker-"))
        .map(|(instance_uuid, _)| instance_uuid.to_string())
        .filter(|s| !s.is_empty())
}

fn build_intranet_alert_body(
    job_uuid: &str,
    instance_uuid: &str,
    event_time: &str,
    event_detail: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event_type": "作业hang住",
        "cluster_id": "zj-cluster-mixed-x10000-4",
        "namespace": "nhss-job",
        "status": "异常",
        "job_uuid": job_uuid,
        "instance_uuid": instance_uuid,
        "event_time": event_time,
        "event_detail": event_detail
    })
}

async fn send_intranet_alert_with_retry(
    client: &reqwest::Client,
    body: &serde_json::Value,
) -> bool {
    let mut last_err: Option<String> = None;
    for attempt in 0..=MAX_RETRIES {
        match client
            .post(INTRANET_ALERT_URL)
            .header("X-Token", INTRANET_ALERT_TOKEN)
            .json(body)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    tracing::info!(
                        "内网后台告警发送成功: attempt={}, status={}, body={}",
                        attempt,
                        status,
                        body_text
                    );
                    return true;
                }
                last_err = Some(format!("status={}, body={}", status, body_text));
                tracing::warn!(
                    "内网后台告警响应失败: attempt={}, {}",
                    attempt,
                    last_err.as_deref().unwrap_or("")
                );
            }
            Err(e) => {
                last_err = Some(e.to_string());
                tracing::warn!("内网后台告警发送失败: attempt={}, err={}", attempt, e);
            }
        }

        if attempt < MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(RETRY_BACKOFFS_MS[attempt])).await;
        }
    }

    tracing::error!(
        "内网后台告警最终失败: retries={}, last_err={}",
        MAX_RETRIES,
        last_err.unwrap_or_else(|| "unknown".to_string())
    );
    false
}

/// 发送 HANG 告警**解除**通知
///
/// - `event_id`：原 HANG 事件 ID（与告警通知里的 ID 对应，便于关联）
/// - `hang_duration_secs`：本次 HANG 总共持续了多少秒
pub async fn send_hang_recovery_alert(
    event_id: Option<u64>,
    hang_duration_secs: Option<u64>,
) -> bool {
    let job_name = env::var("JOB_NAME").unwrap_or_else(|_| "未知任务".to_string());
    let title = format!("[{}] HANG 告警解除", job_name);
    let text = build_hang_recovery_markdown(&job_name, event_id, hang_duration_secs);

    let body = serde_json::json!({
        "msgtype": "markdown",
        "markdown": {
            "title": title,
            "text": text
        }
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("DingTalk: 创建 HTTP client 失败: {}", e);
            return false;
        }
    };

    let user_dingbot = env::var("USER_DINGBOT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let main_fut = send_with_retry(&client, DINGTALK_WEBHOOK, &body, "主通知/恢复");
    if let Some(url) = user_dingbot {
        let user_fut = send_with_retry(&client, &url, &body, "USER_DINGBOT/恢复");
        let (main_ok, _user_ok) = tokio::join!(main_fut, user_fut);
        main_ok
    } else {
        main_fut.await
    }
}

fn build_hang_recovery_markdown(
    job_name: &str,
    event_id: Option<u64>,
    hang_duration_secs: Option<u64>,
) -> String {
    let mut text = format!("### [{}] HANG 告警解除", job_name);
    text.push_str("\n\n训练已恢复正常。");

    if let Some(id) = event_id {
        text.push_str(&format!("\n\n**关联事件 ID**: `{}`", id));
    }
    if let Some(secs) = hang_duration_secs {
        text.push_str(&format!("\n\n**HANG 总持续**: {}s", secs));
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_alert_markdown_includes_event_id_and_duration() {
        let text = build_hang_alert_markdown(
            "test-job",
            Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）"),
            Some(1700000000),
            Some(180),
        );

        assert!(text.contains("### [test-job] 检测到 HANG"));
        assert!(text.contains("**事件 ID**: `1700000000`"));
        assert!(text.contains("**已持续**: 180s"));
        assert!(text.contains("**分析结果可能是：**"));
        assert!(text.contains("Rank 3"));
    }

    #[test]
    fn build_alert_markdown_without_optional_fields() {
        let text = build_hang_alert_markdown("test-job", None, None, None);
        assert_eq!(text, "### [test-job] 检测到 HANG");
    }

    #[test]
    fn build_alert_markdown_contains_job_name() {
        let text = build_hang_alert_markdown("my-job", None, None, None);
        assert!(text.contains("my-job"));
    }

    #[test]
    fn env_flag_enabled_accepts_common_truthy_values() {
        env::set_var("TEST_INTRANET_ALERT_ENABLED", "true");
        assert!(env_flag_enabled("TEST_INTRANET_ALERT_ENABLED"));

        env::set_var("TEST_INTRANET_ALERT_ENABLED", "1");
        assert!(env_flag_enabled("TEST_INTRANET_ALERT_ENABLED"));

        env::set_var("TEST_INTRANET_ALERT_ENABLED", "on");
        assert!(env_flag_enabled("TEST_INTRANET_ALERT_ENABLED"));

        env::set_var("TEST_INTRANET_ALERT_ENABLED", "false");
        assert!(!env_flag_enabled("TEST_INTRANET_ALERT_ENABLED"));

        env::remove_var("TEST_INTRANET_ALERT_ENABLED");
    }

    #[test]
    fn build_intranet_alert_body_uses_runtime_job_and_instance_ids() {
        let body = build_intranet_alert_body(
            "jb-aitrain-156450823014475840",
            "ji-aitrain-156450823388817472",
            "2026-05-21T10:00:00Z",
            "### [test-job] 检测到 HANG",
        );

        assert_eq!(body["event_type"], "作业hang住");
        assert_eq!(body["cluster_id"], "zj-cluster-mixed-x10000-4");
        assert_eq!(body["namespace"], "nhss-job");
        assert_eq!(body["status"], "异常");
        assert_eq!(body["job_uuid"], "jb-aitrain-156450823014475840");
        assert_eq!(body["instance_uuid"], "ji-aitrain-156450823388817472");
        assert_eq!(body["event_time"], "2026-05-21T10:00:00Z");
        assert_eq!(body["event_detail"], "### [test-job] 检测到 HANG");
    }

    #[test]
    fn extract_instance_uuid_from_master_host() {
        assert_eq!(
            extract_instance_uuid_from_host(
                "ji-aitrain-156896073458669440-master-0.ji-aitrain-156896073458669440"
            )
            .as_deref(),
            Some("ji-aitrain-156896073458669440")
        );
    }

    #[test]
    fn extract_instance_uuid_from_first_host_when_multiple_hosts() {
        assert_eq!(
            extract_instance_uuid_from_host(
                "ji-aitrain-156896073458669440-worker-0.ji-aitrain-156896073458669440,ji-other-worker-0.ji-other"
            )
            .as_deref(),
            Some("ji-aitrain-156896073458669440")
        );
    }

    #[test]
    fn build_recovery_markdown_contains_title_and_event() {
        let text = build_hang_recovery_markdown("my-job", Some(1700000000), Some(240));
        assert!(text.contains("HANG 告警解除"));
        assert!(text.contains("my-job"));
        assert!(text.contains("**关联事件 ID**: `1700000000`"));
        assert!(text.contains("**HANG 总持续**: 240s"));
        assert!(text.contains("训练已恢复正常"));
    }

    #[test]
    fn build_recovery_markdown_without_optional_fields() {
        let text = build_hang_recovery_markdown("job-x", None, None);
        assert!(text.contains("### [job-x] HANG 告警解除"));
        assert!(text.contains("训练已恢复正常"));
        assert!(!text.contains("**关联事件 ID**"));
        assert!(!text.contains("**HANG 总持续**"));
    }

    /// 手动测试：向真实钉钉机器人发送一条测试告警
    #[tokio::test]
    #[ignore]
    async fn test_send_dingtalk_alert() {
        send_hang_alert(
            Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）"),
            Some(1700000000),
            Some(123),
        )
        .await;
        println!("消息已发送，请检查钉钉群");
    }

    /// 手动测试：同时向 USER_DINGBOT 机器人发送告警
    #[tokio::test]
    #[ignore]
    async fn test_send_user_dingbot_alert() {
        send_hang_alert(
            Some("1. Rank 3（节点: 10.0.0.1，异常分数: 4）"),
            Some(1700000001),
            Some(456),
        )
        .await;
        println!("消息已发送，请检查主群和 USER_DINGBOT 群");
    }
}
