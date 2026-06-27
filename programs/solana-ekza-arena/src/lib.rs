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

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }

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
}

#[derive(Accounts)]
pub struct Initialize {}
