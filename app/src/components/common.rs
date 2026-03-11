use leptos::prelude::*;
use crate::models::HealthStatus;

/// 状态徽章组件
#[component]
pub fn StatusBadge(status: HealthStatus) -> impl IntoView {
    let (class, label) = match status {
        HealthStatus::Healthy => ("badge badge-healthy", "正常"),
        HealthStatus::Warning => ("badge badge-warning", "警告"),
        HealthStatus::Critical => ("badge badge-critical", "故障"),
    };

    view! {
        <span class=class>{label}</span>
    }
}

/// 热力色块组件 (根据比例 0.0-1.0 显示颜色)
#[component]
pub fn HeatCell(
    #[prop(into)] value: f32,
    #[prop(optional)] show_value: bool,
) -> impl IntoView {
    let level = heat_level(value);
    let class = format!("heat-cell heat-level-{}", level);

    view! {
        <div class=class>
            {if show_value {
                Some(format!("{:.0}%", value * 100.0))
            } else {
                None
            }}
        </div>
    }
}

/// 计算热力等级 (0-4)
fn heat_level(value: f32) -> u8 {
    if value <= 0.0 {
        0
    } else if value < 0.25 {
        1
    } else if value < 0.5 {
        2
    } else if value < 0.75 {
        3
    } else {
        4
    }
}

/// KPI 卡片组件
#[component]
pub fn KpiCard(
    #[prop(into)] title: String,
    #[prop(into)] value: String,
    #[prop(optional, into)] unit: Option<String>,
    #[prop(optional)] status: Option<HealthStatus>,
) -> impl IntoView {
    let status_class = status.map(|s| s.css_class()).unwrap_or("");

    view! {
        <div class=format!("kpi-card {}", status_class)>
            <div class="kpi-title">{title}</div>
            <div class="kpi-value">
                {value}
                {unit.map(|u| view! { <span class="kpi-unit">{u}</span> })}
            </div>
        </div>
    }
}

/// 健康分布块组件
#[component]
pub fn HealthDistribution(
    #[prop(into)] healthy: u16,
    #[prop(into)] warning: u16,
    #[prop(into)] critical: u16,
    #[prop(into)] label: String,
) -> impl IntoView {
    let total = healthy + warning + critical;

    view! {
        <div class="health-distribution">
            <div class="health-label">{label}</div>
            <div class="health-bars">
                <div
                    class="health-bar healthy"
                    style=format!("flex: {}", healthy)
                    title=format!("正常: {}", healthy)
                >
                    {healthy}
                </div>
                {(warning > 0).then(|| view! {
                    <div
                        class="health-bar warning"
                        style=format!("flex: {}", warning)
                        title=format!("警告: {}", warning)
                    >
                        {warning}
                    </div>
                })}
                {(critical > 0).then(|| view! {
                    <div
                        class="health-bar critical"
                        style=format!("flex: {}", critical)
                        title=format!("故障: {}", critical)
                    >
                        {critical}
                    </div>
                })}
            </div>
            <div class="health-total">共 {total}</div>
        </div>
    }
}

/// 加载指示器
#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div class="loading">
            <div class="loading-spinner"></div>
            <span>"加载中..."</span>
        </div>
    }
}

/// 错误显示（带重试按钮）
#[component]
pub fn ErrorDisplay(
    #[prop(into)] message: String,
    #[prop(optional)] on_retry: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div class="error-display error-panel">
            <div class="error-content">
                <span class="error-icon">"❌"</span>
                <div class="error-message">
                    <h3>"连接失败"</h3>
                    <p>{message}</p>
                </div>
            </div>
            {on_retry.map(|callback| view! {
                <button
                    class="retry-button"
                    on:click=move |_| callback.run(())
                >
                    "🔄 重试"
                </button>
            })}
        </div>
    }
}

/// Mock 模式警告横幅
#[component]
pub fn MockModeBanner() -> impl IntoView {
    view! {
        <div class="mock-mode-banner">
            <span class="mock-icon">"⚠️"</span>
            <span>"演示模式 - 显示模拟数据，非真实训练状态"</span>
        </div>
    }
}

/// 面包屑导航
#[component]
pub fn Breadcrumb(
    #[prop(into)] items: Vec<(String, String)>, // (label, href)
) -> impl IntoView {
    let items_len = items.len();
    view! {
        <nav class="breadcrumb">
            {items.into_iter().enumerate().map(|(i, (label, href))| {
                let is_last = i == items_len - 1;
                view! {
                    <a href=href class:active=is_last>{label}</a>
                    {(!is_last).then(|| view! { <span class="separator">"/"</span> })}
                }
            }).collect_view()}
        </nav>
    }
}
