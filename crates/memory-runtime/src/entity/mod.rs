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

/// Core normalization shared by both canonical forms: lowercase + separator
/// normalization + underscore collapse + trailing trim. Input is assumed already
/// NFKC-normalized (and, for the aggressive form, pascal-stripped).
fn core_normalize(s: &str) -> String {
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
    result.trim_end_matches('_').to_string()
}

fn nfkc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfkc().collect()
}

/// Light entity key: case + separator normalization only. Does NOT strip semantic
/// suffixes, so "UserService" stays distinct from "UserModel" while "UserModel",
/// "user_model" and "User Model" all collapse to "user_model".
pub fn canonical_key_light(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return s.to_string();
    }
    core_normalize(&nfkc(s))
}

/// Aggressive canonical key: also strips semantic suffixes ("UserModel" -> "user",
/// "user_service" -> "user"). Used for alias matching / clustering where different
/// surface forms of the same concept must collapse.
pub fn canonical_key(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return s.to_string();
    }

    // Unicode NFKC normalization
    let s = nfkc(s);

    // Strip PascalCase suffixes BEFORE lowercasing (e.g., UserModel -> User)
    let s = strip_pascal_suffixes(&s);

    let s = core_normalize(&s);

    // Strip underscore-separated suffixes (e.g., user_model -> user)
    strip_underscore_suffixes(&s)
}

fn strip_pascal_suffixes(s: &str) -> String {
    for suffix in PASCAL_SUFFIXES {
        if let Some(remaining) = s.strip_suffix(suffix) {
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
            if let Some(remaining) = result.strip_suffix(suffix) {
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
        if let Some(remaining) = result.strip_suffix(suffix) {
            if !remaining.is_empty() {
                return remaining.to_string();
            }
        }
    }
    result
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

    #[test]
    fn canonical_key_light_normalizes_without_stripping_suffixes() {
        // Case + separator normalization collapses separator spellings ...
        assert_eq!(canonical_key_light("User Model"), "user_model");
        assert_eq!(canonical_key_light("user-model"), "user_model");
        assert_eq!(canonical_key_light("user_model"), "user_model");
        assert_eq!(canonical_key_light("POSMASK"), "posmask");
        // ... but semantic suffixes are preserved, so distinct entities stay apart
        // (unlike aggressive canonical_key which collapses both to "user").
        assert_eq!(canonical_key_light("UserService"), "userservice");
        assert_ne!(
            canonical_key_light("UserService"),
            canonical_key_light("UserModel")
        );
        // Chinese is preserved (lowercase is a no-op for CJK).
        assert_eq!(canonical_key_light("机器字段"), "机器字段");
    }
}
