use regex::Regex;

use crate::error::{MemoryError, MemoryResult};
use crate::models::raw_memory::{RawMemory, SourceType};

pub trait SessionParser: Send + Sync {
    fn parse(
        &self,
        content: &str,
        workspace_id: &str,
        source_ref: &str,
    ) -> MemoryResult<Vec<RawMemory>>;
    fn can_parse(&self, content: &str, filename: &str) -> bool;
}

pub struct JournalParser;

impl SessionParser for JournalParser {
    fn parse(
        &self,
        content: &str,
        workspace_id: &str,
        source_ref: &str,
    ) -> MemoryResult<Vec<RawMemory>> {
        let mut memories = Vec::new();
        let mut current_session_id = String::new();
        let mut current_date = String::new();
        let mut current_section = String::new();
        let mut content_buffer = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Session header: ## Session N: Title
            if trimmed.starts_with("## Session ") {
                // Flush previous section
                flush_section(
                    &mut memories,
                    &current_session_id,
                    &current_date,
                    &current_section,
                    &mut content_buffer,
                    workspace_id,
                    source_ref,
                );

                // Extract session info
                let header = trimmed.trim_start_matches('#').trim();
                current_session_id =
                    extract_session_id(header).unwrap_or_else(|| header.to_string());
                current_date.clear();
                current_section = "header".to_string();
                content_buffer = format!("{}\n", header);
                continue;
            }

            // Sub-heading: ### Summary, ### Main Changes, etc.
            if trimmed.starts_with("### ") {
                flush_section(
                    &mut memories,
                    &current_session_id,
                    &current_date,
                    &current_section,
                    &mut content_buffer,
                    workspace_id,
                    source_ref,
                );
                current_section = trimmed.trim_start_matches('#').trim().to_lowercase();
                continue;
            }

            // Metadata: **Key**: Value
            if let Some(caps) = KEY_VALUE_REGEX.captures(trimmed) {
                let key = caps.get(1).unwrap().as_str();
                let value = caps.get(2).unwrap().as_str().trim().to_string();
                if key == "Date" {
                    current_date = value;
                }
            }

            // Horizontal rule: --- (session separator)
            if trimmed == "---" {
                flush_section(
                    &mut memories,
                    &current_session_id,
                    &current_date,
                    &current_section,
                    &mut content_buffer,
                    workspace_id,
                    source_ref,
                );
                continue;
            }

            // Accumulate content
            if !current_session_id.is_empty() {
                content_buffer.push_str(line);
                content_buffer.push('\n');
            }
        }

        // Flush final section
        flush_section(
            &mut memories,
            &current_session_id,
            &current_date,
            &current_section,
            &mut content_buffer,
            workspace_id,
            source_ref,
        );

        Ok(memories)
    }

    fn can_parse(&self, content: &str, _filename: &str) -> bool {
        content.starts_with("# Journal")
    }
}

/// Push the buffered section as a `RawMemory` if it is non-empty and a session is active.
fn flush_section(
    memories: &mut Vec<RawMemory>,
    session_id: &str,
    date: &str,
    section: &str,
    buffer: &mut String,
    workspace_id: &str,
    source_ref: &str,
) {
    if buffer.trim().is_empty() || session_id.is_empty() {
        return;
    }
    memories.push(make_raw_memory(
        session_id,
        date,
        section,
        buffer,
        workspace_id,
        source_ref,
    ));
    buffer.clear();
}

pub struct JsonSessionParser;

impl SessionParser for JsonSessionParser {
    fn parse(
        &self,
        content: &str,
        workspace_id: &str,
        source_ref: &str,
    ) -> MemoryResult<Vec<RawMemory>> {
        let session: JsonSession =
            serde_json::from_str(content).map_err(|e| MemoryError::SessionParse {
                details: format!("Invalid JSON session: {e}"),
            })?;

        let mut memories = Vec::new();
        for msg in &session.messages {
            memories.push(RawMemory {
                memory_id: uuid::Uuid::new_v4().to_string(),
                workspace_id: workspace_id.to_string(),
                session_id: session.session_id.clone(),
                role: msg.role.clone(),
                content: msg.content.clone(),
                source_type: SourceType::SessionFile,
                source_ref: source_ref.to_string(),
                extraction_version: None,
                created_at: session
                    .date
                    .clone()
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            });
        }
        Ok(memories)
    }

    fn can_parse(&self, content: &str, _filename: &str) -> bool {
        content.trim_start().starts_with('{') && content.contains("\"session_id\"")
    }
}

pub fn detect_and_parse(
    content: &str,
    filename: &str,
    workspace_id: &str,
    source_ref: &str,
) -> MemoryResult<Vec<RawMemory>> {
    let parsers: Vec<Box<dyn SessionParser>> =
        vec![Box::new(JournalParser), Box::new(JsonSessionParser)];

    for parser in &parsers {
        if parser.can_parse(content, filename) {
            return parser.parse(content, workspace_id, source_ref);
        }
    }

    Err(MemoryError::SessionParse {
        details: "Unknown session format".to_string(),
    })
}

fn make_raw_memory(
    session_id: &str,
    date: &str,
    section: &str,
    content: &str,
    workspace_id: &str,
    source_ref: &str,
) -> RawMemory {
    RawMemory {
        memory_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        session_id: session_id.to_string(),
        role: section.to_string(),
        content: content.trim().to_string(),
        source_type: SourceType::Journal,
        source_ref: source_ref.to_string(),
        extraction_version: None,
        created_at: if date.is_empty() {
            chrono::Utc::now().to_rfc3339()
        } else {
            date.to_string()
        },
    }
}

fn extract_session_id(header: &str) -> Option<String> {
    let caps = SESSION_ID_REGEX.captures(header)?;
    Some(format!("session_{}", caps.get(1)?.as_str()))
}

#[derive(serde::Deserialize)]
struct JsonSession {
    session_id: String,
    date: Option<String>,
    messages: Vec<JsonMessage>,
}

#[derive(serde::Deserialize)]
struct JsonMessage {
    role: String,
    content: String,
}

use std::sync::LazyLock;

static KEY_VALUE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*(\w+)\*\*:\s*(.+)$").unwrap());
static SESSION_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^session\s+(\d+)").unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_session_parse() {
        let json = r#"{
            "session_id": "test_session_1",
            "date": "2026-05-16",
            "messages": [
                {"role": "user", "content": "POSMASK 表没有机器字段"},
                {"role": "assistant", "content": "了解，我来检查一下。"}
            ]
        }"#;
        let result = JsonSessionParser.parse(json, "ws1", "test.json").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].content, "POSMASK 表没有机器字段");
        assert_eq!(result[0].session_id, "test_session_1");
        assert_eq!(result[0].source_type, SourceType::SessionFile);
    }

    #[test]
    fn journal_parse() {
        let journal = r#"# Journal - test (Part 1)

> AI development session journal

---

## Session 1: Test Session

**Date**: 2026-05-12
**Task**: Test task

### Summary

This is a test summary.

### Main Changes

- Changed something
- Fixed something else

---

## Session 2: Another Session

**Date**: 2026-05-16
**Task**: Another task

### Summary

Another summary.
"#;
        let result = JournalParser.parse(journal, "ws1", "journal.md").unwrap();
        assert!(
            result.len() >= 2,
            "Should have at least 2 sections from session 1"
        );
        assert!(result.iter().any(|m| m.content.contains("Test Session")));
        assert!(result.iter().any(|m| m.content.contains("test summary")));
    }

    #[test]
    fn detect_json_format() {
        let json = r#"{"session_id": "x", "messages": []}"#;
        assert!(JsonSessionParser.can_parse(json, "test.json"));
        assert!(!JournalParser.can_parse(json, "test.json"));
    }

    #[test]
    fn detect_journal_format() {
        let journal = "# Journal - test";
        assert!(JournalParser.can_parse(journal, "journal.md"));
        assert!(!JsonSessionParser.can_parse(journal, "journal.md"));
    }
}
