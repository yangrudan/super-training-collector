//! HANG 检测状态指示灯组件
//! 
//! 在 Dashboard 首页显示训练任务是否 HANG 的状态

use leptos::prelude::*;
use crate::hang_types::HangStatus;

/// HANG 状态指示灯组件
#[component]
pub fn HangIndicator() -> impl IntoView {
    use crate::api::get_hang_status;
    
    // 定期刷新状态（每 10 秒）
    let (refresh_trigger, _set_refresh_trigger) = signal(0u32);
    
    // 注意：自动刷新功能需要在实际部署时启用
    // 目前简化实现，依赖手动刷新或页面重载
    
    let hang_status = Resource::new(
        move || refresh_trigger.get(),
        |_| get_hang_status(),
    );
    
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
                                    HangStatus::Warning => ("🟡", "可能 HANG", "hang-warning"),
                                    HangStatus::Normal => ("🟢", "运行正常", "hang-normal"),
                                    HangStatus::Collecting => ("🔵", "采集中", "hang-collecting"),
                                    HangStatus::Disabled => ("⚪", "未启用", "hang-disabled"),
                                    HangStatus::Error(_) => ("❌", "检测错误", "hang-error"),
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

/// 简化版 HANG 指示灯（只显示图标）
#[component]
pub fn HangIndicatorCompact() -> impl IntoView {
    use crate::api::get_hang_status;
    
    let hang_status = Resource::new(|| (), |_| get_hang_status());
    
    view! {
        <Suspense fallback=move || view! { <span class="hang-dot">"⏳"</span> }>
            {move || {
                hang_status.get().map(|result| {
                    match result {
                        Ok(snapshot) => {
                            let (icon, title) = match &snapshot.status {
                                HangStatus::Hang => ("🔴", "训练已 HANG"),
                                HangStatus::Warning => ("🟡", "可能 HANG"),
                                HangStatus::Normal => ("🟢", "运行正常"),
                                HangStatus::Collecting => ("🔵", "采集中"),
                                HangStatus::Disabled => ("⚪", "HANG 检测未启用"),
                                HangStatus::Error(_) => ("❌", "检测错误"),
                            };
                            view! { <span class="hang-dot" title=title>{icon}</span> }.into_any()
                        }
                        Err(_) => view! { <span class="hang-dot" title="HANG 检测未启用">"⚪"</span> }.into_any()
                    }
                })
            }}
        </Suspense>
    }
}
