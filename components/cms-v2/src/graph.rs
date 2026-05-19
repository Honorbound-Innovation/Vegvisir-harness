use crate::core::MemoryObject;
use crate::sqlite::{SqliteLedger, content_hash};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphHit {
    pub memory_id: String,
    pub relation: String,
    pub depth: usize,
}

pub trait GraphIndex {
    fn upsert_memory(&self, memory: &MemoryObject) -> anyhow::Result<()>;
    fn delete_memory(&self, memory_id: &str) -> anyhow::Result<()>;
    fn related_memories(&self, memory_id: &str, depth: usize) -> anyhow::Result<Vec<GraphHit>>;
}

pub struct SqliteGraphIndex<'a> {
    ledger: &'a SqliteLedger,
}

impl<'a> SqliteGraphIndex<'a> {
    pub fn new(ledger: &'a SqliteLedger) -> Self {
        Self { ledger }
    }

    fn upsert_node(&self, id: &str, kind: &str, label: &str) -> anyhow::Result<()> {
        self.ledger.connection().execute(
            r#"
            INSERT INTO graph_nodes (id, kind, label)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                label = excluded.label
            "#,
            params![id, kind, label],
        )?;
        Ok(())
    }

    fn upsert_edge(
        &self,
        source_id: &str,
        target_id: &str,
        relation: &str,
        confidence: f64,
        memory_id: &str,
    ) -> anyhow::Result<()> {
        self.ledger.connection().execute(
            r#"
            INSERT INTO graph_edges (id, source_id, target_id, relation, confidence, memory_id)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                source_id = excluded.source_id,
                target_id = excluded.target_id,
                relation = excluded.relation,
                confidence = excluded.confidence,
                memory_id = excluded.memory_id
            "#,
            params![
                edge_id(source_id, relation, target_id, memory_id),
                source_id,
                target_id,
                relation,
                confidence,
                memory_id
            ],
        )?;
        Ok(())
    }

    fn neighbors(&self, node_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut stmt = self.ledger.connection().prepare(
            r#"
            SELECT target_id, relation FROM graph_edges WHERE source_id = ?1
            UNION ALL
            SELECT source_id, relation FROM graph_edges WHERE target_id = ?1
            "#,
        )?;
        let rows = stmt.query_map(params![node_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn relation_between(&self, left: &str, right: &str) -> anyhow::Result<Option<String>> {
        self.ledger
            .connection()
            .query_row(
                r#"
                SELECT relation FROM graph_edges
                WHERE (source_id = ?1 AND target_id = ?2)
                   OR (source_id = ?2 AND target_id = ?1)
                ORDER BY relation
                LIMIT 1
                "#,
                params![left, right],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }
}

impl GraphIndex for SqliteGraphIndex<'_> {
    fn upsert_memory(&self, memory: &MemoryObject) -> anyhow::Result<()> {
        self.delete_memory(&memory.id)?;
        self.upsert_node(&memory.id, "memory", &memory.title)?;

        for claim in &memory.claims {
            let claim_id = format!("claim:{}:{}", memory.id, claim.id);
            self.upsert_node(&claim_id, "claim", &claim.text)?;
            self.upsert_edge(
                &memory.id,
                &claim_id,
                "supports_claim",
                claim.confidence,
                &memory.id,
            )?;
        }

        for tag in &memory.tags {
            let tag_id = format!("tag:{tag}");
            self.upsert_node(&tag_id, "tag", tag)?;
            self.upsert_edge(&memory.id, &tag_id, "tagged", 1.0, &memory.id)?;
        }

        for link in &memory.links {
            self.upsert_node(
                &link.target_id,
                target_kind(&link.target_id),
                &link.target_id,
            )?;
            self.upsert_edge(
                &memory.id,
                &link.target_id,
                &link.relation,
                link.confidence,
                &memory.id,
            )?;
        }

        self.ledger.connection().execute(
            r#"
            INSERT INTO index_state (memory_id, index_name, content_hash, indexed_at, status)
            VALUES (?1, 'sqlite-graph', ?2, ?3, 'indexed')
            ON CONFLICT(memory_id, index_name) DO UPDATE SET
                content_hash = excluded.content_hash,
                indexed_at = excluded.indexed_at,
                status = excluded.status
            "#,
            params![memory.id, content_hash(memory), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn delete_memory(&self, memory_id: &str) -> anyhow::Result<()> {
        self.ledger.connection().execute(
            "DELETE FROM graph_edges WHERE memory_id = ?1",
            params![memory_id],
        )?;
        self.ledger.connection().execute(
            "DELETE FROM graph_nodes WHERE id LIKE ?1",
            params![format!("claim:{memory_id}:%")],
        )?;
        Ok(())
    }

    fn related_memories(&self, memory_id: &str, depth: usize) -> anyhow::Result<Vec<GraphHit>> {
        let mut hits = Vec::new();
        let mut visited = BTreeSet::new();
        let mut queued = VecDeque::from([(memory_id.to_string(), 0usize)]);
        visited.insert(memory_id.to_string());

        while let Some((node_id, current_depth)) = queued.pop_front() {
            if current_depth >= depth {
                continue;
            }
            for (neighbor, relation) in self.neighbors(&node_id)? {
                if !visited.insert(neighbor.clone()) {
                    continue;
                }
                let next_depth = current_depth + 1;
                if neighbor.starts_with("mem_")
                    && neighbor != memory_id
                    && self.ledger.memory_exists(&neighbor)?
                {
                    hits.push(GraphHit {
                        memory_id: neighbor.clone(),
                        relation: self
                            .relation_between(&node_id, &neighbor)?
                            .unwrap_or(relation.clone()),
                        depth: next_depth,
                    });
                }
                queued.push_back((neighbor, next_depth));
            }
        }

        hits.sort_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
                .then_with(|| left.relation.cmp(&right.relation))
        });
        Ok(hits)
    }
}

fn edge_id(source_id: &str, relation: &str, target_id: &str, memory_id: &str) -> String {
    format!("{memory_id}:{source_id}:{relation}:{target_id}")
}

fn target_kind(id: &str) -> &str {
    id.split_once(':').map(|(kind, _)| kind).unwrap_or("entity")
}
