//! # Editorial Content & Text Analysis
//!
//! Word count calculation, 64-bit deterministic content hashing, and locality-sensitive
//! 64-bit SimHash for exact and near-duplicate page detection.
//!
//! ## SEO Content Audit Mechanics
//!
//! Search engines evaluate page quality and indexation suitability using textual content:
//! - **Thin Content**: Pages with low editorial word count (< 200 words) offer minimal value
//!   and risk indexing penalties.
//! - **Exact Duplicate Content**: Pages sharing identical content hashes point to duplicate URL
//!   variants (e.g. parameter tracking, non-canonical HTTP/HTTPS or trailing-slash permutations)
//!   that waste crawl budget.
//! - **Near-Duplicate Content**: Pages with very high textual similarity (> 85%) compete against
//!   each other in search results (keyword cannibalization) and dilute PageRank.
//!
//! ## SimHash & Locality-Sensitive Hashing
//!
//! Traditional cryptographic hashes (like SHA-256) or standard hash tables (like MurmurHash)
//! exhibit an avalanche effect: flipping a single bit in the input changes approximately 50%
//! of the output bits.
//!
//! In contrast, Moses Charikar's **SimHash** is a locality-sensitive hashing (LSH) algorithm
//! where the distance between fingerprints is directly proportional to document similarity:
//!
//! 1. A 64-dimensional weight vector $V = [v_0, v_1, \dots, v_{63}]$ is initialized to zeros.
//! 2. The document text is tokenized into clean, lowercased alphanumeric words.
//! 3. Each token is hashed into a 64-bit integer using deterministic AHash.
//! 4. For each bit $i \in [0, 63]$:
//!    - If bit $i$ of the word hash is $1$, accumulator $v_i$ is incremented by $1$.
//!    - If bit $i$ of the word hash is $0$, accumulator $v_i$ is decremented by $1$.
//! 5. The final 64-bit fingerprint is constructed by setting bit $i = 1$ if $v_i > 0$, and $0$ otherwise.
//!
//! ### Similarity & Hamming Distance
//!
//! The similarity percentage between two documents with SimHash fingerprints $A$ and $B$ is:
//!
//! $$\text{Similarity} = \frac{64 - \text{HammingDistance}(A, B)}{64} \times 100\%$$
//!
//! A Hamming distance of $\le 10$ bits corresponds to $\ge 84.4\% \approx >85\%$ content similarity,
//! flagging near-duplicate content according to technical SEO audit rules.
//!
//! ## Deterministic Hashing Across Runs
//!
//! To ensure that 64-bit hashes stored in SQLite persist accurately across crawler sessions,
//! `AHasher` instances are constructed using fixed 128-bit constant seeds (`CONTENT_HASH_SEEDS`)
//! rather than per-process OS random keys.

use ahash::RandomState;
use std::hash::{BuildHasher, Hasher};

/// Deterministic 128-bit seeds for reproducible content and SimHash values across processes.
const CONTENT_HASH_SEEDS: (u64, u64, u64, u64) = (
    0x2360fc216e128fc9,
    0x8067ff4ea1f92acb,
    0x5444e50e122dd4ee,
    0x61a24bb19b35f7da,
);

/// Constructs an `AHasher` with fixed seeds to ensure deterministic, reproducible hashes.
fn deterministic_hasher() -> ahash::AHasher {
    RandomState::with_seeds(
        CONTENT_HASH_SEEDS.0,
        CONTENT_HASH_SEEDS.1,
        CONTENT_HASH_SEEDS.2,
        CONTENT_HASH_SEEDS.3,
    )
    .build_hasher()
}

/// Counts editorial words in extracted text.
///
/// A word is defined as a whitespace-delimited token containing at least
/// one alphanumeric character. Punctuation-only tokens (e.g. `---`, `...`) are excluded.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::content::count_words;
///
/// assert_eq!(count_words("High-performance technical SEO audit engine."), 5);
/// assert_eq!(count_words("   \t\n  "), 0);
/// assert_eq!(count_words("Symbols --- *** ... ignored"), 2);
/// ```
pub fn count_words(text: &str) -> u32 {
    text.split_whitespace()
        .filter(|token| token.chars().any(|c| c.is_alphanumeric()))
        .count() as u32
}

/// Computes a fast, deterministic 64-bit hash of the cleaned editorial text content.
///
/// Produces identical hashes across process invocations for exact duplicate detection.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::content::compute_content_hash;
///
/// let text_a = "High-performance technical SEO audit engine in Rust.";
/// let text_b = "High-performance technical SEO audit engine in Rust.";
/// let text_c = "Completely different editorial content.";
///
/// assert_eq!(compute_content_hash(text_a), compute_content_hash(text_b));
/// assert_ne!(compute_content_hash(text_a), compute_content_hash(text_c));
/// ```
pub fn compute_content_hash(text: &str) -> u64 {
    let mut hasher = deterministic_hasher();
    hasher.write(text.trim().as_bytes());
    hasher.finish()
}

/// Computes a 64-bit locality-sensitive SimHash fingerprint of the text.
///
/// Documents with nearly identical text produce SimHash values with small Hamming
/// distances ($\le 10$ bits differing), whereas completely unrelated documents
/// differ by approximately 28 to 36 bits.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::content::{compute_simhash, hamming_distance};
///
/// let original = "The quick brown fox jumps over the lazy dog in the sunny park.";
/// let modified = "The quick brown fox jumps over the sleepy dog in the sunny park.";
/// let unrelated = "Quantum computing algorithms utilize qubits and entanglement.";
///
/// let hash_orig = compute_simhash(original);
/// let hash_mod = compute_simhash(modified);
/// let hash_unrel = compute_simhash(unrelated);
///
/// // Near-duplicate documents have small Hamming distance (<= 10 bits difference)
/// assert!(hamming_distance(hash_orig, hash_mod) <= 10);
///
/// // Unrelated documents differ by a large number of bits (> 15 bits difference)
/// assert!(hamming_distance(hash_orig, hash_unrel) > 15);
/// ```
pub fn compute_simhash(text: &str) -> u64 {
    let mut v = [0i32; 64];

    for word in text.split_whitespace() {
        let clean = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if clean.is_empty() {
            continue;
        }

        let mut hasher = deterministic_hasher();
        hasher.write(clean.as_bytes());
        let hash = hasher.finish();

        for (i, val) in v.iter_mut().enumerate() {
            let bit = (hash >> i) & 1;
            if bit == 1 {
                *val += 1;
            } else {
                *val -= 1;
            }
        }
    }

    let mut fingerprint: u64 = 0;
    for (i, &val) in v.iter().enumerate() {
        if val > 0 {
            fingerprint |= 1 << i;
        }
    }

    fingerprint
}

/// Calculates the Hamming distance (number of differing bits) between two 64-bit fingerprints.
///
/// Computed via bitwise XOR followed by population count (`count_ones()`).
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::content::hamming_distance;
///
/// assert_eq!(hamming_distance(0b1010, 0b1010), 0);
/// assert_eq!(hamming_distance(0b1010, 0b1001), 2);
/// assert_eq!(hamming_distance(0, u64::MAX), 64);
/// ```
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_words() {
        assert_eq!(count_words("Hello world! This is Rust."), 5);
        assert_eq!(count_words("   \t \n  "), 0);
        assert_eq!(count_words("--- ... ---"), 0);
        assert_eq!(count_words("SEO-Lens version 1.0"), 3);
    }

    #[test]
    fn test_content_hash_consistency() {
        let text1 = "High-performance technical SEO audit engine.";
        let text2 = "High-performance technical SEO audit engine.";
        let text3 = "Different text completely.";

        assert_eq!(compute_content_hash(text1), compute_content_hash(text2));
        assert_ne!(compute_content_hash(text1), compute_content_hash(text3));
    }

    #[test]
    fn test_simhash_near_duplicates() {
        let doc1 = "The quick brown fox jumps over the lazy dog in the sunny morning park.";
        let doc2 = "The quick brown fox jumps over the sleepy dog in the sunny morning park.";
        let doc3 = "Quantum computing relies on qubits and quantum entanglement principles.";

        let hash1 = compute_simhash(doc1);
        let hash2 = compute_simhash(doc2);
        let hash3 = compute_simhash(doc3);

        let near_dist = hamming_distance(hash1, hash2);
        let far_dist = hamming_distance(hash1, hash3);

        assert!(
            near_dist <= 10,
            "Near duplicate distance should be small (<= 10 representing >85% similarity): {near_dist}"
        );
        assert!(
            far_dist > 15,
            "Unrelated document distance should be large: {far_dist}"
        );
    }
}
