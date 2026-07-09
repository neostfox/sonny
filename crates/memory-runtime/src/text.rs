//! Shared text-matching helpers used by keyword classifiers (feedback and
//! recall intent) and CJK-aware query tokenization.

/// Word character for keyword-boundary purposes. `_` and `-` are treated as
/// word characters (not delimiters) so identifier-like tokens don't split into
/// keywords: without this, "no" would match inside "no_cache"/"no-op" because
/// `_`/`-` are not alphanumeric. Non-ASCII (incl. CJK) is word-like.
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Boundary-aware keyword test on already-lowercased text.
///
/// ASCII keywords and single-character CJK keywords match only with non-word
/// neighbors on both sides, so "know" is not negated via `no` and "针对" does
/// not confirm via `对`. Multi-character CJK keywords stay substring — Chinese
/// has no delimiter to anchor a word boundary on ("还有一个" must still hit
/// "还有").
pub(crate) fn keyword_hit(text: &str, keyword: &str) -> bool {
    let needs_boundary = keyword.is_ascii() || keyword.chars().count() == 1;
    if !needs_boundary {
        return text.contains(keyword);
    }
    text.match_indices(keyword).any(|(start, _)| {
        let before_is_word = text[..start].chars().next_back().is_some_and(is_word_char);
        let after_is_word = text[start + keyword.len()..]
            .chars()
            .next()
            .is_some_and(is_word_char);
        !before_is_word && !after_is_word
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_keywords_require_word_boundaries() {
        assert!(keyword_hit("no, that's it", "no"));
        assert!(!keyword_hit("i don't know", "no")); // no ⊄ know
        assert!(!keyword_hit("set no_cache", "no")); // _ is a word char
        assert!(!keyword_hit("a no-op", "no")); // - is a word char
    }

    #[test]
    fn single_char_cjk_requires_boundary_but_multichar_is_substring() {
        assert!(keyword_hit("对，就是这个", "对"));
        assert!(!keyword_hit("针对这个", "对")); // 针 is word-like
        assert!(keyword_hit("还有一个字段", "还有")); // multi-char CJK: substring
    }
}
