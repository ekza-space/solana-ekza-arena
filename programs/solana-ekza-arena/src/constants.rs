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
pub const MINT_COMMIT_SEED: &[u8] = b"mint_commit";

#[constant]
pub const PLAYER_AVATAR_SEED: &[u8] = b"player_avatar_v1";

/// Slots to wait between `commit_mint` and `reveal_mint` (spec §12.1/§12.5).
/// The target slot's hash is unknown at commit time, killing revert-grinding.
///
/// NOTE: the spec's placeholder default is 1, but §12.5 lists this as an open
/// tunable. We pin it to 5 so the reveal window is observable in tests (with a
/// delay of 1, commit confirmation alone already advances past the target, so
/// the mandatory "reveal-before-target is rejected" case is untestable). 5 slots
/// is ~2s on localnet — still a tiny, sane anti-grind delay.
pub const REVEAL_DELAY_SLOTS: u64 = 5;

/// Default non-refundable commit fee (0.01 SOL, spec §12.1). The on-chain value
/// actually charged is `ArenaRegistry::commit_fee_lamports` set at configure.
pub const COMMIT_FEE_LAMPORTS: u64 = 10_000_000;

#[constant]
pub const STELLAR_LINK_SEED: &[u8] = b"stellar_arena_link";

#[constant]
pub const STELLAR_RELEASE_LINK_SEED: &[u8] = b"stellar_release_link";

/// Project slug the Arena writes into the Stellar `ReleaseDeployment` record
/// (gate contract: every consumer app records its own slug).
pub const RELEASE_DEPLOYMENT_PROJECT_ARENA: &str = "arena";

// NOTE: all solana-stellar layout knowledge (program id, account structs,
// statuses, CPI discriminators) now comes from the `solana-stellar` crate
// dependency (`solana_stellar::ID`, `state::*`, `cpi::*`) — never hand-code
// offsets or discriminators here again.
