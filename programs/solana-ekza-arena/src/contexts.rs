use anchor_lang::prelude::*;

use crate::{
    constants::{
        ARENA_ASSET_SEED, ARENA_ITEM_SEED, REGISTRY_SEED, STELLAR_LINK_SEED,
        STELLAR_RELEASE_LINK_SEED,
    },
    error::ArenaRegistryError,
    state::{
        ArenaAssetData, ArenaItem, ArenaRegistry, MintArenaItemArgs, RegisterArenaAssetArgs,
        RegisterArenaAssetFromStellarArgs, StellarArenaAssetLink, StellarReleaseLink,
    },
};

#[derive(Accounts)]
#[instruction(args: RegisterArenaAssetArgs)]
pub struct RegisterArenaAsset<'info> {
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + ArenaRegistry::INIT_SPACE,
        seeds = [REGISTRY_SEED],
        bump
    )]
    pub registry: Account<'info, ArenaRegistry>,

    #[account(
        init,
        payer = payer,
        space = 8 + ArenaAssetData::INIT_SPACE,
        seeds = [
            ARENA_ASSET_SEED,
            &registry.next_index.to_le_bytes()
        ],
        bump
    )]
    pub arena_asset: Account<'info, ArenaAssetData>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: RegisterArenaAssetFromStellarArgs)]
pub struct RegisterArenaAssetFromStellar<'info> {
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + ArenaRegistry::INIT_SPACE,
        seeds = [REGISTRY_SEED],
        bump
    )]
    pub registry: Account<'info, ArenaRegistry>,

    #[account(
        init,
        payer = payer,
        space = 8 + ArenaAssetData::INIT_SPACE,
        seeds = [
            ARENA_ASSET_SEED,
            &registry.next_index.to_le_bytes()
        ],
        bump
    )]
    pub arena_asset: Account<'info, ArenaAssetData>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + StellarArenaAssetLink::INIT_SPACE,
        seeds = [STELLAR_LINK_SEED, arena_asset.key().as_ref()],
        bump
    )]
    pub stellar_link: Account<'info, StellarArenaAssetLink>,

    /// CHECK: Validated by owner, executable bit, and fixed program id.
    pub stellar_program: AccountInfo<'info>,
    /// CHECK: Validated by owner, discriminator, and stored release universe.
    pub stellar_universe: AccountInfo<'info>,
    /// CHECK: Validated as a solana-stellar Release account by fixed-layout fields.
    #[account(mut)]
    pub stellar_release: AccountInfo<'info>,
    /// CHECK: Validated against the vault stored in the Stellar release account.
    pub stellar_vault: AccountInfo<'info>,
    /// CHECK: Created or updated by solana-stellar record_release_deployment CPI.
    #[account(mut)]
    pub stellar_release_deployment: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + StellarReleaseLink::INIT_SPACE,
        seeds = [STELLAR_RELEASE_LINK_SEED, stellar_release.key().as_ref()],
        bump
    )]
    pub stellar_release_link: Account<'info, StellarReleaseLink>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: MintArenaItemArgs)]
pub struct MintArenaItem<'info> {
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + ArenaRegistry::INIT_SPACE,
        seeds = [REGISTRY_SEED],
        bump
    )]
    pub registry: Account<'info, ArenaRegistry>,

    #[account(
        init,
        payer = payer,
        space = 8 + ArenaItem::INIT_SPACE,
        seeds = [
            ARENA_ITEM_SEED,
            &registry.next_index.to_le_bytes()
        ],
        bump
    )]
    pub arena_item: Account<'info, ArenaItem>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: Address-constrained to the SlotHashes sysvar; read as raw data.
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::ID)]
    pub slot_hashes: AccountInfo<'info>,

    // Optional Stellar accounts — required only when `skin == Stellar`.
    /// CHECK: Validated by owner, executable bit, and fixed program id.
    pub stellar_program: Option<AccountInfo<'info>>,
    /// CHECK: Validated as a solana-stellar Release account by fixed-layout fields.
    pub stellar_release: Option<AccountInfo<'info>>,
    /// CHECK: Validated against the vault stored in the Stellar release account.
    pub stellar_vault: Option<AccountInfo<'info>>,

    pub system_program: Program<'info, System>,
}

/// Scrap (close) a rolled `ArenaItem` — the v2 economic SINK (spec §10.5).
///
/// `close = minter` returns the account's rent lamports to the owner. The
/// `has_one = minter` guard ties the stored `ArenaItem.minter` to the signer,
/// so ONLY the item owner may scrap it (anyone else => Unauthorized).
#[derive(Accounts)]
pub struct ScrapArenaItem<'info> {
    #[account(
        mut,
        close = minter,
        has_one = minter @ ArenaRegistryError::Unauthorized,
    )]
    pub arena_item: Account<'info, ArenaItem>,

    #[account(mut)]
    pub minter: Signer<'info>,
}
