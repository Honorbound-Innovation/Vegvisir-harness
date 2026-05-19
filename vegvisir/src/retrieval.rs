use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RetrievalDocument {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryRetriever {
    docs: HashMap<String, RetrievalDocument>,
}

impl InMemoryRetriever {
    pub fn add(&mut self, document: RetrievalDocument) {
        self.docs.insert(document.id.clone(), document);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<RetrievalDocument> {
        let query_terms = tokenize(query);
        let mut ranked: Vec<_> = self
            .docs
            .values()
            .filter_map(|doc| {
                let score = query_terms.intersection(&tokenize(&doc.text)).count();
                (score > 0).then(|| (score, doc.clone()))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        ranked.into_iter().take(limit).map(|(_, doc)| doc).collect()
    }
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
