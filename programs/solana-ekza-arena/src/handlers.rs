use anchor_lang::prelude::*;

use crate::{
    affix::roll_item,
    constants::{
        MAX_BUILTIN_SKINS, RELEASE_DEPLOYMENT_PROJECT_ARENA, RELEASE_STATUS_FINALIZED,
        RELEASE_STATUS_LINKED,
    },
    contexts::{
        MintArenaItem, RegisterArenaAsset, RegisterArenaAssetFromStellar, ScrapArenaItem,
    },
    error::ArenaRegistryError,
    state::{
        ArenaAffix, ArenaAssetData, ArenaCardKind, ArenaElement, ArenaRarity, ArenaStats, ItemSkin,
        MintArenaItemArgs, MintSkinArg, RegisterArenaAssetArgs, RegisterArenaAssetFromStellarArgs,
    },
    utils::validate_stellar_release,
    utils::{
        link_arena_asset_to_stellar, record_release_deployment_to_stellar,
        validate_stellar_universe_owner,
    },
};

fn validate_arena_asset_args(args: &RegisterArenaAssetArgs) -> Result<()> {
    require!(
        !args.metadata_ipfs_hash.is_empty()
            && args.metadata_ipfs_hash.len() <= ArenaAssetData::MAX_METADATA_HASH_LEN,
        ArenaRegistryError::InvalidMetadataLength
    );
    require!(
        is_valid_id(&args.archetype_id)
            && args.archetype_id.len() <= ArenaAssetData::MAX_ARCHETYPE_ID_LEN,
        ArenaRegistryError::InvalidArchetypeId
    );
    require!(args.slot_mask != 0, ArenaRegistryError::InvalidSlotMask);
    require!(
        args.skill_ids.len() <= ArenaAssetData::MAX_SKILL_IDS
            && args.skill_ids.iter().all(|skill| {
                is_valid_id(skill) && skill.len() <= ArenaAssetData::MAX_SKILL_ID_LEN
            }),
        ArenaRegistryError::InvalidSkillIds
    );

    if args.card_kind == ArenaCardKind::Avatar {
        require!(
            args.base_stats.hp > 0
                && args.base_stats.attack >= 0
                && args.base_stats.armor >= 0
                && args.base_stats.speed >= 0,
            ArenaRegistryError::InvalidStats
        );
    }

    Ok(())
}

fn is_valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b':')
        })
}

/// Skin-only validation for the from_stellar path (no stats to check).
fn validate_arena_asset_from_stellar_args(args: &RegisterArenaAssetFromStellarArgs) -> Result<()> {
    require!(
        !args.metadata_ipfs_hash.is_empty()
            && args.metadata_ipfs_hash.len() <= ArenaAssetData::MAX_METADATA_HASH_LEN,
        ArenaRegistryError::InvalidMetadataLength
    );
    require!(
        is_valid_id(&args.archetype_id)
            && args.archetype_id.len() <= ArenaAssetData::MAX_ARCHETYPE_ID_LEN,
        ArenaRegistryError::InvalidArchetypeId
    );
    require!(args.slot_mask != 0, ArenaRegistryError::InvalidSlotMask);
    require!(
        args.skill_ids.len() <= ArenaAssetData::MAX_SKILL_IDS
            && args.skill_ids.iter().all(|skill| {
                is_valid_id(skill) && skill.len() <= ArenaAssetData::MAX_SKILL_ID_LEN
            }),
        ArenaRegistryError::InvalidSkillIds
    );
    Ok(())
}

fn write_arena_asset(
    arena_asset: &mut Account<ArenaAssetData>,
    args: RegisterArenaAssetArgs,
    skin_ref: ItemSkin,
    creator: Pubkey,
    index: u64,
    bump: u8,
) {
    arena_asset.metadata_ipfs_hash = args.metadata_ipfs_hash;
    arena_asset.creator = creator;
    arena_asset.index = index;
    arena_asset.card_kind = args.card_kind;
    arena_asset.archetype_id = args.archetype_id;
    arena_asset.base_stats = args.base_stats;
    arena_asset.stat_delta = args.stat_delta;
    arena_asset.slot_mask = args.slot_mask;
    arena_asset.rarity = args.rarity;
    arena_asset.element = args.element;
    arena_asset.skill_ids = args.skill_ids;
    arena_asset.skin_ref = skin_ref;
    arena_asset.bump = bump;
}

pub fn register_arena_asset(
    ctx: Context<RegisterArenaAsset>,
    args: RegisterArenaAssetArgs,
) -> Result<()> {
    validate_arena_asset_args(&args)?;

    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    let index = registry.next_index;
    registry.next_index = registry
        .next_index
        .checked_add(1)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    // Direct/manual cards carry their own stats and default to a builtin skin
    // (spec §8b — Stellar identity is only set on the from_stellar path).
    write_arena_asset(
        &mut ctx.accounts.arena_asset,
        args,
        ItemSkin::Builtin(0),
        ctx.accounts.payer.key(),
        index,
        ctx.bumps.arena_asset,
    );

    Ok(())
}

pub fn register_arena_asset_from_stellar(
    ctx: Context<RegisterArenaAssetFromStellar>,
    args: RegisterArenaAssetFromStellarArgs,
) -> Result<()> {
    validate_arena_asset_from_stellar_args(&args)?;

    let origin = validate_stellar_release(
        &ctx.accounts.stellar_program,
        &ctx.accounts.stellar_release,
        &ctx.accounts.stellar_vault,
    )?;
    validate_stellar_universe_owner(
        &ctx.accounts.stellar_program,
        &ctx.accounts.stellar_universe,
        origin.universe,
        ctx.accounts.payer.key(),
    )?;
    require!(
        origin.status == RELEASE_STATUS_FINALIZED || origin.status == RELEASE_STATUS_LINKED,
        ArenaRegistryError::InvalidStellarRelease
    );

    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    let index = registry.next_index;
    registry.next_index = registry
        .next_index
        .checked_add(1)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    // SKIN-ONLY bridge (spec §2/§7/§8b): the Stellar publish contributes the
    // skin identity only — `skin_ref = StellarAsset(origin.asset)`. Stats are
    // forced to neutral/zero here so caller-supplied balance can never leak in;
    // balance is rolled later by `mint_arena_item`. The args struct itself omits
    // stats, this is belt-and-suspenders for the persisted account.
    let arena_asset = &mut ctx.accounts.arena_asset;
    arena_asset.metadata_ipfs_hash = args.metadata_ipfs_hash;
    arena_asset.creator = ctx.accounts.payer.key();
    arena_asset.index = index;
    arena_asset.card_kind = args.card_kind;
    arena_asset.archetype_id = args.archetype_id;
    arena_asset.base_stats = ArenaStats::default();
    arena_asset.stat_delta = ArenaStats::default();
    arena_asset.slot_mask = args.slot_mask;
    arena_asset.rarity = ArenaRarity::Common;
    arena_asset.element = ArenaElement::None;
    arena_asset.skill_ids = args.skill_ids;
    arena_asset.skin_ref = ItemSkin::StellarAsset(origin.asset);
    arena_asset.bump = ctx.bumps.arena_asset;

    let stellar_link = &mut ctx.accounts.stellar_link;
    stellar_link.arena_asset = ctx.accounts.arena_asset.key();
    stellar_link.stellar_program = ctx.accounts.stellar_program.key();
    stellar_link.universe = origin.universe;
    stellar_link.asset = origin.asset;
    stellar_link.release = ctx.accounts.stellar_release.key();
    stellar_link.vault = origin.vault;
    stellar_link.bump = ctx.bumps.stellar_link;

    let stellar_release_link = &mut ctx.accounts.stellar_release_link;
    stellar_release_link.release = ctx.accounts.stellar_release.key();
    stellar_release_link.stellar_program = ctx.accounts.stellar_program.key();
    stellar_release_link.universe = origin.universe;
    stellar_release_link.asset = origin.asset;
    stellar_release_link.vault = origin.vault;
    stellar_release_link.arena_asset = ctx.accounts.arena_asset.key();
    stellar_release_link.bump = ctx.bumps.stellar_release_link;

    if origin.status == RELEASE_STATUS_FINALIZED {
        link_arena_asset_to_stellar(
            ctx.accounts.arena_asset.key(),
            &ctx.accounts.payer.to_account_info(),
            &ctx.accounts.stellar_program,
            &ctx.accounts.stellar_universe,
            &ctx.accounts.stellar_release,
        )?;
    }

    record_release_deployment_to_stellar(
        RELEASE_DEPLOYMENT_PROJECT_ARENA,
        crate::ID,
        ctx.accounts.arena_asset.key(),
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.stellar_program,
        &ctx.accounts.stellar_release,
        &ctx.accounts.stellar_release_deployment,
        &ctx.accounts.system_program.to_account_info(),
    )?;

    Ok(())
}

/// First 8 bytes (LE u64) of the most-recent SlotHashes entry's hash (spec §3).
///
/// SlotHashes layout: `[len: u64][ (slot: u64, hash: [u8;32]) ; len ]`, newest
/// first. The recent hash starts at offset `8 (len) + 8 (slot) = 16`.
fn recent_slothash_u64(slot_hashes: &AccountInfo) -> Result<u64> {
    let data = slot_hashes.try_borrow_data()?;
    require!(data.len() >= 8, ArenaRegistryError::InvalidSlotHashes);
    let len = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if len == 0 || data.len() < 24 {
        // No entries yet (fresh validator boot) — fall back to zero entropy.
        return Ok(0);
    }
    Ok(u64::from_le_bytes(data[16..24].try_into().unwrap()))
}

/// Resolve the requested skin into the persisted `ItemSkin` (spec §2).
/// Stellar skins reuse the validated-release pattern; they contribute looks
/// only (the resolved value is the Stellar *asset* pubkey), never balance.
fn resolve_skin(ctx: &Context<MintArenaItem>, skin: MintSkinArg) -> Result<ItemSkin> {
    match skin {
        MintSkinArg::Builtin(id) => {
            require!(id < MAX_BUILTIN_SKINS, ArenaRegistryError::InvalidSkin);
            Ok(ItemSkin::Builtin(id))
        }
        MintSkinArg::Ipfs(hash) => {
            require!(
                !hash.is_empty() && hash.len() <= ItemSkin::MAX_IPFS_LEN,
                ArenaRegistryError::InvalidSkin
            );
            Ok(ItemSkin::Ipfs(hash))
        }
        MintSkinArg::Stellar => {
            let stellar_program = ctx
                .accounts
                .stellar_program
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_release = ctx
                .accounts
                .stellar_release
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_vault = ctx
                .accounts
                .stellar_vault
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;

            let origin =
                validate_stellar_release(stellar_program, stellar_release, stellar_vault)?;
            require!(
                origin.status == RELEASE_STATUS_FINALIZED
                    || origin.status == RELEASE_STATUS_LINKED,
                ArenaRegistryError::InvalidStellarRelease
            );
            Ok(ItemSkin::StellarAsset(origin.asset))
        }
    }
}

pub fn mint_arena_item(ctx: Context<MintArenaItem>, args: MintArenaItemArgs) -> Result<()> {
    // Resolve the skin first (and validate Stellar accounts if required).
    let skin_ref = resolve_skin(&ctx, args.skin)?;

    // Seed derivation (spec §3).
    let slothash_u64 = recent_slothash_u64(&ctx.accounts.slot_hashes)?;
    let minter = ctx.accounts.payer.key();
    let minter_first8 = u64::from_le_bytes(minter.to_bytes()[0..8].try_into().unwrap());

    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    let index = registry.next_index;

    let seed = crate::affix::splitmix64_mix(slothash_u64 ^ minter_first8 ^ index);

    // Roll the item (spec §5).
    let rolled = roll_item(seed, args.base_type.to_roll());

    registry.next_index = registry
        .next_index
        .checked_add(1)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    let item = &mut ctx.accounts.arena_item;
    item.seed = seed;
    item.base_type = args.base_type;
    item.tier = rolled.tier;
    item.rarity = ArenaRarity::from_roll(rolled.rarity);
    item.affixes = rolled
        .affixes
        .iter()
        .map(|a| ArenaAffix {
            kind: a.kind,
            value: a.value,
            element: a.element,
        })
        .collect();
    item.skin_ref = skin_ref;
    item.minter = minter;
    item.index = index;
    item.bump = ctx.bumps.arena_item;

    Ok(())
}

/// Scrap a rolled item — the v2 economic SINK (spec §10.5). The account is
/// closed via the `close = minter` constraint (rent returned to the owner) and
/// ownership is enforced by `has_one = minter` in the context. The handler body
/// is intentionally empty: all the work happens in the account constraints.
pub fn scrap_arena_item(_ctx: Context<ScrapArenaItem>) -> Result<()> {
    Ok(())
}
