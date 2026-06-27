use anchor_lang::prelude::*;

#[constant]
pub const ARENA_ASSET_SEED: &[u8] = b"arena_asset_v1";

#[constant]
pub const ARENA_ITEM_SEED: &[u8] = b"arena_item_v1";

/// Number of built-in skin ids (0..MAX_BUILTIN_SKINS exclusive are valid).
pub const MAX_BUILTIN_SKINS: u8 = 64;

#[constant]
pub const REGISTRY_SEED: &[u8] = b"arena_registry";

#[constant]
pub const STELLAR_LINK_SEED: &[u8] = b"stellar_arena_link";

#[constant]
pub const STELLAR_RELEASE_LINK_SEED: &[u8] = b"stellar_release_link";

pub const RELEASE_DEPLOYMENT_PROJECT_ARENA: &str = "arena";

pub const SOLANA_STELLAR_PROGRAM_ID: Pubkey =
    pubkey!("3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA");

pub const LINK_AVATAR_DATA_DISCRIMINATOR: [u8; 8] =
    [0x64, 0x12, 0x11, 0x63, 0x16, 0x78, 0x8d, 0xfc];
pub const RECORD_RELEASE_DEPLOYMENT_DISCRIMINATOR: [u8; 8] =
    [0xef, 0xb0, 0xe2, 0xe4, 0xf1, 0x01, 0x2a, 0x77];
pub const RELEASE_ACCOUNT_DISCRIMINATOR: [u8; 8] = [0xe5, 0x31, 0x60, 0x94, 0xa7, 0xbc, 0x11, 0x31];
pub const UNIVERSE_ACCOUNT_DISCRIMINATOR: [u8; 8] =
    [0x56, 0x70, 0xe3, 0xe2, 0x58, 0x2f, 0xf2, 0x71];

pub const UNIVERSE_OWNER_OFFSET: usize = 8;

pub const RELEASE_UNIVERSE_OFFSET: usize = 8;
pub const RELEASE_ASSET_OFFSET: usize = 8 + 32;
pub const RELEASE_VAULT_OFFSET: usize = 8 + 32 + 32;
pub const RELEASE_STATUS_OFFSET: usize = 8 + 32 + 32 + 32 + 8 + 32;
pub const RELEASE_STATUS_FINALIZED: u8 = 1;
pub const RELEASE_STATUS_LINKED: u8 = 2;
