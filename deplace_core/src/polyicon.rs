//! Identicons are fallbacks for if someone has no custom icon set.
//!
//! This crate creates identicons based on a seed with the following parametrs:
//! - number in [0, 3, 4, 5, 6] for the outline shape
//! - number in [0, 3, 4, 5, 6] for the inner shape
//! - number in [0, 1, 2] for the amount of dots in the inner shape

use sha2::{Digest, Sha256};

const OUTLINE_NUMS: [u8; 5] = [0, 4, 5, 6, 8];
const INNER_NUMS: [u8; 5] = [0, 4, 5, 6, 8];
const DOT_COUNTS: [u8; 3] = [0, 1, 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PolyIcon {
    pub outline_shape: u8,
    pub inner_shape: u8,
    pub dot_count: u8,
}

impl From<&str> for PolyIcon {
    fn from(value: &str) -> Self {
        let hash = Sha256::digest(value.as_bytes());

        Self {
            outline_shape: OUTLINE_NUMS[hash[0] as usize % OUTLINE_NUMS.len()],
            inner_shape: INNER_NUMS[hash[1] as usize % INNER_NUMS.len()],
            dot_count: DOT_COUNTS[hash[2] as usize % DOT_COUNTS.len()],
        }
    }
}
