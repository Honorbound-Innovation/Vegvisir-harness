use crate::core::{MemorySearchResult, RetrievalBundle};
use crate::graph::{GraphIndex, SqliteGraphIndex};
use crate::sqlite::SqliteLedger;
use crate::vectors::{SqliteVectorIndex, VectorIndex};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalMode {
    Exact,
    Semantic,
    Graph,
    Hybrid,
    Recent,
    Project,
    Contradiction,
    DecisionHistory,
}

pub trait RagOrchestrator {
    fn retrieve(
        &self,
        query: &str,
        mode: RetrievalMode,
        limit: usize,
    ) -> anyhow::Result<RetrievalBundle>;
}

pub struct HybridRagOrchestrator<'a> {
    ledger: &'a SqliteLedger,
    max_context_chars: usize,
}

impl<'a> HybridRagOrchestrator<'a> {
    pub fn new(ledger: &'a SqliteLedger) -> Self {
        Self {
            ledger,
            max_context_chars: 12_000,
        }
    }

    pub fn with_max_context_chars(mut self, max_context_chars: usize) -> Self {
        self.max_context_chars = max_context_chars;
        self
    }

    fn retrieve_exact(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemorySearchResult>> {
        self.ledger.search_exact(query, limit)
    }

    fn retrieve_recent(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        self.ledger.recent_memories(query, limit)
    }

    fn retrieve_project(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let seeds = self.ledger.project_memories(query, limit)?;
        self.expand_graph(seeds, limit)
    }

    fn retrieve_decision_history(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        self.ledger.decision_history(query, limit)
    }

    fn retrieve_with_graph_expansion(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let exact = self.retrieve_exact(query, limit)?;
        self.expand_graph(exact, limit)
    }

    fn retrieve_semantic(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let vector = SqliteVectorIndex::new(self.ledger);
        let mut merged = BTreeMap::new();
        for hit in vector.semantic_search(query, limit)? {
            if let Some(memory) = self.ledger.get_memory(&hit.memory_id)? {
                merged
                    .entry(hit.memory_id.clone())
                    .and_modify(|result: &mut MemorySearchResult| {
                        result.score = result.score.max(hit.score);
                        result.reasons.push(format!(
                            "vector chunk {} score {:.3}",
                            hit.chunk_id, hit.score
                        ));
                    })
                    .or_insert_with(|| MemorySearchResult {
                        memory,
                        score: hit.score,
                        reasons: vec![format!(
                            "vector chunk {} score {:.3}",
                            hit.chunk_id, hit.score
                        )],
                    });
            }
        }
        Ok(sorted_limited(merged, limit))
    }

    fn retrieve_hybrid(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let mut merged = BTreeMap::new();

        for (rank, mut result) in self.retrieve_exact(query, limit)?.into_iter().enumerate() {
            result
                .reasons
                .push(hybrid_contribution_reason("exact", rank, result.score));
            merged.insert(result.memory.id.clone(), result);
        }

        for (rank, mut result) in self
            .retrieve_semantic(query, limit)?
            .into_iter()
            .enumerate()
        {
            result
                .reasons
                .push(hybrid_contribution_reason("semantic", rank, result.score));
            merged
                .entry(result.memory.id.clone())
                .and_modify(|existing: &mut MemorySearchResult| {
                    existing.score = existing.score.max(0.85 + result.score * 0.15);
                    existing.reasons.extend(result.reasons.clone());
                })
                .or_insert(result);
        }

        self.expand_graph(merged.into_values().collect(), limit)
    }

    fn retrieve_contradictions(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let expanded_limit = limit.saturating_mul(3).max(limit);
        let candidates = self.retrieve_hybrid(query, expanded_limit)?;
        let mut scored = Vec::new();

        for mut result in candidates {
            let mut contradiction_score = 0.0;

            for link in &result.memory.links {
                if is_contradiction_relation(&link.relation) {
                    contradiction_score += 0.45 * link.confidence;
                    result.reasons.push(format!(
                        "explicit contradiction link {} -> {}",
                        link.relation, link.target_id
                    ));
                }
            }

            for claim in &result.memory.claims {
                if looks_contradictory(&claim.text) {
                    contradiction_score += 0.2 * claim.confidence;
                    result.reasons.push(format!(
                        "claim {} contains contradiction language",
                        claim.id
                    ));
                }
            }

            if looks_contradictory(&result.memory.summary)
                || looks_contradictory(&result.memory.body)
            {
                contradiction_score += 0.1;
                result
                    .reasons
                    .push("summary/body contains contradiction language".to_string());
            }

            if contradiction_score > 0.0 {
                result.score = (result.score + contradiction_score).min(1.5);
                scored.push(result);
            }
        }

        scored.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.memory.id.cmp(&right.memory.id))
        });
        scored.truncate(limit);
        Ok(scored)
    }

    fn expand_graph(
        &self,
        seeds: Vec<MemorySearchResult>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let mut merged = BTreeMap::new();

        for result in seeds {
            merged.insert(result.memory.id.clone(), result);
        }

        let graph = SqliteGraphIndex::new(self.ledger);
        for seed_id in merged.keys().cloned().collect::<Vec<_>>() {
            for hit in graph.related_memories(&seed_id, 2)? {
                if merged.contains_key(&hit.memory_id) {
                    continue;
                }
                if let Some(memory) = self.ledger.get_memory(&hit.memory_id)? {
                    merged.insert(
                        hit.memory_id.clone(),
                        MemorySearchResult {
                            memory,
                            score: 0.75 / hit.depth as f64,
                            reasons: vec![format!(
                                "graph expansion from {seed_id} via {}",
                                hit.relation
                            )],
                        },
                    );
                }
            }
        }

        Ok(sorted_limited(merged, limit))
    }

    fn build_context(&self, results: &[MemorySearchResult]) -> String {
        let mut context = String::new();
        for result in results {
            let memory = &result.memory;
            let mut block = String::new();
            block.push_str("--- memory ---\n");
            block.push_str(&format!("id: {}\n", memory.id));
            block.push_str(&format!("type: {}\n", memory.memory_type));
            block.push_str(&format!("title: {}\n", memory.title));
            block.push_str(&format!("confidence: {}\n", memory.confidence));
            if !memory.tags.is_empty() {
                block.push_str(&format!("tags: {}\n", memory.tags.join(", ")));
            }
            if !memory.summary.trim().is_empty() {
                block.push_str("summary:\n");
                block.push_str(memory.summary.trim());
                block.push('\n');
            }
            if !memory.claims.is_empty() {
                block.push_str("claims:\n");
                for claim in &memory.claims {
                    block.push_str(&format!(
                        "- {} ({}) {}\n",
                        claim.id, claim.confidence, claim.text
                    ));
                }
            }
            if !memory.links.is_empty() {
                block.push_str("links:\n");
                for link in &memory.links {
                    block.push_str(&format!(
                        "- {} -> {} ({})\n",
                        link.relation, link.target_id, link.confidence
                    ));
                }
            }

            if context.len() + block.len() > self.max_context_chars {
                break;
            }
            context.push_str(&block);
        }
        context
    }
}

impl RagOrchestrator for HybridRagOrchestrator<'_> {
    fn retrieve(
        &self,
        query: &str,
        mode: RetrievalMode,
        limit: usize,
    ) -> anyhow::Result<RetrievalBundle> {
        let results = match mode {
            RetrievalMode::Exact => self.retrieve_exact(query, limit)?,
            RetrievalMode::Recent => self.retrieve_recent(query, limit)?,
            RetrievalMode::Semantic => self.retrieve_semantic(query, limit)?,
            RetrievalMode::Hybrid => self.retrieve_hybrid(query, limit)?,
            RetrievalMode::Graph => self.retrieve_with_graph_expansion(query, limit)?,
            RetrievalMode::Contradiction => self.retrieve_contradictions(query, limit)?,
            RetrievalMode::Project => self.retrieve_project(query, limit)?,
            RetrievalMode::DecisionHistory => self.retrieve_decision_history(query, limit)?,
        };
        let selected_memory_ids = results
            .iter()
            .map(|result| result.memory.id.clone())
            .collect::<Vec<_>>();
        self.ledger.log_retrieval(query, &selected_memory_ids)?;
        let context = self.build_context(&results);
        let trace = retrieval_trace(query, mode, limit, &results, context.len());
        Ok(RetrievalBundle {
            query: query.to_string(),
            results,
            context,
            trace,
        })
    }
}

fn retrieval_trace(
    query: &str,
    mode: RetrievalMode,
    limit: usize,
    results: &[MemorySearchResult],
    context_chars: usize,
) -> BTreeMap<String, Value> {
    let mut trace = BTreeMap::new();
    trace.insert("query".to_string(), Value::String(query.to_string()));
    trace.insert("mode".to_string(), Value::String(format!("{mode:?}")));
    trace.insert("limit".to_string(), Value::from(limit as u64));
    trace.insert(
        "result_count".to_string(),
        Value::from(results.len() as u64),
    );
    trace.insert(
        "context_chars".to_string(),
        Value::from(context_chars as u64),
    );
    trace.insert(
        "selected_memory_ids".to_string(),
        Value::Array(
            results
                .iter()
                .map(|result| Value::String(result.memory.id.clone()))
                .collect(),
        ),
    );
    trace.insert(
        "results".to_string(),
        Value::Array(
            results
                .iter()
                .map(|result| {
                    serde_json::json!({
                        "memory_id": result.memory.id,
                        "score": result.score,
                        "reasons": result.reasons,
                        "contributing_modes": contributing_modes(&result.reasons),
                    })
                })
                .collect(),
        ),
    );
    trace.insert(
        "mode_contribution_counts".to_string(),
        serde_json::to_value(mode_contribution_counts(results)).unwrap_or(Value::Null),
    );
    trace
}

fn hybrid_contribution_reason(mode: &str, rank: usize, score: f64) -> String {
    format!(
        "hybrid contribution mode={mode} rank={} score={score:.3}",
        rank + 1
    )
}

fn contributing_modes(reasons: &[String]) -> Vec<String> {
    let mut modes = reasons
        .iter()
        .filter_map(|reason| {
            reason
                .strip_prefix("hybrid contribution mode=")
                .and_then(|rest| rest.split_whitespace().next())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();
    modes.sort();
    modes.dedup();
    modes
}

fn mode_contribution_counts(results: &[MemorySearchResult]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for result in results {
        for mode in contributing_modes(&result.reasons) {
            *counts.entry(mode).or_insert(0) += 1;
        }
    }
    counts
}

fn sorted_limited(
    merged: BTreeMap<String, MemorySearchResult>,
    limit: usize,
) -> Vec<MemorySearchResult> {
    let mut results = merged.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.memory.id.cmp(&right.memory.id))
    });
    results.truncate(limit);
    results
}

fn is_contradiction_relation(relation: &str) -> bool {
    let normalized = relation.to_ascii_lowercase().replace(['-', ' '], "_");
    matches!(
        normalized.as_str(),
        "contradicts"
            | "contradicts_with"
            | "conflicts_with"
            | "conflicts"
            | "refutes"
            | "invalidates"
            | "opposes"
            | "disproves"
    )
}

fn looks_contradictory(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        " contradict",
        " conflict",
        " false",
        " invalid",
        " not ",
        " never ",
        " cannot ",
        " can't ",
        " should not ",
        " must not ",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}
