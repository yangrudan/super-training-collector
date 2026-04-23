use crate::api::get_node_flamegraph;
use crate::components::common::Loading;
use leptos::prelude::*;

/// 堆栈分析面板组件
#[component]
pub fn StackAnalysisPanel(#[prop(into)] node_ip: String) -> impl IntoView {
    let (loading, set_loading) = signal(false);
    let flamegraph_svg: RwSignal<Option<String>> = RwSignal::new(None);
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);

    let ip = node_ip.clone();
    let on_generate = move |_| {
        let ip = ip.clone();
        set_loading.set(true);
        flamegraph_svg.set(None);
        error_msg.set(None);
        leptos::task::spawn_local(async move {
            match get_node_flamegraph(ip).await {
                Ok(svg) => flamegraph_svg.set(Some(svg)),
                Err(e) => error_msg.set(Some(e.to_string())),
            }
            set_loading.set(false);
        });
    };

    let download_node_ip = StoredValue::new(node_ip.clone());

    view! {
        <section class="panel-surface stack-analysis">
            <div class="panel-header-line">
                <div>
                    <div class="section-label">"日志诊断"</div>
                    <h2 class="section-title">"堆栈分析"</h2>
                </div>
                <div class="stack-actions">
                    <Show when=move || flamegraph_svg.get().is_some()>
                        <button
                            class="collect-btn subtle"
                            on:click=move |_| {
                                if let Some(svg_content) = flamegraph_svg.get() {
                                    #[cfg(feature = "hydrate")]
                                    {
                                        use wasm_bindgen::JsCast;

                                        let filename = format!("flamegraph_{}_{}.svg", download_node_ip.get_value(), {
                                            use js_sys::Date;
                                            let date = Date::new_0();
                                            format!(
                                                "{:04}{:02}{:02}{:02}{:02}{:02}",
                                                date.get_full_year(),
                                                date.get_month() + 1,
                                                date.get_date(),
                                                date.get_hours(),
                                                date.get_minutes(),
                                                date.get_seconds()
                                            )
                                        });
                                        let document = web_sys::window().unwrap().document().unwrap();
                                        let mut blob_options = web_sys::BlobPropertyBag::new();
                                        blob_options.set_type("image/svg+xml");
                                        let blob = web_sys::Blob::new_with_str_sequence_and_options(
                                            &js_sys::Array::of1(&svg_content.into()),
                                            &blob_options,
                                        )
                                            .unwrap();
                                        let url =
                                            web_sys::Url::create_object_url_with_blob(&blob).unwrap();

                                        let a = document
                                            .create_element("a")
                                            .unwrap()
                                            .dyn_into::<web_sys::HtmlAnchorElement>()
                                            .unwrap();
                                        a.set_href(&url);
                                        a.set_download(&filename);
                                        a.click();

                                        web_sys::Url::revoke_object_url(&url).unwrap();
                                    }
                                }
                            }
                            title="下载火焰图 SVG"
                        >
                            "下载 SVG"
                        </button>
                    </Show>
                    <button
                        class="collect-btn"
                        on:click=on_generate
                        disabled=move || loading.get()
                    >
                        {move || if loading.get() { "采集中..." } else { "采集堆栈" }}
                    </button>
                </div>
            </div>

            <p class="section-note">
                "使用等宽日志式火焰图定位慢 Rank 的根因，重点观察 NCCL 等待、I/O 堵塞等热点函数。"
            </p>

            <Show when=move || loading.get()>
                <Loading />
            </Show>

            <Show when=move || error_msg.get().is_some()>
                <div class="stack-error">
                    {move || error_msg.get().unwrap_or_default()}
                </div>
            </Show>

            <Show when=move || flamegraph_svg.get().is_some()>
                <div class="flamegraph-container">
                    <div
                        class="flamegraph-svg"
                        inner_html=move || flamegraph_svg.get().unwrap_or_default()
                    />
                </div>
            </Show>

            <Show when=move || !loading.get() && flamegraph_svg.get().is_none() && error_msg.get().is_none()>
                <div class="stack-placeholder">
                    <p>"点击“采集堆栈”拉取该节点所有 Rank 的调用栈并生成火焰图。"</p>
                    <p class="hint">"面板保留完整错误信息，避免关键异常栈被直接截断。"</p>
                </div>
            </Show>
        </section>
    }
}
