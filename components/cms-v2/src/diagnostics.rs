use crate::maintenance::{LmlMaintenanceEngine, MaintenanceEngine, MaintenanceReport};
use crate::safety::redact_sensitive_text;
use crate::sqlite::{
    AuditEvent, CURRENT_SCHEMA_VERSION, LedgerStats, PromptCacheStats, SchemaMigrationRecord,
    SqliteLedger,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub health: DiagnosticHealth,
    pub stats: LedgerStats,
    pub migrations: Vec<SchemaMigrationRecord>,
    pub schema: DiagnosticSchemaStatus,
    pub maintenance: MaintenanceReport,
    pub prompt_cache: PromptCacheStats,
    pub observability: ObservabilitySummary,
    pub recent_audit_events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticHealth {
    pub status: String,
    pub schema_issues: usize,
    pub maintenance_issues: usize,
    pub error_issues: usize,
    pub warning_issues: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSchemaStatus {
    pub expected_version: i64,
    pub current_version: i64,
    pub applied_migrations: usize,
    pub missing_versions: Vec<i64>,
    pub unexpected_versions: Vec<i64>,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservabilitySummary {
    pub recent_audit_event_count: usize,
    pub recent_audit_event_types: BTreeMap<String, usize>,
    pub retrieval_events: usize,
    pub writeback_events: usize,
    pub import_events: usize,
    pub repair_events: usize,
    pub prompt_cache_events: usize,
    pub prompt_cache_manifests: i64,
    pub prompt_cache_usage_records: i64,
    pub prompt_cache_invalidations: i64,
    pub redacted_audit_messages: usize,
}

impl DiagnosticReport {
    pub fn is_healthy(&self) -> bool {
        self.health.status == "healthy"
    }
}

pub fn run_diagnostics(
    ledger: &SqliteLedger,
    lml_root: impl AsRef<Path>,
    audit_limit: usize,
) -> anyhow::Result<DiagnosticReport> {
    let stats = ledger.stats()?;
    let migrations = ledger.applied_migrations()?;
    let schema = schema_status(stats.schema_version, &migrations);
    let maintenance = LmlMaintenanceEngine::new(ledger, lml_root.as_ref()).run_full_check()?;
    let prompt_cache = ledger.prompt_cache_stats()?;
    let recent_audit_events = ledger
        .audit_events(audit_limit)?
        .into_iter()
        .map(redact_audit_event)
        .collect::<Vec<_>>();
    let observability = observability_summary(&recent_audit_events, &prompt_cache);
    let maintenance_error_issues = maintenance
        .issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .count();
    let warning_issues = maintenance
        .issues
        .iter()
        .filter(|issue| issue.severity == "warning")
        .count();
    let maintenance_issues = maintenance.issues.len();
    let schema_issues = usize::from(!schema.is_current);
    let error_issues = maintenance_error_issues + schema_issues;
    let status = if error_issues > 0 {
        "error"
    } else if warning_issues > 0 {
        "warning"
    } else {
        "healthy"
    }
    .to_string();

    Ok(DiagnosticReport {
        health: DiagnosticHealth {
            status,
            schema_issues,
            maintenance_issues,
            error_issues,
            warning_issues,
        },
        stats,
        migrations,
        schema,
        maintenance,
        prompt_cache,
        observability,
        recent_audit_events,
    })
}

fn redact_audit_event(mut event: AuditEvent) -> AuditEvent {
    event.message = redact_sensitive_text(&event.message);
    event
}

fn observability_summary(
    audit_events: &[AuditEvent],
    prompt_cache: &PromptCacheStats,
) -> ObservabilitySummary {
    let mut event_types = BTreeMap::new();
    let mut redacted_audit_messages = 0usize;
    for event in audit_events {
        *event_types.entry(event.event_type.clone()).or_insert(0) += 1;
        if event.message.contains("[REDACTED]") {
            redacted_audit_messages += 1;
        }
    }

    ObservabilitySummary {
        recent_audit_event_count: audit_events.len(),
        recent_audit_event_types: event_types,
        retrieval_events: count_events(audit_events, |event_type| event_type == "retrieval.logged"),
        writeback_events: count_events(audit_events, |event_type| {
            event_type.starts_with("writeback.") || event_type == "memory.created"
        }),
        import_events: count_events(audit_events, |event_type| event_type.starts_with("import.")),
        repair_events: count_events(audit_events, |event_type| {
            event_type.starts_with("repair.")
                || event_type.starts_with("maintenance.")
                || event_type.contains("reindexed")
        }),
        prompt_cache_events: count_events(audit_events, |event_type| {
            event_type.starts_with("prompt_cache.")
        }),
        prompt_cache_manifests: prompt_cache.manifests,
        prompt_cache_usage_records: prompt_cache.usage_records,
        prompt_cache_invalidations: prompt_cache.invalidations,
        redacted_audit_messages,
    }
}

fn count_events(audit_events: &[AuditEvent], matches_event_type: impl Fn(&str) -> bool) -> usize {
    audit_events
        .iter()
        .filter(|event| matches_event_type(&event.event_type))
        .count()
}

fn schema_status(
    current_version: i64,
    migrations: &[SchemaMigrationRecord],
) -> DiagnosticSchemaStatus {
    let applied_versions = migrations
        .iter()
        .map(|migration| migration.version)
        .collect::<std::collections::BTreeSet<_>>();
    let missing_versions = (1..=CURRENT_SCHEMA_VERSION)
        .filter(|version| !applied_versions.contains(version))
        .collect::<Vec<_>>();
    let unexpected_versions = applied_versions
        .iter()
        .copied()
        .filter(|version| *version > CURRENT_SCHEMA_VERSION || *version <= 0)
        .collect::<Vec<_>>();
    let is_current = current_version == CURRENT_SCHEMA_VERSION
        && missing_versions.is_empty()
        && unexpected_versions.is_empty();

    DiagnosticSchemaStatus {
        expected_version: CURRENT_SCHEMA_VERSION,
        current_version,
        applied_migrations: migrations.len(),
        missing_versions,
        unexpected_versions,
        is_current,
    }
}
