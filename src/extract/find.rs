//! `find`: rank passages of a page against a query (BM25), or match CSS selectors and regexes.

/// Common English words ignored when building search queries.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "how",
    "i", "if", "in", "is", "it", "its", "me", "my", "of", "on", "or", "our", "so", "than", "that",
    "the", "their", "there", "this", "to", "was", "we", "what", "when", "where", "which", "who",
    "why", "will", "with", "you", "your",
];

/// Lowercase alphanumeric terms of a query. With `drop_stopwords`, common words are removed
/// (unless that would leave nothing).
pub fn query_terms(query: &str, drop_stopwords: bool) -> Vec<String> {
    let all: Vec<String> = tokenize(query);
    if !drop_stopwords {
        return all;
    }
    let kept: Vec<String> = all
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if kept.is_empty() {
        all
    } else {
        kept
    }
}

/// Splits text into lowercase alphanumeric tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}
