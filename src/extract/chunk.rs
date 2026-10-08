//! Heading-based chunking of page documents into passages for `find` and full-text search.

use crate::extract::tokens::estimate_tokens;
use crate::extract::types::{Block, BlockKind};

/// Default chunk size target in tokens.
pub const DEFAULT_CHUNK_TOKENS: usize = 350;

/// A passage of a page: consecutive blocks under one heading.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Plain text of the passage (heading line first when the chunk starts at a heading).
    pub text: String,
    /// Headings the passage sits under, outermost first, including its own heading.
    pub heading_path: Vec<String>,
    /// CSS selector of the first content block.
    pub selector: String,
    /// Estimated token count.
    pub tokens: usize,
    /// Index of the first block in the document.
    pub first_block: usize,
    /// Number of blocks in the chunk.
    pub block_count: usize,
}

/// Splits blocks into chunks. A heading always starts a new chunk; a chunk grows until adding
/// the next block would exceed `target_tokens`. Blocks are never split, so tables, lists and
/// code blocks stay whole even when they alone exceed the target.
pub fn chunk_blocks(blocks: &[Block], target_tokens: usize) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current: Option<Chunk> = None;

    for (index, block) in blocks.iter().enumerate() {
        let tokens = estimate_tokens(&block.text);
        let is_heading = matches!(block.kind, BlockKind::Heading { .. });

        let flush = match current.as_ref() {
            Some(chunk) => {
                is_heading || (chunk.tokens + tokens > target_tokens && chunk.tokens > 0)
            }
            None => false,
        };
        if flush {
            if let Some(done) = current.take() {
                chunks.push(done);
            }
        }

        let chunk = current.get_or_insert_with(|| {
            let mut heading_path = block.heading_path.clone();
            if is_heading {
                heading_path.push(block.text.clone());
            }
            Chunk {
                text: String::new(),
                heading_path,
                selector: block.selector.clone(),
                tokens: 0,
                first_block: index,
                block_count: 0,
            }
        });
        if !chunk.text.is_empty() {
            chunk.text.push('\n');
        }
        chunk.text.push_str(&block.text);
        chunk.tokens += tokens;
        chunk.block_count += 1;
        if chunk.block_count == 2
            && matches!(blocks[chunk.first_block].kind, BlockKind::Heading { .. })
        {
            chunk.selector = block.selector.clone();
        }
    }
    if let Some(done) = current {
        chunks.push(done);
    }
    chunks.retain(|c| !c.text.trim().is_empty());
    chunks
}
