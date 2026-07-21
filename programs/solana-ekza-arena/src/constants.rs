use anchor_lang::prelude::*;

#[constant]
pub const ARENA_ASSET_SEED: &[u8] = b"arena_asset_v1";

#[constant]
pub const ARENA_ITEM_SEED: &[u8] = b"arena_item_v1";

/// Number of built-in skin ids (0..MAX_BUILTIN_SKINS exclusive are valid).
pub const MAX_BUILTIN_SKINS: u8 = 64;

#[constant]
pub const REGISTRY_SEED: &[u8] = b"arena_registry";

/// Project-controlled genesis authority allowed to bootstrap a brand-new
/// registry when the deployed ProgramData authority cannot sign (notably the
/// local validator's system-program sentinel).
///
/// Real deployments should bootstrap with their ProgramData upgrade authority;
/// this key is part of the binary's trust root and MUST be reviewed/replaced
/// before every production deployment. It has no special power after bootstrap:
/// only the authority stored in `ArenaRegistry` can configure or rotate then.
pub const GENESIS_REGISTRY_AUTHORITY: Pubkey =
    pubkey!("Ab5TgPbcB8QVuormXYXHzRVkV7okAbzkS2sU2neKoWvQ");

#[constant]
pub const MINT_COMMIT_SEED: &[u8] = b"mint_commit";

#[constant]
pub const PLAYER_AVATAR_SEED: &[u8] = b"player_avatar_v1";

/// `EquipmentRecord` PDA seed: `["equipment", player_avatar]` — the v2 full
/// equipped set (7 active slots, 16 reserved).
#[constant]
pub const EQUIPMENT_SEED: &[u8] = b"equipment";

/// Slots to wait between `commit_mint` and `reveal_mint` (spec §12.1/§12.5).
/// The target slot's hash is unknown at commit time, killing revert-grinding.
///
/// NOTE: the spec's placeholder default is 1, but §12.5 lists this as an open
/// tunable. We pin it to 5 so the reveal window is observable in tests (with a
/// delay of 1, commit confirmation alone already advances past the target, so
/// the mandatory "reveal-before-target is rejected" case is untestable). 5 slots
/// is ~2s on localnet — still a tiny, sane anti-grind delay.
pub const REVEAL_DELAY_SLOTS: u64 = 5;

/// Deterministic lifetime of a paid mint commitment after its target slot.
///
/// Keeping an explicit bound makes stale commits recoverable without trusting
/// a caller-provided clock or waiting for implementation-specific SlotHashes
/// retention. At normal Solana slot times this leaves roughly 50 seconds after
/// the target slot for the reveal transaction.
#[constant]
pub const COMMIT_REVEAL_WINDOW_SLOTS: u64 = 128;

/// Recommended/default non-refundable commit fee (0.02 SOL). The actual value
/// charged is governed by `ArenaRegistry::commit_fee_lamports`.
#[constant]
pub const COMMIT_FEE_LAMPORTS: u64 = 20_000_000;

/// Fee and royalty basis-point denominator.
#[constant]
pub const BPS_DENOMINATOR: u16 = 10_000;

/// Launch defaults: 50% creator / 40% platform / 10% protocol sink.
#[constant]
pub const DEFAULT_CREATOR_BPS: u16 = 5_000;
#[constant]
pub const DEFAULT_PLATFORM_BPS: u16 = 4_000;
#[constant]
pub const DEFAULT_SINK_BPS: u16 = 1_000;

/// Metaplex legacy royalty advertised by every Arena item NFT. This is the
/// immediately-compatible royalty signal; marketplaces may still choose not
/// to honor legacy royalties.
#[constant]
pub const ITEM_ROYALTY_BPS: u16 = 500;

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
