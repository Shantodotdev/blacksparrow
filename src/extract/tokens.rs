//! Token counting (o200k_base via `tiktoken-rs`) and token-budget trimming.

use crate::extract::types::{Block, BlockKind};

/// Counts tokens with the o200k_base encoding. An estimate for non-OpenAI models, close enough
/// for budgeting context windows.
pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
}

/// Cheap estimate (~4 bytes per token) for sizing chunks where exact counts are unnecessary.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Trims blocks to fit `max_tokens`. Every heading is kept so the page outline survives; body
/// blocks are dropped starting from the longest sections at the bottom of the page.
/// Returns `true` when anything was removed.
pub fn trim_blocks_to_budget(blocks: &mut Vec<Block>, max_tokens: usize) -> bool {
    let costs: Vec<usize> = blocks
        .iter()
        .map(|b| count_tokens(&b.markdown) + 1)
        .collect();
    let mut total: usize = costs.iter().sum();
    if total <= max_tokens {
        return false;
    }

    // Section index of each block: increments at every heading.
    let mut section_of = Vec::with_capacity(blocks.len());
    let mut section = 0usize;
    for block in blocks.iter() {
        if matches!(block.kind, BlockKind::Heading { .. }) {
            section += 1;
        }
        section_of.push(section);
    }
    let mut section_sizes = vec![0usize; section + 1];
    for (i, block) in blocks.iter().enumerate() {
        if !matches!(block.kind, BlockKind::Heading { .. }) {
            section_sizes[section_of[i]] += costs[i];
        }
    }

    let mut keep = vec![true; blocks.len()];
    // Visit body blocks from the bottom, longest sections first.
    let mut order: Vec<usize> = (0..blocks.len())
        .filter(|&i| !matches!(blocks[i].kind, BlockKind::Heading { .. }))
        .collect();
    order.sort_by(|&a, &b| {
        section_sizes[section_of[b]]
            .cmp(&section_sizes[section_of[a]])
            .then(b.cmp(&a))
    });
    for i in order {
        if total <= max_tokens {
            break;
        }
        keep[i] = false;
        total = total.saturating_sub(costs[i]);
    }

    let mut index = 0;
    blocks.retain(|_| {
        let k = keep[index];
        index += 1;
        k
    });
    true
}
