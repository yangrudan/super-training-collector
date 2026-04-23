use crate::models::HealthStatus;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

#[component]
pub fn AppChrome(children: Children) -> impl IntoView {
    let location = use_location();
    let current_path = move || location.pathname.get();

    view! {
        <div class="app-shell">
            <aside class="left-area">
                <div class="left-area-brand">
                    <div class="brand-mark">"STC"</div>
                    <div class="brand-copy">
                        <span class="brand-title">"超级训练收集器"</span>
                        <span class="brand-subtitle">"Super Training Collector"</span>
                    </div>
                </div>

                <nav class="left-nav" aria-label="主导航">
                    <a
                        href="/"
                        class=move || nav_link_class(&current_path(), "/")
                    >
                        <span class="left-nav-icon">"总"</span>
                        <span class="left-nav-label">"全局总览"</span>
                    </a>
                    <a
                        href="/nodes"
                        class=move || nav_link_class(&current_path(), "/nodes")
                    >
                        <span class="left-nav-icon">"节"</span>
                        <span class="left-nav-label">"节点巡检"</span>
                    </a>
                </nav>

                <div class="left-area-footer">
                    <div class="sidebar-caption">"Zone"</div>
                    <div class="sidebar-value">"Enterprise GPU Ops"</div>
                </div>
            </aside>

            <div class="app-main">
                <header class="top-bar">
                    <div class="top-bar-copy">
                        <span class="top-bar-title">"训练资源调度与监控台"</span>
                        <span class="top-bar-subtitle">
                            "高信息密度、低干扰的企业级调度视图"
                        </span>
                    </div>

                    <div class="top-bar-actions">
                        <span class="zone-badge">"Zone A"</span>
                        <a class="secondary-action" href="/nodes">"节点控制台"</a>
                    </div>
                </header>

                <main class="app-content">
                    <div class="page-watermark" aria-hidden="true">
                        "Super Training Collector"
                    </div>
                    <div class="page-frame">{children()}</div>
                </main>
            </div>
        </div>
    }
}

/// 状态徽章组件
#[component]
pub fn StatusBadge(status: HealthStatus) -> impl IntoView {
    let (class, label) = match status {
        HealthStatus::Healthy => ("badge badge-healthy", "正常"),
        HealthStatus::Warning => ("badge badge-warning", "挂起"),
        HealthStatus::Critical => ("badge badge-critical", "异常"),
    };

    view! { <span class=class>{label}</span> }
}

/// 复制按钮
#[component]
pub fn CopyButton(
    #[prop(into)] value: String,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let copied = RwSignal::new(false);
    let copy_value = StoredValue::new(value);
    let button_label = label.unwrap_or_else(|| "复制".to_string());

    let on_copy = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();

        if copy_to_clipboard(&copy_value.get_value()) {
            copied.set(true);
            leptos::task::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(1200).await;
                copied.set(false);
            });
        }
    };

    view! {
        <button
            type="button"
            class="copy-button"
            title=copy_value.get_value()
            on:click=on_copy
        >
            {move || if copied.get() { "已复制".to_string() } else { button_label.clone() }}
        </button>
    }
}

/// 热力色块组件 (根据比例 0.0-1.0 显示颜色)
#[component]
pub fn HeatCell(#[prop(into)] value: f32, #[prop(optional)] show_value: bool) -> impl IntoView {
    let level = heat_level(value);
    let class = format!("heat-cell heat-level-{}", level);

    view! {
        <div class=class>
            <span class="heat-bar-fill" style=format!("width: {:.0}%;", value * 100.0)></span>
            <span class="heat-bar-label">
                {if show_value {
                    Some(format!("{:.0}%", value * 100.0))
                } else {
                    None
                }}
            </span>
        </div>
    }
}

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
        <article class=format!("data-card kpi-card {}", status_class)>
            <div class="kpi-title">{title}</div>
            <div class="kpi-value-row">
                <span class="kpi-value">{value}</span>
                {unit.map(|u| view! { <span class="kpi-unit">{u}</span> })}
            </div>
        </article>
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
    let healthy_percent = segment_percent(healthy, total);
    let warning_percent = segment_percent(warning, total);
    let critical_percent = segment_percent(critical, total);
    let aria_label = format!("{}状态分布", label);

    view! {
        <article class="panel-surface health-distribution">
            <div class="panel-header-line">
                <div>
                    <div class="section-label">{label}</div>
                    <div class="panel-subtitle">"状态分布"</div>
                </div>
                <div class="panel-stat">{format!("共 {}", total)}</div>
            </div>

            <div class="health-bars" role="img" aria-label=aria_label>
                <span class="health-bar healthy" style=format!("width: {}%;", healthy_percent)></span>
                <span class="health-bar warning" style=format!("width: {}%;", warning_percent)></span>
                <span class="health-bar critical" style=format!("width: {}%;", critical_percent)></span>
            </div>

            <div class="health-legend">
                <span><i class="legend-dot healthy"></i>"正常 " {healthy}</span>
                <span><i class="legend-dot warning"></i>"挂起 " {warning}</span>
                <span><i class="legend-dot critical"></i>"异常 " {critical}</span>
            </div>
        </article>
    }
}

/// 加载指示器
#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div class="loading panel-surface">
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
        <div class="error-display panel-surface">
            <div class="error-content">
                <span class="error-icon">"!"</span>
                <div class="error-message">
                    <h3>"数据获取失败"</h3>
                    <p>{message}</p>
                </div>
            </div>
            {on_retry.map(|callback| view! {
                <button class="retry-button" on:click=move |_| callback.run(())>
                    "重新加载"
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
            <span class="mock-icon">"!"</span>
            <span>"当前为演示模式，面板展示的是模拟训练数据。"</span>
        </div>
    }
}

/// 面包屑导航
#[component]
pub fn Breadcrumb(#[prop(into)] items: Vec<(String, String)>) -> impl IntoView {
    let items_len = items.len();

    view! {
        <nav class="breadcrumb" aria-label="Breadcrumb">
            {items
                .into_iter()
                .enumerate()
                .map(|(i, (label, href))| {
                    let is_last = i == items_len - 1;
                    view! {
                        <a href=href class:active=is_last>{label}</a>
                        {(!is_last).then(|| view! { <span class="separator">"/"</span> })}
                    }
                })
                .collect_view()}
        </nav>
    }
}

fn nav_link_class(path: &str, target: &str) -> &'static str {
    let is_active = if target == "/" {
        path == "/"
    } else {
        path.starts_with(target)
    };

    if is_active {
        "left-nav-link is-active"
    } else {
        "left-nav-link"
    }
}

fn segment_percent(value: u16, total: u16) -> u16 {
    if total == 0 {
        0
    } else {
        ((value as f32 / total as f32) * 100.0).round() as u16
    }
}

#[cfg(feature = "hydrate")]
fn copy_to_clipboard(value: &str) -> bool {
    use wasm_bindgen::JsValue;

    let copy_fn = js_sys::Function::new_with_args(
        "text",
        r#"
        const fallback = () => {
            const textarea = document.createElement('textarea');
            textarea.value = text;
            textarea.setAttribute('readonly', '');
            textarea.style.position = 'fixed';
            textarea.style.top = '-9999px';
            textarea.style.opacity = '0';
            document.body.appendChild(textarea);
            textarea.focus();
            textarea.select();
            const ok = document.execCommand('copy');
            document.body.removeChild(textarea);
            return ok;
        };

        if (navigator.clipboard && window.isSecureContext) {
            navigator.clipboard.writeText(text).catch(() => fallback());
            return true;
        }

        return fallback();
        "#,
    );

    copy_fn
        .call1(&JsValue::NULL, &JsValue::from_str(value))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(not(feature = "hydrate"))]
fn copy_to_clipboard(_: &str) -> bool {
    false
}
