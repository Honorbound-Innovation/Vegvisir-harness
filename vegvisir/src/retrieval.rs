use std::collections::{HashMap, HashSet, VecDeque};

pub const DEFAULT_MAX_RETRIEVAL_DOCUMENTS: usize = 256;
pub const DEFAULT_MAX_RETRIEVAL_DOCUMENT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_RETRIEVAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RetrievalDocument {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct InMemoryRetriever {
    docs: HashMap<String, RetrievalDocument>,
    order: VecDeque<String>,
    retained_bytes: usize,
    max_documents: usize,
    max_document_bytes: usize,
    max_bytes: usize,
}

impl InMemoryRetriever {
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_MAX_RETRIEVAL_DOCUMENTS,
            DEFAULT_MAX_RETRIEVAL_DOCUMENT_BYTES,
            DEFAULT_MAX_RETRIEVAL_BYTES,
        )
    }

    pub fn with_limits(max_documents: usize, max_document_bytes: usize, max_bytes: usize) -> Self {
        Self {
            docs: HashMap::new(),
            order: VecDeque::new(),
            retained_bytes: 0,
            max_documents: max_documents.max(1),
            max_document_bytes: max_document_bytes.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn add(&mut self, document: RetrievalDocument) {
        let mut document = document;
        let id = document.id.clone();
        document.text = bounded_text(&document.text, self.max_document_bytes.min(self.max_bytes));

        self.remove(&id);
        while self.docs.len() >= self.max_documents
            || self.retained_bytes.saturating_add(document.text.len()) > self.max_bytes
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.docs.remove(&oldest) {
                self.retained_bytes = self.retained_bytes.saturating_sub(removed.text.len());
            }
        }

        self.retained_bytes = self.retained_bytes.saturating_add(document.text.len());
        self.order.push_back(id.clone());
        self.docs.insert(id, document);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<RetrievalDocument> {
        let query_terms = tokenize(query);
        let mut ranked: Vec<_> = self
            .docs
            .iter()
            .filter_map(|doc| {
                let score = query_terms.intersection(&tokenize(&doc.1.text)).count();
                (score > 0).then(|| (score, doc.0.clone()))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        ranked
            .into_iter()
            .take(limit)
            .filter_map(|(_, id)| self.docs.get(&id).cloned())
            .collect()
    }

    fn remove(&mut self, id: &str) {
        if let Some(document) = self.docs.remove(id) {
            self.retained_bytes = self.retained_bytes.saturating_sub(document.text.len());
            self.order.retain(|entry| entry != id);
        }
    }
}

impl Default for InMemoryRetriever {
    fn default() -> Self {
        Self::new()
    }
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let marker = "\n[retrieval document truncated by memory bound]\n";
    if max_bytes <= marker.len() {
        return utf8_prefix(value, max_bytes);
    }
    let content_bytes = max_bytes - marker.len();
    let head_bytes = content_bytes / 2;
    let tail_bytes = content_bytes.saturating_sub(head_bytes);
    let head_end = safe_char_boundary_at_or_before(value, head_bytes);
    let tail_start = safe_char_boundary_at_or_after(value, value.len().saturating_sub(tail_bytes));
    format!("{}{}{}", &value[..head_end], marker, &value[tail_start..])
}

fn utf8_prefix(value: &str, max_bytes: usize) -> String {
    let mut prefix = String::new();
    for ch in value.chars() {
        if prefix.len().saturating_add(ch.len_utf8()) > max_bytes {
            break;
        }
        prefix.push(ch);
    }
    prefix
}

fn safe_char_boundary_at_or_before(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn safe_char_boundary_at_or_after(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub fn tokenize(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|part| {
            part.trim_matches(|ch: char| ".,:;!?()[]{}\"'".contains(ch))
                .to_lowercase()
        })
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, text: &str) -> RetrievalDocument {
        RetrievalDocument {
            id: id.to_string(),
            text: text.to_string(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn retriever_evicts_old_documents_and_bounds_text() {
        let mut retriever = InMemoryRetriever::with_limits(2, 32, 48);
        retriever.add(document("one", &"one needle ".repeat(20)));
        retriever.add(document("two", "two needle"));
        retriever.add(document("three", "three needle"));

        assert_eq!(retriever.len(), 2);
        assert!(retriever.retained_bytes() <= 48);
        assert!(retriever.search("one", 10).is_empty());
        assert_eq!(retriever.search("three", 10).len(), 1);
        assert!(
            retriever
                .search("two", 10)
                .first()
                .is_some_and(|item| item.text.len() <= 32)
        );
    }

    #[test]
    fn replacing_a_document_does_not_leave_stale_bytes_or_order_entries() {
        let mut retriever = InMemoryRetriever::with_limits(2, 64, 128);
        retriever.add(document("same", "old text"));
        retriever.add(document("same", "new text"));

        assert_eq!(retriever.len(), 1);
        assert_eq!(retriever.retained_bytes(), "new text".len());
        assert_eq!(retriever.search("new", 1)[0].text, "new text");
        assert!(retriever.search("old", 1).is_empty());
    }
}
