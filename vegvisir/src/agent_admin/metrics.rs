use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::core::normalize_agent_id;

use super::{AgentMetrics, MetricsReport, ratio};

pub fn metrics_path(data_root: &Path, id: &str) -> PathBuf {
    data_root
        .join("agents")
        .join("metrics")
        .join(format!("{}.json", normalize_agent_id(id)))
}

pub fn load_metrics_report(data_root: &Path, id: &str) -> anyhow::Result<MetricsReport> {
    let path = metrics_path(data_root, id);
    let metrics = if path.exists() {
        serde_json::from_str::<AgentMetrics>(
            &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        )?
    } else {
        AgentMetrics::default()
    };
    let verification_total = metrics.verification_successes + metrics.verification_failures;
    let task_total = metrics.tasks_completed + metrics.tasks_failed;
    Ok(MetricsReport {
        id: id.to_string(),
        path,
        verification_success_rate: ratio(metrics.verification_successes, verification_total),
        task_success_rate: ratio(metrics.tasks_completed, task_total),
        metrics,
        warnings: Vec::new(),
    })
}
