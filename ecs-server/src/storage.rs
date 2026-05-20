//! SQLite 历史存储层
//!
//! - `metrics_ts`：每次 push 写一行精简指标，用于时序图
//! - `events`：HANG 起止、状态翻转、job_info 抓取等稀疏事件
//! - `latest_snapshot`：最新一帧完整 payload，重启时恢复内存
//! - `unnamed_alloc`：未命名任务计数器持久化
//!
//! 所有同步 rusqlite 调用都包在 `spawn_blocking` 内，不阻塞 tokio runtime。

use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const RETENTION_SECS: i64 = 24 * 3600;

#[derive(Clone)]
pub struct Storage {
    inner: Arc<Mutex<Connection>>,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsRow {
    pub ts: i64,
    pub total_nodes: Option<i64>,
    pub healthy_nodes: Option<i64>,
    pub warning_nodes: Option<i64>,
    pub critical_nodes: Option<i64>,
    pub current_step: Option<i64>,
    pub avg_gpu_utilization: Option<f64>,
    pub p50_step_time_ms: Option<f64>,
    pub p99_step_time_ms: Option<f64>,
    pub slow_node_ratio: Option<f64>,
    pub is_hanging: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub ts: i64,
    pub kind: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub collector_id: String,
    pub source_ip: String,
    pub collector_addr: String,
    pub last_seen: i64,
    pub job_id: String,
    pub job_info_json: Option<String>,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub struct UnnamedAllocRow {
    pub source_ip: String,
    pub name: String,
    pub counter: i64,
}

impl Storage {
    /// 打开（或新建）DB 文件并初始化 schema。
    pub fn open(path: impl Into<PathBuf>) -> rusqlite::Result<Self> {
        let path = path.into();
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", &"WAL")?;
        conn.pragma_update(None, "synchronous", &"NORMAL")?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<R>) -> rusqlite::Result<R> {
        let guard = self.inner.lock().expect("storage mutex poisoned");
        f(&guard)
    }

    // ─── 写入 ───────────────────────────────────────────────────────────────

    pub fn insert_metrics(&self, collector_id: &str, row: &MetricsRow) -> rusqlite::Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR REPLACE INTO metrics_ts \
                 (collector_id, ts, total_nodes, healthy_nodes, warning_nodes, critical_nodes, \
                  current_step, avg_gpu_utilization, p50_step_time_ms, p99_step_time_ms, \
                  slow_node_ratio, is_hanging) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    collector_id,
                    row.ts,
                    row.total_nodes,
                    row.healthy_nodes,
                    row.warning_nodes,
                    row.critical_nodes,
                    row.current_step,
                    row.avg_gpu_utilization,
                    row.p50_step_time_ms,
                    row.p99_step_time_ms,
                    row.slow_node_ratio,
                    row.is_hanging as i64,
                ],
            )?;
            Ok(())
        })
    }

    pub fn insert_event(
        &self,
        collector_id: &str,
        ts: i64,
        kind: &str,
        detail: &serde_json::Value,
    ) -> rusqlite::Result<()> {
        let detail_str = detail.to_string();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO events (collector_id, ts, kind, detail) VALUES (?1, ?2, ?3, ?4)",
                params![collector_id, ts, kind, detail_str],
            )?;
            Ok(())
        })
    }

    pub fn upsert_snapshot(&self, snap: &SnapshotRow) -> rusqlite::Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO latest_snapshot \
                 (collector_id, source_ip, collector_addr, last_seen, job_id, job_info_json, payload_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(collector_id) DO UPDATE SET \
                   source_ip=excluded.source_ip, \
                   collector_addr=excluded.collector_addr, \
                   last_seen=excluded.last_seen, \
                   job_id=excluded.job_id, \
                   job_info_json=COALESCE(excluded.job_info_json, latest_snapshot.job_info_json), \
                   payload_json=excluded.payload_json",
                params![
                    snap.collector_id,
                    snap.source_ip,
                    snap.collector_addr,
                    snap.last_seen,
                    snap.job_id,
                    snap.job_info_json,
                    snap.payload_json,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_snapshot_job_info(
        &self,
        collector_id: &str,
        job_info_json: &str,
    ) -> rusqlite::Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE latest_snapshot SET job_info_json=?1 WHERE collector_id=?2",
                params![job_info_json, collector_id],
            )?;
            Ok(())
        })
    }

    pub fn upsert_unnamed(&self, row: &UnnamedAllocRow) -> rusqlite::Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO unnamed_alloc (source_ip, name, counter) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(source_ip) DO NOTHING",
                params![row.source_ip, row.name, row.counter],
            )?;
            Ok(())
        })
    }

    // ─── 读取 ───────────────────────────────────────────────────────────────

    pub fn load_all_snapshots(&self) -> rusqlite::Result<Vec<SnapshotRow>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT collector_id, source_ip, collector_addr, last_seen, job_id, job_info_json, payload_json \
                 FROM latest_snapshot",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(SnapshotRow {
                        collector_id: r.get(0)?,
                        source_ip: r.get(1)?,
                        collector_addr: r.get(2)?,
                        last_seen: r.get(3)?,
                        job_id: r.get(4)?,
                        job_info_json: r.get(5)?,
                        payload_json: r.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn load_all_unnamed(&self) -> rusqlite::Result<Vec<UnnamedAllocRow>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT source_ip, name, counter FROM unnamed_alloc")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(UnnamedAllocRow {
                        source_ip: r.get(0)?,
                        name: r.get(1)?,
                        counter: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn max_unnamed_counter(&self) -> rusqlite::Result<i64> {
        self.with_conn(|c| {
            c.query_row("SELECT COALESCE(MAX(counter), 0) FROM unnamed_alloc", [], |r| {
                r.get::<_, i64>(0)
            })
        })
    }

    pub fn query_history(
        &self,
        collector_id: &str,
        since: i64,
        until: i64,
    ) -> rusqlite::Result<Vec<MetricsRow>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT ts, total_nodes, healthy_nodes, warning_nodes, critical_nodes, \
                        current_step, avg_gpu_utilization, p50_step_time_ms, p99_step_time_ms, \
                        slow_node_ratio, is_hanging \
                 FROM metrics_ts \
                 WHERE collector_id=?1 AND ts BETWEEN ?2 AND ?3 \
                 ORDER BY ts ASC",
            )?;
            let rows = stmt
                .query_map(params![collector_id, since, until], |r| {
                    Ok(MetricsRow {
                        ts: r.get(0)?,
                        total_nodes: r.get(1)?,
                        healthy_nodes: r.get(2)?,
                        warning_nodes: r.get(3)?,
                        critical_nodes: r.get(4)?,
                        current_step: r.get(5)?,
                        avg_gpu_utilization: r.get(6)?,
                        p50_step_time_ms: r.get(7)?,
                        p99_step_time_ms: r.get(8)?,
                        slow_node_ratio: r.get(9)?,
                        is_hanging: r.get::<_, i64>(10)? != 0,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn query_events(
        &self,
        collector_id: &str,
        since: i64,
        until: i64,
    ) -> rusqlite::Result<Vec<EventRow>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT ts, kind, detail FROM events \
                 WHERE collector_id=?1 AND ts BETWEEN ?2 AND ?3 \
                 ORDER BY ts ASC",
            )?;
            let rows = stmt
                .query_map(params![collector_id, since, until], |r| {
                    let detail_str: String = r.get(2)?;
                    let detail = serde_json::from_str(&detail_str)
                        .unwrap_or(serde_json::Value::Null);
                    Ok(EventRow {
                        ts: r.get(0)?,
                        kind: r.get(1)?,
                        detail,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    #[allow(dead_code)]
    pub fn last_metric(&self, collector_id: &str) -> rusqlite::Result<Option<MetricsRow>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT ts, total_nodes, healthy_nodes, warning_nodes, critical_nodes, \
                        current_step, avg_gpu_utilization, p50_step_time_ms, p99_step_time_ms, \
                        slow_node_ratio, is_hanging \
                 FROM metrics_ts WHERE collector_id=?1 ORDER BY ts DESC LIMIT 1",
                params![collector_id],
                |r| {
                    Ok(MetricsRow {
                        ts: r.get(0)?,
                        total_nodes: r.get(1)?,
                        healthy_nodes: r.get(2)?,
                        warning_nodes: r.get(3)?,
                        critical_nodes: r.get(4)?,
                        current_step: r.get(5)?,
                        avg_gpu_utilization: r.get(6)?,
                        p50_step_time_ms: r.get(7)?,
                        p99_step_time_ms: r.get(8)?,
                        slow_node_ratio: r.get(9)?,
                        is_hanging: r.get::<_, i64>(10)? != 0,
                    })
                },
            )
            .map(Some)
            .or_else(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    Ok(None)
                } else {
                    Err(e)
                }
            })
        })
    }

    // ─── 清理 ───────────────────────────────────────────────────────────────

    pub fn cleanup_older_than(&self, now: i64) -> rusqlite::Result<(usize, usize)> {
        let cutoff = now - RETENTION_SECS;
        self.with_conn(|c| {
            let m = c.execute("DELETE FROM metrics_ts WHERE ts < ?1", params![cutoff])?;
            let e = c.execute("DELETE FROM events WHERE ts < ?1", params![cutoff])?;
            Ok((m, e))
        })
    }
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS metrics_ts (
    collector_id        TEXT NOT NULL,
    ts                  INTEGER NOT NULL,
    total_nodes         INTEGER,
    healthy_nodes       INTEGER,
    warning_nodes       INTEGER,
    critical_nodes      INTEGER,
    current_step        INTEGER,
    avg_gpu_utilization REAL,
    p50_step_time_ms    REAL,
    p99_step_time_ms    REAL,
    slow_node_ratio     REAL,
    is_hanging          INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (collector_id, ts)
);
CREATE INDEX IF NOT EXISTS idx_metrics_ts_ts ON metrics_ts(ts);

CREATE TABLE IF NOT EXISTS events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    collector_id TEXT NOT NULL,
    ts           INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    detail       TEXT
);
CREATE INDEX IF NOT EXISTS idx_events_collector_ts ON events(collector_id, ts);

CREATE TABLE IF NOT EXISTS latest_snapshot (
    collector_id   TEXT PRIMARY KEY,
    source_ip      TEXT NOT NULL,
    collector_addr TEXT NOT NULL,
    last_seen      INTEGER NOT NULL,
    job_id         TEXT NOT NULL,
    job_info_json  TEXT,
    payload_json   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS unnamed_alloc (
    source_ip TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    counter   INTEGER NOT NULL
);
"#;

/// 从 payload 中抽取时序行。所有 NaN 字段会被转成 `None`。
pub fn extract_metrics(ts: i64, payload: &serde_json::Value) -> MetricsRow {
    let global = payload.get("global");
    let int = |key: &str| -> Option<i64> {
        global.and_then(|g| g.get(key)).and_then(|v| v.as_i64())
    };
    let uint = |key: &str| -> Option<i64> {
        global
            .and_then(|g| g.get(key))
            .and_then(|v| v.as_u64())
            .map(|n| n as i64)
    };
    let float = |key: &str| -> Option<f64> {
        global
            .and_then(|g| g.get(key))
            .and_then(|v| v.as_f64())
            .filter(|f| !f.is_nan())
    };

    let is_hanging = payload
        .get("hang")
        .and_then(|h| h.get("is_hanging"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    MetricsRow {
        ts,
        total_nodes: uint("total_nodes").or_else(|| int("total_nodes")),
        healthy_nodes: uint("healthy_nodes"),
        warning_nodes: uint("warning_nodes"),
        critical_nodes: uint("critical_nodes"),
        current_step: uint("current_step"),
        avg_gpu_utilization: float("global_avg_gpu_utilization"),
        p50_step_time_ms: float("global_p50_step_time_ms"),
        p99_step_time_ms: float("global_p99_step_time_ms"),
        slow_node_ratio: float("slow_node_ratio"),
        is_hanging,
    }
}

pub fn hang_status_of(payload: &serde_json::Value) -> String {
    payload
        .get("hang")
        .and_then(|h| h.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string()
}
