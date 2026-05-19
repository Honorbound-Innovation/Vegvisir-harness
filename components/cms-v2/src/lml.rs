use crate::core::{Claim, MemoryLink, MemoryObject, MemorySource};
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

pub const CURRENT_LML_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum LmlError {
    #[error("expected {expected} at byte {at}")]
    Expected { expected: String, at: usize },
    #[error("unexpected token at byte {at}: {token}")]
    Unexpected { token: String, at: usize },
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("invalid field `{field}`: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("unsupported LML schema version {found}; current version is {current}")]
    UnsupportedSchemaVersion { found: i64, current: i64 },
    #[error("unknown field `{field}` at {scope}")]
    UnknownField { scope: &'static str, field: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LmlError>;

pub struct LmlParser;
pub struct LmlWriter;
pub struct LmlValidator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LmlParserOptions {
    pub allow_unknown_fields: bool,
    pub allow_future_schema_versions: bool,
}

impl LmlParserOptions {
    pub fn strict() -> Self {
        Self {
            allow_unknown_fields: false,
            allow_future_schema_versions: false,
        }
    }

    pub fn permissive() -> Self {
        Self {
            allow_unknown_fields: true,
            allow_future_schema_versions: false,
        }
    }

    pub fn future_compatible() -> Self {
        Self {
            allow_unknown_fields: true,
            allow_future_schema_versions: true,
        }
    }
}

impl Default for LmlParserOptions {
    fn default() -> Self {
        Self::permissive()
    }
}

impl LmlParser {
    pub fn parse_file(path: impl AsRef<Path>) -> Result<MemoryObject> {
        Self::parse_text(&fs::read_to_string(path)?)
    }

    pub fn parse_text(text: &str) -> Result<MemoryObject> {
        Self::parse_text_with_options(text, LmlParserOptions::default())
    }

    pub fn parse_text_strict(text: &str) -> Result<MemoryObject> {
        Self::parse_text_with_options(text, LmlParserOptions::strict())
    }

    pub fn parse_text_with_options(text: &str, options: LmlParserOptions) -> Result<MemoryObject> {
        let mut parser = Parser::new(text, options);
        parser.parse_memory()
    }
}

impl LmlWriter {
    pub fn write_file(memory: &MemoryObject, path: impl AsRef<Path>) -> Result<()> {
        fs::write(path, Self::to_text(memory)?)?;
        Ok(())
    }

    pub fn to_text(memory: &MemoryObject) -> Result<String> {
        LmlValidator::validate(memory)?;
        let mut out = String::new();
        out.push_str("memory {\n");
        out.push_str(&format!(
            "    schema_version: {CURRENT_LML_SCHEMA_VERSION}\n"
        ));
        push_field(&mut out, 1, "id", &memory.id);
        push_field(&mut out, 1, "type", &memory.memory_type);
        push_field(&mut out, 1, "title", &memory.title);
        push_field(&mut out, 1, "created", &memory.created_at.to_rfc3339());
        push_field(&mut out, 1, "updated", &memory.updated_at.to_rfc3339());
        out.push_str(&format!("    confidence: {}\n", memory.confidence));
        if let Some(source) = &memory.source {
            push_field(&mut out, 1, "source", &source.reference);
            push_field(&mut out, 1, "source_kind", &source.kind);
        }
        push_triple_field(&mut out, 1, "summary", &memory.summary);
        if !memory.body.is_empty() {
            push_triple_field(&mut out, 1, "body", &memory.body);
        }
        if !memory.claims.is_empty() {
            out.push_str("\n    claims {\n");
            for claim in &memory.claims {
                out.push_str("        claim {\n");
                push_field(&mut out, 3, "id", &claim.id);
                push_field(&mut out, 3, "text", &claim.text);
                out.push_str(&format!("            confidence: {}\n", claim.confidence));
                if let Some(source) = &claim.source {
                    push_field(&mut out, 3, "source", source);
                }
                out.push_str("        }\n");
            }
            out.push_str("    }\n");
        }
        if !memory.links.is_empty() {
            out.push_str("\n    links {\n");
            for link in &memory.links {
                out.push_str("        link {\n");
                push_field(&mut out, 3, "target", &link.target_id);
                push_field(&mut out, 3, "relation", &link.relation);
                out.push_str(&format!("            confidence: {}\n", link.confidence));
                out.push_str("        }\n");
            }
            out.push_str("    }\n");
        }
        if !memory.tags.is_empty() || !memory.metadata.is_empty() {
            out.push_str("\n    retrieval {\n");
            if !memory.tags.is_empty() {
                let tags = memory
                    .tags
                    .iter()
                    .map(|tag| format!("\"{}\"", escape(tag)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("        tags: [{tags}]\n"));
            }
            for (key, value) in &memory.metadata {
                push_field(&mut out, 2, key, value);
            }
            out.push_str("    }\n");
        }
        out.push_str("}\n");
        Ok(out)
    }
}

impl LmlValidator {
    pub fn validate(memory: &MemoryObject) -> Result<()> {
        if memory.id.trim().is_empty() {
            return Err(LmlError::MissingField("id"));
        }
        if memory.memory_type.trim().is_empty() {
            return Err(LmlError::MissingField("type"));
        }
        if memory.title.trim().is_empty() {
            return Err(LmlError::MissingField("title"));
        }
        validate_confidence("confidence", memory.confidence)?;
        for claim in &memory.claims {
            if claim.id.trim().is_empty() {
                return Err(LmlError::InvalidField {
                    field: "claims.id",
                    message: "claim id must not be empty".to_string(),
                });
            }
            if claim.text.trim().is_empty() {
                return Err(LmlError::InvalidField {
                    field: "claims.text",
                    message: "claim text must not be empty".to_string(),
                });
            }
            validate_confidence("claims.confidence", claim.confidence)?;
        }
        for link in &memory.links {
            if link.target_id.trim().is_empty() {
                return Err(LmlError::InvalidField {
                    field: "links.target",
                    message: "link target must not be empty".to_string(),
                });
            }
            validate_confidence("links.confidence", link.confidence)?;
        }
        Ok(())
    }
}

fn validate_confidence(field: &'static str, value: f64) -> Result<()> {
    if !(0.0..=1.0).contains(&value) {
        return Err(LmlError::InvalidField {
            field,
            message: "must be between 0.0 and 1.0".to_string(),
        });
    }
    Ok(())
}

fn push_field(out: &mut String, indent: usize, key: &str, value: &str) {
    out.push_str(&"    ".repeat(indent));
    out.push_str(key);
    out.push_str(": \"");
    out.push_str(&escape(value));
    out.push_str("\"\n");
}

fn push_triple_field(out: &mut String, indent: usize, key: &str, value: &str) {
    out.push_str(&"    ".repeat(indent));
    out.push_str(key);
    out.push_str(": \"\"\"\n");
    out.push_str(value.trim());
    out.push('\n');
    out.push_str(&"    ".repeat(indent));
    out.push_str("\"\"\"\n");
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    String(String),
    Number(f64),
    Array(Vec<Value>),
    Block(BTreeMap<String, Vec<Value>>),
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    options: LmlParserOptions,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, options: LmlParserOptions) -> Self {
        Self {
            text,
            pos: 0,
            options,
        }
    }

    fn parse_memory(&mut self) -> Result<MemoryObject> {
        self.skip_ws();
        self.expect_ident("memory")?;
        let block = self.parse_block()?;
        self.skip_ws();
        if self.pos < self.text.len() {
            return Err(self.unexpected("trailing input"));
        }
        let map = match block {
            Value::Block(map) => map,
            _ => unreachable!(),
        };
        memory_from_map(map, self.options)
    }

    fn parse_block(&mut self) -> Result<Value> {
        self.skip_ws();
        self.expect_char('{')?;
        let mut map: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.consume_char('}') {
                break;
            }
            let key = self.parse_ident()?;
            self.skip_ws();
            let value = if self.consume_char(':') {
                self.parse_value()?
            } else {
                Value::Block(match self.parse_block()? {
                    Value::Block(map) => map,
                    _ => unreachable!(),
                })
            };
            map.entry(key).or_default().push(value);
        }
        Ok(Value::Block(map))
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.skip_ws();
        match self.peek_char() {
            Some('"') => self.parse_string().map(Value::String),
            Some('[') => self.parse_array(),
            Some('-') | Some('0'..='9') => self.parse_number().map(Value::Number),
            Some('{') => self.parse_block(),
            _ => Err(self.expected("value")),
        }
    }

    fn parse_array(&mut self) -> Result<Value> {
        self.expect_char('[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.consume_char(']') {
                break;
            }
            values.push(self.parse_value()?);
            self.skip_ws();
            let _ = self.consume_char(',');
        }
        Ok(Value::Array(values))
    }

    fn parse_string(&mut self) -> Result<String> {
        if self.text[self.pos..].starts_with("\"\"\"") {
            self.pos += 3;
            let start = self.pos;
            if let Some(end) = self.text[start..].find("\"\"\"") {
                self.pos = start + end + 3;
                return Ok(self.text[start..start + end].trim().to_string());
            }
            return Err(self.expected("closing triple quote"));
        }

        self.expect_char('"')?;
        let mut out = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.next_char().ok_or_else(|| self.expected("escape"))?;
                    out.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        '"' => '"',
                        '\\' => '\\',
                        other => other,
                    });
                }
                other => out.push(other),
            }
        }
        Err(self.expected("closing quote"))
    }

    fn parse_number(&mut self) -> Result<f64> {
        let start = self.pos;
        while matches!(self.peek_char(), Some('-' | '.' | '0'..='9')) {
            self.pos += self.peek_char().unwrap().len_utf8();
        }
        self.text[start..self.pos]
            .parse::<f64>()
            .map_err(|err| LmlError::InvalidField {
                field: "number",
                message: err.to_string(),
            })
    }

    fn parse_ident(&mut self) -> Result<String> {
        self.skip_ws();
        let start = self.pos;
        while matches!(
            self.peek_char(),
            Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-')
        ) {
            self.pos += self.peek_char().unwrap().len_utf8();
        }
        if self.pos == start {
            return Err(self.expected("identifier"));
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn expect_ident(&mut self, expected: &str) -> Result<()> {
        let found = self.parse_ident()?;
        if found == expected {
            Ok(())
        } else {
            Err(self.expected(expected))
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<()> {
        self.skip_ws();
        if self.consume_char(expected) {
            Ok(())
        } else {
            Err(self.expected(&expected.to_string()))
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        loop {
            while matches!(self.peek_char(), Some(ch) if ch.is_whitespace()) {
                self.pos += self.peek_char().unwrap().len_utf8();
            }
            if self.text[self.pos..].starts_with("//") {
                while !matches!(self.peek_char(), None | Some('\n')) {
                    self.pos += self.peek_char().unwrap().len_utf8();
                }
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn expected(&self, expected: &str) -> LmlError {
        LmlError::Expected {
            expected: expected.to_string(),
            at: self.pos,
        }
    }

    fn unexpected(&self, token: &str) -> LmlError {
        LmlError::Unexpected {
            token: token.to_string(),
            at: self.pos,
        }
    }
}

fn memory_from_map(
    map: BTreeMap<String, Vec<Value>>,
    options: LmlParserOptions,
) -> Result<MemoryObject> {
    validate_lml_schema_version(&map, options)?;
    if !options.allow_unknown_fields {
        validate_known_top_level_fields(&map)?;
    }

    let id = string_field(&map, "id")?;
    let memory_type = string_field(&map, "type")?;
    let title = string_field(&map, "title")?;
    let summary = optional_string_field(&map, "summary").unwrap_or_default();
    let body = optional_string_field(&map, "body").unwrap_or_default();
    let confidence = optional_number_field(&map, "confidence").unwrap_or(1.0);
    let created_at = parse_datetime(optional_string_field(&map, "created"), "created")?;
    let updated_at = parse_datetime(optional_string_field(&map, "updated"), "updated")?;
    let source = optional_string_field(&map, "source").map(|reference| MemorySource {
        kind: optional_string_field(&map, "source_kind").unwrap_or_else(|| "lml".to_string()),
        reference,
    });
    let mut memory = MemoryObject {
        id: id.clone(),
        memory_type,
        title,
        summary,
        body,
        claims: claims_from_map(&map)?,
        links: links_from_map(&map, &id)?,
        metadata: BTreeMap::new(),
        confidence,
        created_at,
        updated_at,
        source,
        tags: tags_from_map(&map)?,
    };
    if let Some(Value::Block(retrieval)) = first(&map, "retrieval") {
        for (key, values) in retrieval {
            if key == "tags" {
                continue;
            }
            if let Some(Value::String(value)) = values.first() {
                memory.metadata.insert(key.clone(), value.clone());
            }
        }
    }
    LmlValidator::validate(&memory)?;
    Ok(memory)
}

fn validate_lml_schema_version(
    map: &BTreeMap<String, Vec<Value>>,
    options: LmlParserOptions,
) -> Result<()> {
    let Some(version) = optional_integer_field(map, "schema_version")? else {
        return Ok(());
    };
    if version < 1 {
        return Err(LmlError::InvalidField {
            field: "schema_version",
            message: "schema version must be at least 1".to_string(),
        });
    }
    if version > CURRENT_LML_SCHEMA_VERSION && !options.allow_future_schema_versions {
        return Err(LmlError::UnsupportedSchemaVersion {
            found: version,
            current: CURRENT_LML_SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn validate_known_top_level_fields(map: &BTreeMap<String, Vec<Value>>) -> Result<()> {
    const KNOWN_FIELDS: &[&str] = &[
        "schema_version",
        "id",
        "type",
        "title",
        "created",
        "updated",
        "confidence",
        "source",
        "source_kind",
        "summary",
        "body",
        "claims",
        "links",
        "retrieval",
    ];
    for key in map.keys() {
        if !KNOWN_FIELDS.contains(&key.as_str()) {
            return Err(LmlError::UnknownField {
                scope: "memory",
                field: key.clone(),
            });
        }
    }
    Ok(())
}

fn claims_from_map(map: &BTreeMap<String, Vec<Value>>) -> Result<Vec<Claim>> {
    let mut claims = Vec::new();
    if let Some(Value::Block(claims_block)) = first(map, "claims") {
        for value in claims_block.get("claim").into_iter().flatten() {
            if let Value::Block(claim_map) = value {
                claims.push(Claim {
                    id: string_field(claim_map, "id")?,
                    text: string_field(claim_map, "text")?,
                    confidence: optional_number_field(claim_map, "confidence").unwrap_or(1.0),
                    source: optional_string_field(claim_map, "source"),
                });
            }
        }
    }
    Ok(claims)
}

fn links_from_map(map: &BTreeMap<String, Vec<Value>>, source_id: &str) -> Result<Vec<MemoryLink>> {
    let mut links = Vec::new();
    if let Some(Value::Block(links_block)) = first(map, "links") {
        for (relation, values) in links_block {
            for value in values {
                match value {
                    Value::String(target_id) => links.push(MemoryLink {
                        source_id: source_id.to_string(),
                        target_id: target_id.clone(),
                        relation: relation.clone(),
                        confidence: 1.0,
                    }),
                    Value::Block(link_map) if relation == "link" => links.push(MemoryLink {
                        source_id: source_id.to_string(),
                        target_id: string_field(link_map, "target")?,
                        relation: string_field(link_map, "relation")?,
                        confidence: optional_number_field(link_map, "confidence").unwrap_or(1.0),
                    }),
                    _ => {}
                }
            }
        }
    }
    Ok(links)
}

fn tags_from_map(map: &BTreeMap<String, Vec<Value>>) -> Result<Vec<String>> {
    let Some(Value::Block(retrieval)) = first(map, "retrieval") else {
        return Ok(Vec::new());
    };
    let Some(Value::Array(values)) = first(retrieval, "tags") else {
        return Ok(Vec::new());
    };
    values
        .iter()
        .map(|value| match value {
            Value::String(tag) => Ok(tag.clone()),
            _ => Err(LmlError::InvalidField {
                field: "retrieval.tags",
                message: "tags must be strings".to_string(),
            }),
        })
        .collect()
}

fn first<'a>(map: &'a BTreeMap<String, Vec<Value>>, key: &str) -> Option<&'a Value> {
    map.get(key).and_then(|values| values.first())
}

fn string_field(map: &BTreeMap<String, Vec<Value>>, key: &'static str) -> Result<String> {
    optional_string_field(map, key).ok_or(LmlError::MissingField(key))
}

fn optional_string_field(map: &BTreeMap<String, Vec<Value>>, key: &str) -> Option<String> {
    match first(map, key) {
        Some(Value::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn optional_number_field(map: &BTreeMap<String, Vec<Value>>, key: &str) -> Option<f64> {
    match first(map, key) {
        Some(Value::Number(value)) => Some(*value),
        _ => None,
    }
}

fn optional_integer_field(
    map: &BTreeMap<String, Vec<Value>>,
    key: &'static str,
) -> Result<Option<i64>> {
    match first(map, key) {
        Some(Value::Number(value)) if value.fract() == 0.0 => Ok(Some(*value as i64)),
        Some(Value::Number(_)) => Err(LmlError::InvalidField {
            field: key,
            message: "expected integer value".to_string(),
        }),
        Some(Value::String(value)) => {
            value
                .parse::<i64>()
                .map(Some)
                .map_err(|err| LmlError::InvalidField {
                    field: key,
                    message: err.to_string(),
                })
        }
        Some(_) => Err(LmlError::InvalidField {
            field: key,
            message: "expected integer value".to_string(),
        }),
        None => Ok(None),
    }
}

fn parse_datetime(value: Option<String>, field: &'static str) -> Result<DateTime<Utc>> {
    let Some(value) = value else {
        return Ok(Utc::now());
    };
    if let Ok(datetime) = DateTime::parse_from_rfc3339(&value) {
        return Ok(datetime.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(&value, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }
    Err(LmlError::InvalidField {
        field,
        message: "expected RFC3339 datetime or YYYY-MM-DD date".to_string(),
    })
}
