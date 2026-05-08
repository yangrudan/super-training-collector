//! HANG 检测状态指示灯组件
//!
//! 在 Dashboard 首页显示训练任务是否 HANG 的状态

use crate::hang_types::HangStatus;
use leptos::prelude::*;

/// HANG 状态指示灯组件
#[component]
pub fn HangIndicator() -> impl IntoView {
    use crate::api::get_hang_status;

    // 定期刷新状态（每 10 秒）
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    // 客户端自动刷新
    #[cfg(not(feature = "ssr"))]
    {
        use leptos::task::spawn_local;
        spawn_local(async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(10_000).await;
                set_refresh_trigger.update(|n| *n = n.wrapping_add(1));
            }
        });
    }

    // 避免 SSR 时的未使用警告
    #[cfg(feature = "ssr")]
    let _ = set_refresh_trigger;

    let hang_status = Resource::new(move || refresh_trigger.get(), |_| get_hang_status());

    view! {
        <div class="hang-indicator">
            <Suspense fallback=move || view! {
                <div class="hang-status hang-loading">
                    <span class="hang-icon">"⏳"</span>
                    <span class="hang-label">"检测中..."</span>
                </div>
            }>
                {move || {
                    hang_status.get().map(|result| {
                        match result {
                            Ok(snapshot) => {
                                let (icon, label, class) = match &snapshot.status {
                                    HangStatus::Hang => ("🔴", "训练已 HANG", "hang-critical"),
                                    HangStatus::Normal => ("🟢", "运行正常", "hang-normal"),
                                    HangStatus::Disabled => ("⚪", "未启用", "hang-disabled"),
                                };

                                let details_text = if !snapshot.details.hang_nodes.is_empty() {
                                    format!("HANG 节点: {}", snapshot.details.hang_nodes.join(", "))
                                } else if !snapshot.details.node_similarities.is_empty() {
                                    let avg_sim: f64 = snapshot.details.node_similarities.values().sum::<f64>()
                                        / snapshot.details.node_similarities.len() as f64;
                                    format!("平均相似度: {:.1}%", avg_sim * 100.0)
                                } else {
                                    String::new()
                                };

                                let details_clone = details_text.clone();
                                let has_details = !details_text.is_empty();

                                view! {
                                    <div class=format!("hang-status {}", class) title=details_clone>
                                        <span class="hang-icon">{icon}</span>
                                        <span class="hang-label">{label}</span>
                                        {if has_details {
                                            Some(view! {
                                                <span class="hang-details">{details_text}</span>
                                            })
                                        } else {
                                            None
                                        }}
                                    </div>
                                }.into_any()
                            }
                            Err(_) => {
                                view! {
                                    <div class="hang-status hang-disabled">
                                        <span class="hang-icon">"⚪"</span>
                                        <span class="hang-label">"未启用"</span>
                                    </div>
                                }.into_any()
                            }
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// 简化版 HANG 指示灯（只显示图标，带颜色说明提示，每10秒自动刷新）
#[component]
pub fn HangIndicatorCompact() -> impl IntoView {
    use crate::api::get_hang_status;

    // 定期刷新状态（每 10 秒）
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    // 客户端自动刷新
    #[cfg(not(feature = "ssr"))]
    {
        use leptos::task::spawn_local;
        spawn_local(async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(10_000).await;
                set_refresh_trigger.update(|n| *n = n.wrapping_add(1));
            }
        });
    }

    // 避免 SSR 时的未使用警告
    #[cfg(feature = "ssr")]
    let _ = set_refresh_trigger;

    let hang_status = Resource::new(move || refresh_trigger.get(), |_| get_hang_status());

    // 颜色说明
    let color_legend = "HANG 检测状态 (每10秒自动刷新):\n\
        🔴 红灯: 训练已 HANG（堆栈连续多次无变化）\n\
        🟢 绿灯: 运行正常\n\
        ⚪ 白灯: 检测未启用";

    view! {
        <Suspense fallback=move || view! { <span class="hang-dot" title="加载中...">"⏳"</span> }>
            {move || {
                hang_status.get().map(|result| {
                    match result {
                        Ok(snapshot) => {
                            let (icon, status_text) = match &snapshot.status {
                                HangStatus::Hang => ("🔴", "训练已 HANG"),
                                HangStatus::Normal => ("🟢", "运行正常"),
                                HangStatus::Disabled => ("⚪", "检测未启用"),
                            };

                            // 组合当前状态和颜色说明
                            let tooltip = format!("当前: {}\n\n{}", status_text, color_legend);

                            view! { <span class="hang-dot" title=tooltip>{icon}</span> }.into_any()
                        }
                        Err(_) => {
                            let tooltip = format!("当前: 检测未启用\n\n{}", color_legend);
                            view! { <span class="hang-dot" title=tooltip>"⚪"</span> }.into_any()
                        }
                    }
                })
            }}
        </Suspense>
    }
}
