use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::Metadata,
    token::{Mint, Token, TokenAccount},
};

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

/// Mint a rolled Arena item as a REAL tradeable Metaplex NFT (spec §11).
///
/// The `arena_item` PDA is re-seeded by the **NFT mint pubkey** (1:1 mint↔item)
/// and holds the immutable rolled stats — the game's source of truth. The SPL
/// mint (supply 1, decimals 0) + Master Edition (max supply 0) make it a true
/// non-fungible that any Solana marketplace can trade.
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

    /// The new NFT mint (1 token, 0 decimals). Mint+freeze authority start on the
    /// payer; the Master Edition CPI then takes them over.
    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = payer,
        mint::freeze_authority = payer
    )]
    pub mint: Account<'info, Mint>,

    /// Game stats PDA, seeded by the NFT mint (spec §11.2).
    #[account(
        init,
        payer = payer,
        space = 8 + ArenaItem::INIT_SPACE,
        seeds = [ARENA_ITEM_SEED, mint.key().as_ref()],
        bump
    )]
    pub arena_item: Account<'info, ArenaItem>,

    /// Minter's associated token account — receives the single NFT token.
    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = payer
    )]
    pub minter_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: Address-constrained to the SlotHashes sysvar; read as raw data.
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::ID)]
    pub slot_hashes: AccountInfo<'info>,

    /// CHECK: Metaplex metadata account PDA for this mint (created via CPI).
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    pub metadata_account: UncheckedAccount<'info>,

    /// CHECK: Metaplex master edition PDA for this mint (created via CPI).
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), mint.key().as_ref(), b"edition"],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    pub master_edition: UncheckedAccount<'info>,

    // Optional Stellar accounts — required only when `skin == Stellar`.
    /// CHECK: Validated by owner, executable bit, and fixed program id.
    pub stellar_program: Option<AccountInfo<'info>>,
    /// CHECK: Validated as a solana-stellar Release account by fixed-layout fields.
    pub stellar_release: Option<AccountInfo<'info>>,
    /// CHECK: Validated against the vault stored in the Stellar release account.
    pub stellar_vault: Option<AccountInfo<'info>>,

    pub token_metadata_program: Program<'info, Metadata>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

/// Scrap a rolled item — the economic SINK (spec §10.5/§11.3). BURNS the NFT
/// (token + metadata + master edition via Metaplex `BurnNft`) AND closes the
/// `ArenaItem` PDA, returning all rent to the current owner.
///
/// Ownership is the **NFT token holder** (spec §11.2), not `minter`: the signer
/// must hold the single token in `token_account`. `BurnNft` enforces this (it
/// fails unless `owner` signs and owns the token); the `amount == 1` constraint
/// gives a clean error first.
#[derive(Accounts)]
pub struct ScrapArenaItem<'info> {
    #[account(
        mut,
        close = owner,
        has_one = mint,
        seeds = [ARENA_ITEM_SEED, mint.key().as_ref()],
        bump = arena_item.bump,
    )]
    pub arena_item: Account<'info, ArenaItem>,

    /// The NFT mint bound to this item.
    #[account(mut)]
    pub mint: Account<'info, Mint>,

    /// Owner's token account — must hold exactly the 1 NFT token.
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = owner,
        constraint = token_account.amount == 1 @ ArenaRegistryError::NotNftHolder,
    )]
    pub token_account: Account<'info, TokenAccount>,

    /// CHECK: Metaplex metadata PDA, closed by the `BurnNft` CPI.
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    pub metadata_account: UncheckedAccount<'info>,

    /// CHECK: Metaplex master edition PDA, closed by the `BurnNft` CPI.
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), mint.key().as_ref(), b"edition"],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    pub master_edition: UncheckedAccount<'info>,

    #[account(mut)]
    pub owner: Signer<'info>,

    pub token_metadata_program: Program<'info, Metadata>,
    pub token_program: Program<'info, Token>,
}
