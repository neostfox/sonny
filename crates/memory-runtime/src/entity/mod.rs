use rusqlite::Connection;

use crate::error::MemoryResult;

const STRIPPABLE_SUFFIXES: &[&str] = &[
    "_table", "_tbl", "_entity", "_model", "_class", "_type", "_info", "_data", "_config", "_obj",
    "_mgr", "_manager", "_service", "_handler",
];

const BARE_SUFFIXES: &[&str] = &[
    "service", "handler", "manager", "config", "entity", "model", "class", "table", "type", "info",
    "data", "object", "impl",
];

const PASCAL_SUFFIXES: &[&str] = &[
    "Impl", "Class", "Type", "Model", "Table", "Entity", "Info", "Data", "Object", "Service",
    "Manager", "Handler", "Config",
];

pub fn canonical_key(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return s.to_string();
    }

    // Unicode NFKC normalization
    use unicode_normalization::UnicodeNormalization;
    let s: String = s.nfkc().collect();

    // Strip PascalCase suffixes BEFORE lowercasing (e.g., UserModel -> User)
    let s = strip_pascal_suffixes(&s);

    // Lowercase (Unicode-aware)
    let s = s.to_lowercase();

    // Normalize separators: spaces and hyphens -> underscores
    let s: String = s
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();

    // Collapse repeated underscores
    let mut result = String::with_capacity(s.len());
    let mut prev_underscore = false;
    for c in s.chars() {
        if c == '_' {
            if !prev_underscore {
                result.push(c);
            }
            prev_underscore = true;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }

    // Strip trailing underscores
    let result = result.trim_end_matches('_').to_string();

    // Strip underscore-separated suffixes (e.g., user_model -> user)
    strip_underscore_suffixes(&result)
}

fn strip_pascal_suffixes(s: &str) -> String {
    for suffix in PASCAL_SUFFIXES {
        if s.ends_with(suffix) {
            let remaining = &s[..s.len() - suffix.len()];
            if !remaining.is_empty() {
                return remaining.to_string();
            }
        }
    }
    s.to_string()
}

fn strip_underscore_suffixes(s: &str) -> String {
    let mut result = s.to_string();
    // Iteratively strip underscore-separated suffixes
    loop {
        let mut stripped = false;
        for suffix in STRIPPABLE_SUFFIXES {
            if result.ends_with(suffix) {
                let remaining = &result[..result.len() - suffix.len()];
                if !remaining.is_empty() && !remaining.ends_with('_') {
                    result = remaining.trim_end_matches('_').to_string();
                    stripped = true;
                    break;
                }
            }
        }
        if !stripped {
            break;
        }
    }
    // Also strip bare suffixes (e.g., "authservice" -> "auth")
    for suffix in BARE_SUFFIXES {
        if result.ends_with(suffix) {
            let remaining = &result[..result.len() - suffix.len()];
            if !remaining.is_empty() {
                return remaining.to_string();
            }
        }
    }
    result
}

pub struct EntityNormalizer<'a> {
    conn: &'a Connection,
}

impl<'a> EntityNormalizer<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn resolve(&self, raw_entity: &str, workspace_id: &str) -> MemoryResult<String> {
        let key = canonical_key(raw_entity);

        let result: Option<String> = self.conn.query_row(
            "SELECT canonical_form FROM entity_alias WHERE alias_form = ?1 AND (workspace_id IS NULL OR workspace_id = ?2) LIMIT 1",
            rusqlite::params![key, workspace_id],
            |r| r.get(0),
        ).ok();

        if let Some(canonical) = result {
            return Ok(canonical);
        }

        let result: Option<String> = self.conn.query_row(
            "SELECT canonical_form FROM entity_alias WHERE canonical_form = ?1 AND (workspace_id IS NULL OR workspace_id = ?2) LIMIT 1",
            rusqlite::params![key, workspace_id],
            |r| r.get(0),
        ).ok();

        if let Some(canonical) = result {
            return Ok(canonical);
        }

        Ok(key)
    }

    pub fn add_alias(&self, canonical: &str, alias: &str, workspace_id: &str) -> MemoryResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO entity_alias (canonical_form, alias_form, workspace_id, confirmed, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![canonical, alias, workspace_id, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_key_basic() {
        assert_eq!(canonical_key("User Model"), "user");
        assert_eq!(canonical_key("user_model"), "user");
        assert_eq!(canonical_key("UserModel"), "user");
        assert_eq!(canonical_key("user-model"), "user");
    }

    #[test]
    fn canonical_key_preserves_chinese() {
        assert_eq!(canonical_key("MASK 表"), "mask_表");
        assert_eq!(canonical_key("用户模型"), "用户模型");
    }

    #[test]
    fn canonical_key_mixed_language() {
        assert_eq!(canonical_key("BOE 内网 DNS"), "boe_内网_dns");
    }

    #[test]
    fn canonical_key_suffix_stripping() {
        assert_eq!(canonical_key("UserModel_table"), "user");
        assert_eq!(canonical_key("AuthService_handler"), "auth");
        assert_eq!(canonical_key("UserConfig"), "user");
    }

    #[test]
    fn canonical_key_no_over_strip() {
        assert_eq!(canonical_key("Model"), "model");
        assert_eq!(canonical_key("table"), "table");
    }

    #[test]
    fn canonical_key_collapses_underscores() {
        assert_eq!(canonical_key("user__model"), "user");
    }

    #[test]
    fn canonical_key_trims_whitespace() {
        assert_eq!(canonical_key("  user model  "), "user");
    }

    #[test]
    fn canonical_key_empty() {
        assert_eq!(canonical_key(""), "");
        assert_eq!(canonical_key("  "), "");
    }
}
