#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod affix;
pub mod constants;
pub mod contexts;
pub mod error;
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

    pub fn mint_arena_item(
        ctx: Context<MintArenaItem>,
        args: MintArenaItemArgs,
    ) -> Result<()> {
        handlers::mint_arena_item(ctx, args)
    }

    pub fn scrap_arena_item(ctx: Context<ScrapArenaItem>) -> Result<()> {
        handlers::scrap_arena_item(ctx)
    }

    /// Set the treasury + commit fee for the commit-reveal mint (spec §12.1).
    pub fn configure_registry(
        ctx: Context<ConfigureRegistry>,
        args: ConfigureRegistryArgs,
    ) -> Result<()> {
        handlers::configure_registry(ctx, args)
    }

    /// Commit-reveal mint — step 1: lock a future slot + charge the fee (§12.1).
    pub fn commit_mint(ctx: Context<CommitMint>, args: CommitMintArgs) -> Result<()> {
        handlers::commit_mint(ctx, args)
    }

    /// Commit-reveal mint — step 2: roll from the slot hash + mint (§12.1).
    /// `nonce` re-derives the `MintCommit` PDA.
    pub fn reveal_mint(ctx: Context<RevealMint>, nonce: u64) -> Result<()> {
        handlers::reveal_mint(ctx, nonce)
    }

    /// Create the player's character (one `PlayerAvatar` per wallet).
    pub fn create_player_avatar(
        ctx: Context<CreatePlayerAvatar>,
        args: CreatePlayerAvatarArgs,
    ) -> Result<()> {
        handlers::create_player_avatar(ctx, args)
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
}
