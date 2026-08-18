#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod affix;
pub mod constants;
pub mod contexts;
pub mod error;
pub mod fighter;
pub mod handlers;
pub mod state;
pub mod utils;

pub use contexts::*;
pub use error::*;
pub use state::*;

declare_id!("D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ");

#[program]
pub mod solana_ekza_arena {
    use super::*;

    pub fn register_arena_asset(
        ctx: Context<RegisterArenaAsset>,
        args: RegisterArenaAssetArgs,
    ) -> Result<()> {
        handlers::register_arena_asset(ctx, args)
    }

    pub fn register_arena_asset_from_stellar(
        ctx: Context<RegisterArenaAssetFromStellar>,
        args: RegisterArenaAssetFromStellarArgs,
    ) -> Result<()> {
        handlers::register_arena_asset_from_stellar(ctx, args)
    }

    pub fn mint_arena_item(ctx: Context<MintArenaItem>, args: MintArenaItemArgs) -> Result<()> {
        handlers::mint_arena_item(ctx, args)
    }

    pub fn scrap_arena_item(ctx: Context<ScrapArenaItem>) -> Result<()> {
        handlers::scrap_arena_item(ctx)
    }

    /// Set authority-guarded fee destinations and creator/platform/sink split.
    pub fn configure_registry(
        ctx: Context<ConfigureRegistry>,
        args: ConfigureRegistryArgs,
    ) -> Result<()> {
        handlers::configure_registry(ctx, args)
    }

    /// Transfer registry governance. Signed by the current authority.
    pub fn rotate_registry_authority(
        ctx: Context<RotateRegistryAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        handlers::rotate_registry_authority(ctx, new_authority)
    }

    /// Upgrade-authority-gated migration of the legacy, smaller registry PDA.
    pub fn migrate_registry_v1(
        ctx: Context<MigrateRegistryV1>,
        args: ConfigureRegistryArgs,
    ) -> Result<()> {
        handlers::migrate_registry_v1(ctx, args)
    }

    /// Commit-reveal mint — step 1: lock a future slot + distribute the fee.
    pub fn commit_mint(ctx: Context<CommitMint>, args: CommitMintArgs) -> Result<()> {
        handlers::commit_mint(ctx, args)
    }

    /// Commit-reveal mint — step 2: roll from the slot hash + mint (§12.1).
    /// `nonce` re-derives the `MintCommit` PDA.
    pub fn reveal_mint(ctx: Context<RevealMint>, nonce: u64) -> Result<()> {
        handlers::reveal_mint(ctx, nonce)
    }

    /// Commit-reveal fighter mint — consumes the same already-paid
    /// `MintCommit` as `reveal_mint`, but writes a mint-keyed
    /// `ArenaAssetData { card_kind: Avatar }` proof PDA with canonical rolled
    /// combat stats. The exact EKZAF0..3 symbol binds the faction.
    pub fn reveal_avatar_mint(ctx: Context<RevealAvatarMint>, nonce: u64) -> Result<()> {
        handlers::reveal_avatar_mint(ctx, nonce)
    }

    /// Permissionlessly close an expired commit; PDA rent always returns to
    /// the original minter, never to the cleanup caller.
    pub fn close_expired_commit(ctx: Context<CloseExpiredCommit>, nonce: u64) -> Result<()> {
        handlers::close_expired_commit(ctx, nonce)
    }

    /// Create the player's character (one `PlayerAvatar` per wallet).
    pub fn create_player_avatar(
        ctx: Context<CreatePlayerAvatar>,
        args: CreatePlayerAvatarArgs,
    ) -> Result<()> {
        handlers::create_player_avatar(ctx, args)
    }

    /// Create or switch the player's active P3 fighter while proving current
    /// ownership of its canonical 1/1 NFT. Clears both legacy and v2 equipment
    /// atomically so no loadout leaks across fighters.
    pub fn activate_fighter_v2(
        ctx: Context<ActivateFighterV2>,
        args: ActivateFighterV2Args,
    ) -> Result<()> {
        handlers::activate_fighter_v2(ctx, args)
    }

    /// Rename / reskin the character, or swap its base Avatar card.
    pub fn customize_avatar(
        ctx: Context<CustomizeAvatar>,
        args: CustomizeAvatarArgs,
    ) -> Result<()> {
        handlers::customize_avatar(ctx, args)
    }

    /// Equip an owned item NFT into the slot implied by its base type.
    pub fn equip_item(ctx: Context<EquipItem>) -> Result<()> {
        handlers::equip_item(ctx)
    }

    /// Clear one equip slot (`slot` = ArenaBaseType slot index 0..3).
    pub fn unequip_item(ctx: Context<UnequipItem>, slot: u8) -> Result<()> {
        handlers::unequip_item(ctx, slot)
    }

    /// v2 equip ("Lineage tribute"): place an owned item NFT into an explicit
    /// slot (0=Weapon 1=Head 2=Body 3=Gloves 4=Boots 5=Amulet 6=Ring) of the
    /// avatar's `EquipmentRecord` (`["equipment", avatar]`).
    pub fn equip_item_v2(ctx: Context<EquipItemV2>, slot: u8) -> Result<()> {
        handlers::equip_item_v2(ctx, slot)
    }

    /// v2 unequip: clear one `EquipmentRecord` slot (0..6).
    pub fn unequip_item_v2(ctx: Context<UnequipItemV2>, slot: u8) -> Result<()> {
        handlers::unequip_item_v2(ctx, slot)
    }

    /// Buy one consumable EnhanceScroll NFT for `commit_fee_lamports × 2`,
    /// split to the same treasury/sink destinations as `commit_mint`.
    pub fn mint_enhance_scroll(
        ctx: Context<MintEnhanceScroll>,
        args: MintEnhanceScrollArgs,
    ) -> Result<()> {
        handlers::mint_enhance_scroll(ctx, args)
    }

    /// Enhancement — step 1: escrow a scroll AND the item, and lock a future
    /// slot for the roll (same commit-reveal shape as `commit_mint`).
    /// Rejected while the item is equipped.
    pub fn commit_enhance(ctx: Context<CommitEnhance>, nonce: u64) -> Result<()> {
        handlers::commit_enhance(ctx, nonce)
    }

    /// Enhancement — step 2, PERMISSIONLESS: anyone may roll a pending commit
    /// once its slot passed. Success = +1 level and the item returns from
    /// escrow; a risky-zone failure burns the item from escrow. The scroll
    /// burns either way; all rent goes to the commit's owner. `nonce`
    /// re-derives the `EnhanceCommit` PDA.
    pub fn reveal_enhance(ctx: Context<RevealEnhance>, nonce: u64) -> Result<()> {
        handlers::reveal_enhance(ctx, nonce)
    }

    /// Permissionlessly close an expired enhancement commit: the escrowed
    /// item returns to its owner, the scroll is BURNED (abandoning costs the
    /// full ticket), and all rent returns to the committing owner, never the
    /// caller.
    pub fn close_expired_enhance_commit(
        ctx: Context<CloseExpiredEnhanceCommit>,
        nonce: u64,
    ) -> Result<()> {
        handlers::close_expired_enhance_commit(ctx, nonce)
    }
}
