//! HANG 检测模块
//! 
//! 通过定时采集堆栈并计算 Jaccard 相似度来检测训练任务是否 HANG

pub mod config;
pub mod jaccard;
pub mod state;
pub mod detector;
pub mod scheduler;
#[cfg(feature = "ssr")]
pub mod runner;
#[cfg(feature = "ssr")]
pub mod logger;
#[cfg(feature = "ssr")]
pub mod notifier;

pub use config::HangConfig;
pub use state::{HangStatus, HangDetectorState, HangStatusSnapshot};
pub use detector::HangDetector;
#[cfg(feature = "ssr")]
pub use runner::start_hang_detector_scheduler;
#[cfg(feature = "ssr")]
pub use logger::HangLogger;
#[cfg(feature = "ssr")]
pub use notifier::send_hang_alert;
