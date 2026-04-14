use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    ParamSegment, StaticSegment,
};

pub mod adapter;
pub mod api;
pub mod components;
#[cfg(feature = "ssr")]
pub mod flamegraph;
#[cfg(feature = "ssr")]
pub mod hang_detector;
pub mod hang_types;
pub mod mock;
pub mod models;
pub mod rank_analysis_types;
#[cfg(feature = "ssr")]
pub mod rank_analyzer;

// 测试工具模块 - benches 需要通过 lib 访问
#[cfg(any(test, feature = "bench"))]
pub mod bench_utils;

use api::get_mock_mode_status;
use components::{Level1View, Level2View, Level3View, MockModeBanner};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="zh-CN">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let mock_mode_resource = Resource::new(|| (), |_| get_mock_mode_status());

    view! {
        <Stylesheet id="leptos" href="/pkg/super-trainning-collector.css"/>
        <Title text="训练任务监控面板"/>

        // Mock 模式警告横幅
        <Suspense fallback=|| ()>
            {move || {
                mock_mode_resource.get().map(|result| {
                    match result {
                        Ok(true) => Some(view! { <MockModeBanner /> }),
                        _ => None,
                    }
                })
            }}
        </Suspense>

        <Router>
            <main class="app-container">
                <Routes fallback=|| "页面未找到".into_view()>
                    <Route path=StaticSegment("") view=Level1View/>
                    <Route path=StaticSegment("nodes") view=Level2View/>
                    <Route path=(StaticSegment("nodes"), ParamSegment("ip")) view=Level3View/>
                </Routes>
            </main>
        </Router>
    }
}
