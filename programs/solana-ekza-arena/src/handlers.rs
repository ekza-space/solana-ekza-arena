use anchor_lang::prelude::*;
use anchor_spl::metadata::{
    create_master_edition_v3, create_metadata_accounts_v3,
    mpl_token_metadata::types::{Creator, DataV2},
    CreateMasterEditionV3, CreateMetadataAccountsV3,
};
use anchor_spl::token::{mint_to, MintTo};

use solana_stellar::state::ReleaseStatus;

use crate::{
    affix::{roll_item, roll_item_capped, RARITY_LEGENDARY},
    constants::{
        BPS_DENOMINATOR, COMMIT_REVEAL_WINDOW_SLOTS, ENHANCE_COMMIT_SEED, ENHANCE_ROLL_DENOMINATOR,
        GENESIS_REGISTRY_AUTHORITY, ITEM_ROYALTY_BPS, MAX_BUILTIN_SKINS, MAX_ENHANCE_LEVEL,
        RELEASE_DEPLOYMENT_PROJECT_ARENA, REVEAL_DELAY_SLOTS, SCROLL_FEE_MULTIPLIER, SCROLL_SYMBOL,
        SUCCESS_BPS,
    },
    contexts::{
        ActivateFighterV2, CloseExpiredCommit, CloseExpiredEnhanceCommit, CommitEnhance,
        CommitMint, ConfigureRegistry, CreatePlayerAvatar, CustomizeAvatar, EquipItem, EquipItemV2,
        MigrateRegistryV1, MintArenaItem, MintEnhanceScroll, RegisterArenaAsset,
        RegisterArenaAssetFromStellar, RevealAvatarMint, RevealEnhance, RevealMint,
        RotateRegistryAuthority, ScrapArenaItem, UnequipItem, UnequipItemV2,
    },
    error::ArenaRegistryError,
    state::{
        ActivateFighterV2Args, ArenaAffix, ArenaAssetData, ArenaCardKind, ArenaElement,
        ArenaRarity, ArenaRegistry, ArenaStats, CommitMintArgs, ConfigureRegistryArgs,
        CreatePlayerAvatarArgs, CustomizeAvatarArgs, EnhanceResult, ItemSkin, MintArenaItemArgs,
        MintEnhanceScrollArgs, MintSkinArg, PlayerAvatar, RegisterArenaAssetArgs,
        RegisterArenaAssetFromStellarArgs,
    },
    utils::{deposit_revenue_to_stellar, validate_stellar_release, StellarReleaseOrigin},
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
        matches!(
            origin.status,
            ReleaseStatus::Finalized | ReleaseStatus::Linked
        ),
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

    if origin.status == ReleaseStatus::Finalized {
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

/// First 8 bytes (LE u64) of the first produced slot hash at or after the
/// committed lower bound. Slots may be skipped, so requiring an exact slot can
/// make an otherwise valid paid commit permanently unrevealable.
///
/// SlotHashes is newest-first. For a skipped target we only accept a candidate
/// after observing an older entry below the target. That boundary proves the
/// candidate is the first produced slot, and prevents a late reveal from
/// silently shifting to a newer hash after the canonical one has aged out.
fn slothash_at_or_after(slot_hashes: &AccountInfo, target_slot: u64) -> Result<u64> {
    let data = slot_hashes.try_borrow_data()?;
    slothash_at_or_after_data(&data, target_slot)
}

fn slothash_at_or_after_data(data: &[u8], target_slot: u64) -> Result<u64> {
    require!(data.len() >= 8, ArenaRegistryError::InvalidSlotHashes);
    let len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let required_len = len
        .checked_mul(40)
        .and_then(|entries| entries.checked_add(8))
        .ok_or(ArenaRegistryError::InvalidSlotHashes)?;
    require!(
        data.len() >= required_len,
        ArenaRegistryError::InvalidSlotHashes
    );

    let mut candidate = None;
    for i in 0..len {
        let base = 8 + i * 40;
        let slot = u64::from_le_bytes(data[base..base + 8].try_into().unwrap());
        if slot == target_slot {
            let hash_off = base + 8;
            return Ok(u64::from_le_bytes(
                data[hash_off..hash_off + 8].try_into().unwrap(),
            ));
        }
        if slot > target_slot {
            let hash_off = base + 8;
            candidate = Some(u64::from_le_bytes(
                data[hash_off..hash_off + 8].try_into().unwrap(),
            ));
        } else {
            return candidate.ok_or(ArenaRegistryError::SlotHashNotFound.into());
        }
    }
    // All retained entries are newer than the target. Without the lower
    // boundary we cannot distinguish "first post-target slot not recorded yet"
    // from "canonical slot hash aged out", so fail closed and allow a retry.
    Err(ArenaRegistryError::SlotHashNotFound.into())
}

/// Resolve the requested skin into the persisted `ItemSkin` (spec §2).
/// Stellar skins reuse the validated-release pattern; they contribute looks
/// only (the resolved value is the Stellar *asset* pubkey), never balance.
///
/// Account-based so both `mint_arena_item` and `reveal_mint` can call it with
/// their (optional) Stellar accounts.
struct ResolvedSkin {
    skin_ref: ItemSkin,
    stellar_origin: Option<StellarReleaseOrigin>,
}

fn resolve_skin_accounts<'info>(
    skin: MintSkinArg,
    stellar_program: Option<&AccountInfo<'info>>,
    stellar_release: Option<&AccountInfo<'info>>,
    stellar_vault: Option<&AccountInfo<'info>>,
) -> Result<ResolvedSkin> {
    match skin {
        MintSkinArg::Builtin(id) => {
            require!(id < MAX_BUILTIN_SKINS, ArenaRegistryError::InvalidSkin);
            Ok(ResolvedSkin {
                skin_ref: ItemSkin::Builtin(id),
                stellar_origin: None,
            })
        }
        MintSkinArg::Ipfs(hash) => {
            require!(
                !hash.is_empty() && hash.len() <= ItemSkin::MAX_IPFS_LEN,
                ArenaRegistryError::InvalidSkin
            );
            Ok(ResolvedSkin {
                skin_ref: ItemSkin::Ipfs(hash),
                stellar_origin: None,
            })
        }
        MintSkinArg::Stellar => {
            let stellar_program =
                stellar_program.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_release =
                stellar_release.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_vault =
                stellar_vault.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;

            let origin = validate_stellar_release(stellar_program, stellar_release, stellar_vault)?;
            require!(
                matches!(
                    origin.status,
                    ReleaseStatus::Finalized | ReleaseStatus::Linked
                ),
                ArenaRegistryError::InvalidStellarRelease
            );
            Ok(ResolvedSkin {
                skin_ref: ItemSkin::StellarAsset(origin.asset),
                stellar_origin: Some(origin),
            })
        }
    }
}

fn resolve_skin(ctx: &Context<MintArenaItem>, skin: MintSkinArg) -> Result<ResolvedSkin> {
    resolve_skin_accounts(
        skin,
        ctx.accounts.stellar_program.as_ref(),
        ctx.accounts.stellar_release.as_ref(),
        ctx.accounts.stellar_vault.as_ref(),
    )
}

pub fn mint_arena_item(ctx: Context<MintArenaItem>, args: MintArenaItemArgs) -> Result<()> {
    // Validate caller-supplied NFT metadata strings (spec §11.2).
    validate_nft_metadata(&args.name, &args.symbol, &args.uri)?;

    // The one-transaction path is deliberately a privileged development/admin
    // tool. Public production minting must use commit_mint so the configured
    // creator/platform/sink economics cannot be bypassed.
    require!(
        ctx.accounts.registry.configuration_authority != Pubkey::default(),
        ArenaRegistryError::RegistryNotConfigured
    );
    require_keys_eq!(
        ctx.accounts.registry.configuration_authority,
        ctx.accounts.payer.key(),
        ArenaRegistryError::QuickMintRestricted
    );

    // Resolve the skin first (and validate Stellar accounts if required).
    let resolved_skin = resolve_skin(&ctx, args.skin)?;
    let royalty_recipient = resolved_skin
        .stellar_origin
        .as_ref()
        .map(|origin| origin.authority)
        .unwrap_or_else(|| {
            if ctx.accounts.registry.treasury == Pubkey::default() {
                ctx.accounts.payer.key()
            } else {
                ctx.accounts.registry.treasury
            }
        });
    let skin_ref = resolved_skin.skin_ref;

    // Seed derivation (spec §3).
    let slothash_u64 = recent_slothash_u64(&ctx.accounts.slot_hashes)?;
    let minter = ctx.accounts.payer.key();
    let minter_first8 = u64::from_le_bytes(minter.to_bytes()[0..8].try_into().unwrap());

    let index = ctx.accounts.registry.next_index;
    let seed = crate::affix::splitmix64_mix(slothash_u64 ^ minter_first8 ^ index);

    // INTEGRITY (spec §12.1): this is the 1-tx dev/non-valuable path whose seed
    // is fully known at tx time, so it is revert-grindable. To protect the
    // jackpot it is hard-capped at Legendary and CANNOT roll Mythic — only the
    // commit-reveal `reveal_mint` can. Mythic rides on unpredictable randomness.
    let rolled = roll_item_capped(seed, args.base_type.to_roll(), RARITY_LEGENDARY);

    // --- Mint the real NFT (spec §11.2): supply 1, decimals 0, Master Edition. ---
    // 1. Mint the single token to the minter's ATA.
    mint_to(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.minter_token_account.to_account_info(),
                authority: ctx.accounts.payer.to_account_info(),
            },
        ),
        1,
    )?;

    // 2. Create the Metaplex Metadata account (immutable: is_mutable = false).
    let data = DataV2 {
        name: args.name,
        symbol: args.symbol,
        uri: args.uri,
        seller_fee_basis_points: ITEM_ROYALTY_BPS,
        creators: Some(vec![Creator {
            address: royalty_recipient,
            verified: royalty_recipient == ctx.accounts.payer.key(),
            share: 100,
        }]),
        collection: None,
        uses: None,
    };
    create_metadata_accounts_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                mint_authority: ctx.accounts.payer.to_account_info(),
                payer: ctx.accounts.payer.to_account_info(),
                update_authority: ctx.accounts.payer.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        data,
        false, // is_mutable
        true,  // update_authority_is_signer
        None,  // collection_details
    )?;

    // 3. Create the Master Edition (max_supply = 0 => true 1-of-1 non-fungible).
    create_master_edition_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMasterEditionV3 {
                edition: ctx.accounts.master_edition.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                update_authority: ctx.accounts.payer.to_account_info(),
                mint_authority: ctx.accounts.payer.to_account_info(),
                payer: ctx.accounts.payer.to_account_info(),
                metadata: ctx.accounts.metadata_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        Some(0),
    )?;

    // --- Persist game stats (the source of truth), bound 1:1 to the mint. ---
    let mint_key = ctx.accounts.mint.key();

    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
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
    item.mint = mint_key;
    item.index = index;
    // Forward-compat (spec §12.3): defaulted at mint, no mutator yet.
    item.enhance_level = 0;
    item.sockets = Vec::new();
    item.bump = ctx.bumps.arena_item;

    Ok(())
}

/// Scrap a rolled item — the economic SINK (spec §10.5/§11.3). BURNS the NFT
/// via Metaplex `BurnNft` (burns the token + closes the metadata and master
/// edition accounts), then the `close = owner` constraint closes the
/// `ArenaItem` PDA. All rent returns to the current NFT holder (the signer).
/// Ownership is enforced by the context: only the token holder can sign here.
pub fn scrap_arena_item(ctx: Context<ScrapArenaItem>) -> Result<()> {
    use anchor_spl::metadata::{burn_nft, BurnNft};

    burn_nft(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            BurnNft {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                owner: ctx.accounts.owner.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                token: ctx.accounts.token_account.to_account_info(),
                edition: ctx.accounts.master_edition.to_account_info(),
                spl_token: ctx.accounts.token_program.to_account_info(),
            },
        ),
        None,
    )?;

    Ok(())
}

/// Validate the NFT metadata strings shared by both mint paths (spec §11.2).
fn validate_nft_metadata(name: &str, symbol: &str, uri: &str) -> Result<()> {
    require!(
        !name.is_empty() && name.len() <= MintArenaItemArgs::MAX_NAME_LEN,
        ArenaRegistryError::InvalidNftMetadata
    );
    require!(
        !symbol.is_empty() && symbol.len() <= MintArenaItemArgs::MAX_SYMBOL_LEN,
        ArenaRegistryError::InvalidNftMetadata
    );
    require!(
        !uri.is_empty() && uri.len() <= MintArenaItemArgs::MAX_URI_LEN,
        ArenaRegistryError::InvalidNftMetadata
    );
    Ok(())
}

/// Fighter metadata is a gameplay identity boundary rather than merely NFT
/// display text. Keep it canonical enough that clients never need to accept
/// ambiguous names/URIs while authenticating the mint-keyed proof PDA.
fn validate_fighter_metadata(name: &str, uri: &str) -> Result<()> {
    let clean_name = !name.is_empty()
        && name == name.trim()
        && name.len() <= MintArenaItemArgs::MAX_NAME_LEN
        && !name.chars().any(char::is_control);
    let clean_uri = uri == uri.trim()
        && uri.len() <= MintArenaItemArgs::MAX_URI_LEN
        && (uri.starts_with("https://") || uri.starts_with("ipfs://"))
        && !uri.chars().any(char::is_control);
    require!(
        clean_name && clean_uri,
        ArenaRegistryError::InvalidFighterMetadata
    );
    Ok(())
}

fn validate_registry_config(args: &ConfigureRegistryArgs) -> Result<()> {
    let total_bps = u32::from(args.creator_bps)
        .checked_add(u32::from(args.platform_bps))
        .and_then(|value| value.checked_add(u32::from(args.sink_bps)))
        .ok_or(ArenaRegistryError::NumericalOverflow)?;
    require!(
        total_bps == u32::from(BPS_DENOMINATOR),
        ArenaRegistryError::InvalidFeeSplit
    );
    require!(
        args.treasury != Pubkey::default(),
        ArenaRegistryError::InvalidTreasury
    );
    require!(
        args.sink != Pubkey::default(),
        ArenaRegistryError::InvalidSink
    );
    Ok(())
}

fn require_program_upgrade_authority(
    program_data: &Account<ProgramData>,
    signer: Pubkey,
) -> Result<()> {
    require!(
        signer == GENESIS_REGISTRY_AUTHORITY
            || program_data.upgrade_authority_address == Some(signer),
        ArenaRegistryError::UnauthorizedRegistryBootstrap
    );
    Ok(())
}

/// Configure the registry's commit-reveal economics (spec §12.1).
pub fn configure_registry(
    ctx: Context<ConfigureRegistry>,
    args: ConfigureRegistryArgs,
) -> Result<()> {
    validate_registry_config(&args)?;

    let registry = &mut ctx.accounts.registry;
    if registry.configuration_authority == Pubkey::default() {
        require_program_upgrade_authority(&ctx.accounts.program_data, ctx.accounts.payer.key())?;
        registry.configuration_authority = ctx.accounts.payer.key();
    } else {
        require_keys_eq!(
            registry.configuration_authority,
            ctx.accounts.payer.key(),
            ArenaRegistryError::Unauthorized
        );
    }
    registry.bump = ctx.bumps.registry;
    registry.treasury = args.treasury;
    registry.sink = args.sink;
    registry.commit_fee_lamports = args.commit_fee_lamports;
    registry.creator_bps = args.creator_bps;
    registry.platform_bps = args.platform_bps;
    registry.sink_bps = args.sink_bps;
    Ok(())
}

pub fn rotate_registry_authority(
    ctx: Context<RotateRegistryAuthority>,
    new_authority: Pubkey,
) -> Result<()> {
    require!(
        new_authority != Pubkey::default(),
        ArenaRegistryError::InvalidConfigurationAuthority
    );
    ctx.accounts.registry.configuration_authority = new_authority;
    Ok(())
}

/// Legacy registry account bytes, including the 8-byte discriminator.
const LEGACY_REGISTRY_ACCOUNT_SPACE: usize = 8 + 8 + 32 + 8 + 1;

fn legacy_registry_next_index(data: &[u8]) -> Result<u64> {
    require!(
        data.len() == LEGACY_REGISTRY_ACCOUNT_SPACE && &data[..8] == ArenaRegistry::DISCRIMINATOR,
        ArenaRegistryError::InvalidRegistryMigration
    );
    Ok(u64::from_le_bytes(data[8..16].try_into().map_err(
        |_| ArenaRegistryError::InvalidRegistryMigration,
    )?))
}

pub fn migrate_registry_v1(
    ctx: Context<MigrateRegistryV1>,
    args: ConfigureRegistryArgs,
) -> Result<()> {
    validate_registry_config(&args)?;
    require_program_upgrade_authority(&ctx.accounts.program_data, ctx.accounts.payer.key())?;

    let registry_info = ctx.accounts.registry.to_account_info();
    let next_index = {
        let data = registry_info.try_borrow_data()?;
        legacy_registry_next_index(&data)?
    };

    let new_space = 8 + ArenaRegistry::INIT_SPACE;
    let required_lamports = Rent::get()?.minimum_balance(new_space);
    let top_up = required_lamports.saturating_sub(registry_info.lamports());
    if top_up > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: registry_info.clone(),
                },
            ),
            top_up,
        )?;
    }
    registry_info.resize(new_space)?;

    let migrated = ArenaRegistry {
        next_index,
        configuration_authority: ctx.accounts.payer.key(),
        treasury: args.treasury,
        sink: args.sink,
        commit_fee_lamports: args.commit_fee_lamports,
        creator_bps: args.creator_bps,
        platform_bps: args.platform_bps,
        sink_bps: args.sink_bps,
        bump: ctx.bumps.registry,
    };
    let mut data = registry_info.try_borrow_mut_data()?;
    data.fill(0);
    migrated.try_serialize(&mut &mut data[..])?;
    Ok(())
}

fn fee_slice(amount: u64, bps: u16) -> Result<u64> {
    let value = u128::from(amount)
        .checked_mul(u128::from(bps))
        .ok_or(ArenaRegistryError::NumericalOverflow)?
        .checked_div(u128::from(BPS_DENOMINATOR))
        .ok_or(ArenaRegistryError::NumericalOverflow)?;
    u64::try_from(value).map_err(|_| ArenaRegistryError::NumericalOverflow.into())
}

fn require_rent_safe_fee_destination(account: &AccountInfo, amount: u64) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let post_transfer_lamports = account
        .lamports()
        .checked_add(amount)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;
    let rent_exempt_minimum = Rent::get()?.minimum_balance(account.data_len());
    require!(
        post_transfer_lamports >= rent_exempt_minimum,
        ArenaRegistryError::FeeDestinationNotRentExempt
    );
    Ok(())
}

/// `commit_mint` (spec §12.1): persist the intent, lock a FUTURE slot, and
/// charge and fully distribute the non-refundable commit fee. No roll happens
/// here. Distribution at commit is intentional: reveal may be abandoned after
/// its SlotHash expires, but no paid SOL can remain stranded in a commit PDA.
pub fn commit_mint(ctx: Context<CommitMint>, args: CommitMintArgs) -> Result<()> {
    validate_nft_metadata(&args.name, &args.symbol, &args.uri)?;
    // Canonical EKZAF0..3 commits are fighter intents from this point on. Bind
    // their carrier + stricter metadata BEFORE taking the non-refundable fee,
    // so a malformed fighter cannot become a paid but unrevealable commit.
    if crate::fighter::faction_from_symbol(&args.symbol).is_some() {
        require!(
            args.base_type == crate::state::ArenaBaseType::Charm,
            ArenaRegistryError::InvalidFighterCommit
        );
        validate_fighter_metadata(&args.name, &args.uri)?;
    }

    let registry = &ctx.accounts.registry;
    // The registry must have been configured with a real treasury + fee.
    require!(
        registry.configuration_authority != Pubkey::default()
            && registry.treasury != Pubkey::default()
            && registry.sink != Pubkey::default(),
        ArenaRegistryError::RegistryNotConfigured
    );
    require_keys_eq!(
        ctx.accounts.treasury.key(),
        registry.treasury,
        ArenaRegistryError::InvalidTreasury
    );
    require_keys_eq!(
        ctx.accounts.sink.key(),
        registry.sink,
        ArenaRegistryError::InvalidSink
    );

    // Resolve and bind Stellar identity now, while `minter` can sign the
    // permissionless deposit_revenue CPI. reveal_mint validates the same
    // release/vault/asset again, preventing a release swap after payment.
    let resolved_skin = resolve_skin_accounts(
        args.skin.clone(),
        ctx.accounts.stellar_program.as_ref(),
        ctx.accounts.stellar_release.as_ref(),
        ctx.accounts.stellar_vault.as_ref(),
    )?;

    // Compute with u128 in fee_slice to make arbitrary governed fee values
    // overflow-safe. Platform receives division dust, so every lamport of the
    // configured fee is routed exactly once.
    let fee = registry.commit_fee_lamports;
    let sink_amount = fee_slice(fee, registry.sink_bps)?;
    let creator_amount = if resolved_skin.stellar_origin.is_some() {
        fee_slice(fee, registry.creator_bps)?
    } else {
        0
    };
    let platform_amount = fee
        .checked_sub(sink_amount)
        .and_then(|value| value.checked_sub(creator_amount))
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    // A sub-rent transfer cannot create a new SystemAccount. Check both plain
    // destinations before any CPI so a low launch fee fails with an explicit
    // configuration error instead of the runtime's opaque rent failure.
    require_rent_safe_fee_destination(&ctx.accounts.treasury.to_account_info(), platform_amount)?;
    require_rent_safe_fee_destination(&ctx.accounts.sink.to_account_info(), sink_amount)?;

    if creator_amount > 0 {
        deposit_revenue_to_stellar(
            creator_amount,
            &ctx.accounts.minter.to_account_info(),
            ctx.accounts
                .stellar_program
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?,
            ctx.accounts
                .stellar_release
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?,
            ctx.accounts
                .stellar_vault
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?,
            &ctx.accounts.system_program.to_account_info(),
        )?;
    }
    if platform_amount > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.minter.to_account_info(),
                    to: ctx.accounts.treasury.to_account_info(),
                },
            ),
            platform_amount,
        )?;
    }
    if sink_amount > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.minter.to_account_info(),
                    to: ctx.accounts.sink.to_account_info(),
                },
            ),
            sink_amount,
        )?;
    }

    let target_slot = Clock::get()?
        .slot
        .checked_add(REVEAL_DELAY_SLOTS)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    let commit = &mut ctx.accounts.mint_commit;
    commit.minter = ctx.accounts.minter.key();
    commit.nonce = args.nonce;
    commit.target_slot = target_slot;
    commit.base_type = args.base_type;
    commit.skin = args.skin;
    commit.name = args.name;
    commit.symbol = args.symbol;
    commit.uri = args.uri;
    if let Some(origin) = resolved_skin.stellar_origin {
        commit.stellar_release = ctx
            .accounts
            .stellar_release
            .as_ref()
            .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?
            .key();
        commit.stellar_vault = origin.vault;
        commit.stellar_asset = origin.asset;
        commit.royalty_recipient = origin.authority;
    } else {
        commit.stellar_release = Pubkey::default();
        commit.stellar_vault = Pubkey::default();
        commit.stellar_asset = Pubkey::default();
        commit.royalty_recipient = registry.treasury;
    }
    commit.bump = ctx.bumps.mint_commit;

    Ok(())
}

/// `reveal_mint` (spec §12.1): roll from the committed slot's now-known hash,
/// mint the NFT (spec §11), write `ArenaItem`, and close the `MintCommit`.
/// This is the ONLY path that can roll Mythic. Single-shot.
fn commit_expires_after(target_slot: u64) -> Result<u64> {
    target_slot
        .checked_add(COMMIT_REVEAL_WINDOW_SLOTS)
        .ok_or(ArenaRegistryError::NumericalOverflow.into())
}

pub fn reveal_mint(ctx: Context<RevealMint>, _nonce: u64) -> Result<()> {
    // Canonical fighter intents must never be consumable through the generic
    // gear branch. Otherwise an attacker could mint EKZAF metadata while
    // deliberately avoiding the mint-keyed Avatar proof PDA.
    require!(
        crate::fighter::faction_from_symbol(&ctx.accounts.mint_commit.symbol).is_none(),
        ArenaRegistryError::FighterSymbolReserved
    );

    // 1. Enforce the reveal window: the target slot must have PASSED, and its
    //    hash must still be retrievable from SlotHashes (spec §12.1).
    let target_slot = ctx.accounts.mint_commit.target_slot;
    let now = Clock::get()?.slot;
    require!(now > target_slot, ArenaRegistryError::RevealTooEarly);
    let expires_after = commit_expires_after(target_slot)?;
    require!(
        now <= expires_after,
        ArenaRegistryError::RevealWindowExpired
    );
    let slothash_u64 = slothash_at_or_after(&ctx.accounts.slot_hashes, target_slot)?;

    // 2. Seed = splitmix64_mix(target_slothash ^ minter_first8 ^ commit_first8).
    let minter = ctx.accounts.minter.key();
    let minter_first8 = u64::from_le_bytes(minter.to_bytes()[0..8].try_into().unwrap());
    let commit_key = ctx.accounts.mint_commit.key();
    let commit_first8 = u64::from_le_bytes(commit_key.to_bytes()[0..8].try_into().unwrap());
    let seed = crate::affix::splitmix64_mix(slothash_u64 ^ minter_first8 ^ commit_first8);

    // 3. Roll — full ladder (CAN be Mythic). Resolve the committed skin.
    let base_type = ctx.accounts.mint_commit.base_type;
    let skin_arg = ctx.accounts.mint_commit.skin.clone();
    let rolled = roll_item(seed, base_type.to_roll());
    let resolved_skin = resolve_skin_accounts(
        skin_arg,
        ctx.accounts.stellar_program.as_ref(),
        ctx.accounts.stellar_release.as_ref(),
        ctx.accounts.stellar_vault.as_ref(),
    )?;
    if let Some(origin) = resolved_skin.stellar_origin.as_ref() {
        require_keys_eq!(
            ctx.accounts
                .stellar_release
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?
                .key(),
            ctx.accounts.mint_commit.stellar_release,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.vault,
            ctx.accounts.mint_commit.stellar_vault,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.asset,
            ctx.accounts.mint_commit.stellar_asset,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.authority,
            ctx.accounts.mint_commit.royalty_recipient,
            ArenaRegistryError::StellarCommitMismatch
        );
    } else {
        require!(
            ctx.accounts.mint_commit.stellar_release == Pubkey::default()
                && ctx.accounts.mint_commit.stellar_vault == Pubkey::default()
                && ctx.accounts.mint_commit.stellar_asset == Pubkey::default(),
            ArenaRegistryError::StellarCommitMismatch
        );
    }
    let skin_ref = resolved_skin.skin_ref;

    // 4. Mint the real NFT (spec §11.2): supply 1, decimals 0, Master Edition.
    mint_to(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.minter_token_account.to_account_info(),
                authority: ctx.accounts.minter.to_account_info(),
            },
        ),
        1,
    )?;

    let data = DataV2 {
        name: ctx.accounts.mint_commit.name.clone(),
        symbol: ctx.accounts.mint_commit.symbol.clone(),
        uri: ctx.accounts.mint_commit.uri.clone(),
        seller_fee_basis_points: ITEM_ROYALTY_BPS,
        creators: Some(vec![Creator {
            address: ctx.accounts.mint_commit.royalty_recipient,
            verified: ctx.accounts.mint_commit.royalty_recipient == minter,
            share: 100,
        }]),
        collection: None,
        uses: None,
    };
    create_metadata_accounts_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                mint_authority: ctx.accounts.minter.to_account_info(),
                payer: ctx.accounts.minter.to_account_info(),
                update_authority: ctx.accounts.minter.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        data,
        false, // is_mutable
        true,  // update_authority_is_signer
        None,  // collection_details
    )?;

    create_master_edition_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMasterEditionV3 {
                edition: ctx.accounts.master_edition.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                update_authority: ctx.accounts.minter.to_account_info(),
                mint_authority: ctx.accounts.minter.to_account_info(),
                payer: ctx.accounts.minter.to_account_info(),
                metadata: ctx.accounts.metadata_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        Some(0),
    )?;

    // 5. Persist game stats, bound 1:1 to the mint (spec §11.2).
    let mint_key = ctx.accounts.mint.key();
    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    let index = registry.next_index;
    registry.next_index = registry
        .next_index
        .checked_add(1)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    let item = &mut ctx.accounts.arena_item;
    item.seed = seed;
    item.base_type = base_type;
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
    item.mint = mint_key;
    item.index = index;
    // Forward-compat (spec §12.3): defaulted at mint, no mutator yet.
    item.enhance_level = 0;
    item.sockets = Vec::new();
    item.bump = ctx.bumps.arena_item;

    // 6. `MintCommit` is closed by the `close = minter` constraint (rent →
    //    minter). Single-shot: this commit can never be revealed again.
    Ok(())
}

/// Hardened playable-fighter reveal.
///
/// The already-paid `MintCommit` remains the common commit ABI, so existing
/// item clients and pending item commits are untouched. A fighter intent is
/// recognized only when all of the following hold:
///
/// - the inert carrier class is `Charm` (the existing web convention);
/// - the symbol is exactly one of `EKZAF0..3`, which binds the faction;
/// - name/URI are canonical bounded display metadata.
///
/// Successful reveal writes an `ArenaAssetData { Avatar }` at
/// `[ARENA_AVATAR_SEED, mint]`. That program-owned mint-keyed PDA is the
/// anti-spoof proof; the Metaplex symbol by itself is never authoritative.
pub fn reveal_avatar_mint(ctx: Context<RevealAvatarMint>, _nonce: u64) -> Result<()> {
    let commit = &ctx.accounts.mint_commit;
    require!(
        commit.base_type == crate::state::ArenaBaseType::Charm,
        ArenaRegistryError::InvalidFighterCommit
    );
    let faction = crate::fighter::faction_from_symbol(&commit.symbol)
        .ok_or(ArenaRegistryError::InvalidFighterSymbol)?;
    validate_fighter_metadata(&commit.name, &commit.uri)?;

    // Same future-slot, expiry, and seed contract as the item reveal. The
    // caller-chosen fresh mint never contributes entropy, so key grinding
    // cannot improve fighter stats.
    let target_slot = commit.target_slot;
    let now = Clock::get()?.slot;
    require!(now > target_slot, ArenaRegistryError::RevealTooEarly);
    let expires_after = commit_expires_after(target_slot)?;
    require!(
        now <= expires_after,
        ArenaRegistryError::RevealWindowExpired
    );
    let slothash_u64 = slothash_at_or_after(&ctx.accounts.slot_hashes, target_slot)?;
    let minter = ctx.accounts.minter.key();
    let minter_first8 = u64::from_le_bytes(minter.to_bytes()[0..8].try_into().unwrap());
    let commit_key = ctx.accounts.mint_commit.key();
    let commit_first8 = u64::from_le_bytes(commit_key.to_bytes()[0..8].try_into().unwrap());
    let seed = crate::affix::splitmix64_mix(slothash_u64 ^ minter_first8 ^ commit_first8);
    let rolled = crate::fighter::roll_fighter(seed, faction)
        .ok_or(ArenaRegistryError::InvalidFighterSymbol)?;

    // Resolve the committed cosmetic identity again, and bind any Stellar
    // origin to the exact release/vault/asset whose creator fee was paid.
    let resolved_skin = resolve_skin_accounts(
        commit.skin.clone(),
        ctx.accounts.stellar_program.as_ref(),
        ctx.accounts.stellar_release.as_ref(),
        ctx.accounts.stellar_vault.as_ref(),
    )?;
    if let Some(origin) = resolved_skin.stellar_origin.as_ref() {
        require_keys_eq!(
            ctx.accounts
                .stellar_release
                .as_ref()
                .ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?
                .key(),
            commit.stellar_release,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.vault,
            commit.stellar_vault,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.asset,
            commit.stellar_asset,
            ArenaRegistryError::StellarCommitMismatch
        );
        require_keys_eq!(
            origin.authority,
            commit.royalty_recipient,
            ArenaRegistryError::StellarCommitMismatch
        );
    } else {
        require!(
            commit.stellar_release == Pubkey::default()
                && commit.stellar_vault == Pubkey::default()
                && commit.stellar_asset == Pubkey::default(),
            ArenaRegistryError::StellarCommitMismatch
        );
    }
    let skin_ref = resolved_skin.skin_ref;

    // Real immutable 1/1 NFT. Re-derive the canonical symbol from the parsed
    // faction rather than blindly copying caller text into Metadata.
    mint_to(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.minter_token_account.to_account_info(),
                authority: ctx.accounts.minter.to_account_info(),
            },
        ),
        1,
    )?;

    let canonical_symbol = crate::fighter::symbol_for_faction(faction)
        .ok_or(ArenaRegistryError::InvalidFighterSymbol)?;
    let data = DataV2 {
        name: commit.name.clone(),
        symbol: canonical_symbol.to_string(),
        uri: commit.uri.clone(),
        seller_fee_basis_points: ITEM_ROYALTY_BPS,
        creators: Some(vec![Creator {
            address: commit.royalty_recipient,
            verified: commit.royalty_recipient == minter,
            share: 100,
        }]),
        collection: None,
        uses: None,
    };
    create_metadata_accounts_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                mint_authority: ctx.accounts.minter.to_account_info(),
                payer: ctx.accounts.minter.to_account_info(),
                update_authority: ctx.accounts.minter.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        data,
        false,
        true,
        None,
    )?;
    create_master_edition_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMasterEditionV3 {
                edition: ctx.accounts.master_edition.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                update_authority: ctx.accounts.minter.to_account_info(),
                mint_authority: ctx.accounts.minter.to_account_info(),
                payer: ctx.accounts.minter.to_account_info(),
                metadata: ctx.accounts.metadata_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        Some(0),
    )?;

    // Durable gameplay truth. `archetype_id = fighter:<mint>` preserves the
    // web/domain identity contract and redundantly binds the fresh mint inside
    // the account data; the primary binding remains the PDA seed itself.
    let mint_key = ctx.accounts.mint.key();
    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    let index = registry.next_index;
    registry.next_index = registry
        .next_index
        .checked_add(1)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    let avatar = &mut ctx.accounts.avatar_asset;
    avatar.metadata_ipfs_hash = commit.uri.clone();
    avatar.creator = commit.royalty_recipient;
    avatar.index = index;
    avatar.card_kind = ArenaCardKind::Avatar;
    avatar.archetype_id = format!("fighter:{mint_key}");
    avatar.base_stats = ArenaStats {
        hp: rolled.stats.hp,
        attack: rolled.stats.attack,
        armor: rolled.stats.armor,
        speed: rolled.stats.speed,
    };
    avatar.stat_delta = ArenaStats::default();
    // All four legacy item classes are enabled. The v2 7-slot record maps its
    // named slots back through these same four governing bits.
    avatar.slot_mask = 0b0000_1111;
    avatar.rarity = ArenaRarity::from_roll(rolled.rarity);
    avatar.element = ArenaElement::None;
    avatar.skill_ids = rolled.skill_ids.into_iter().map(str::to_string).collect();
    avatar.skin_ref = skin_ref;
    avatar.bump = ctx.bumps.avatar_asset;

    // `MintCommit` closes to `minter` via the account constraint. There is no
    // `ArenaItem` for this mint, so it can never become equippable gear merely
    // by claiming an EKZAF symbol.
    Ok(())
}

pub fn close_expired_commit(ctx: Context<CloseExpiredCommit>, _nonce: u64) -> Result<()> {
    let expires_after = commit_expires_after(ctx.accounts.mint_commit.target_slot)?;
    require!(
        Clock::get()?.slot > expires_after,
        ArenaRegistryError::CommitNotExpired
    );
    // `close = minter` returns every lamport held by the PDA to the wallet that
    // paid its rent. The non-refundable mint fee was distributed at commit and
    // is never held here, so a permissionless closer cannot capture any value.
    Ok(())
}

// ---------------------------------------------------------------------------
// Item enhancement («заточка», docs/enhancement-design.md): consumable scroll
// NFTs + commit-reveal upgrade rolls off SlotHashes.
// ---------------------------------------------------------------------------

/// Commit guard shared with the unit tests: an item at the level cap can never
/// enter another attempt (the SUCCESS_BPS table has no row past +9→+10).
fn require_enhance_committable(level: u8) -> Result<()> {
    require!(
        level < MAX_ENHANCE_LEVEL,
        ArenaRegistryError::EnhanceLevelMaxed
    );
    Ok(())
}

/// `mint_enhance_scroll`: sell one consumable EnhanceScroll NFT for
/// `registry.commit_fee_lamports × SCROLL_FEE_MULTIPLIER`, split exactly like
/// a non-Stellar `commit_mint` (no creator on this path, so the creator share
/// folds into platform: sink slice by `sink_bps`, remainder → treasury). The
/// `["scroll", scroll_mint]` marker PDA is the proof-of-purchase gate.
pub fn mint_enhance_scroll(
    ctx: Context<MintEnhanceScroll>,
    args: MintEnhanceScrollArgs,
) -> Result<()> {
    validate_nft_metadata(&args.name, SCROLL_SYMBOL, &args.uri)?;

    let registry = &ctx.accounts.registry;
    require!(
        registry.configuration_authority != Pubkey::default()
            && registry.treasury != Pubkey::default()
            && registry.sink != Pubkey::default(),
        ArenaRegistryError::RegistryNotConfigured
    );
    require_keys_eq!(
        ctx.accounts.treasury.key(),
        registry.treasury,
        ArenaRegistryError::InvalidTreasury
    );
    require_keys_eq!(
        ctx.accounts.sink.key(),
        registry.sink,
        ArenaRegistryError::InvalidSink
    );

    let fee = registry
        .commit_fee_lamports
        .checked_mul(SCROLL_FEE_MULTIPLIER)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;
    let sink_amount = fee_slice(fee, registry.sink_bps)?;
    let platform_amount = fee
        .checked_sub(sink_amount)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;
    require_rent_safe_fee_destination(&ctx.accounts.treasury.to_account_info(), platform_amount)?;
    require_rent_safe_fee_destination(&ctx.accounts.sink.to_account_info(), sink_amount)?;
    if platform_amount > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.buyer.to_account_info(),
                    to: ctx.accounts.treasury.to_account_info(),
                },
            ),
            platform_amount,
        )?;
    }
    if sink_amount > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.buyer.to_account_info(),
                    to: ctx.accounts.sink.to_account_info(),
                },
            ),
            sink_amount,
        )?;
    }

    // Mint the scroll NFT (supply 1, decimals 0, Master Edition) — the exact
    // shape of the item mints so any wallet/marketplace displays it.
    mint_to(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.scroll_mint.to_account_info(),
                to: ctx.accounts.buyer_token_account.to_account_info(),
                authority: ctx.accounts.buyer.to_account_info(),
            },
        ),
        1,
    )?;

    let royalty_recipient = registry.treasury;
    let data = DataV2 {
        name: args.name,
        symbol: SCROLL_SYMBOL.to_string(),
        uri: args.uri,
        seller_fee_basis_points: ITEM_ROYALTY_BPS,
        creators: Some(vec![Creator {
            address: royalty_recipient,
            verified: royalty_recipient == ctx.accounts.buyer.key(),
            share: 100,
        }]),
        collection: None,
        uses: None,
    };
    create_metadata_accounts_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                mint: ctx.accounts.scroll_mint.to_account_info(),
                mint_authority: ctx.accounts.buyer.to_account_info(),
                payer: ctx.accounts.buyer.to_account_info(),
                update_authority: ctx.accounts.buyer.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        data,
        false, // is_mutable
        true,  // update_authority_is_signer
        None,  // collection_details
    )?;

    create_master_edition_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMasterEditionV3 {
                edition: ctx.accounts.master_edition.to_account_info(),
                mint: ctx.accounts.scroll_mint.to_account_info(),
                update_authority: ctx.accounts.buyer.to_account_info(),
                mint_authority: ctx.accounts.buyer.to_account_info(),
                payer: ctx.accounts.buyer.to_account_info(),
                metadata: ctx.accounts.metadata_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        ),
        Some(0),
    )?;

    let marker = &mut ctx.accounts.scroll_marker;
    marker.scroll_mint = ctx.accounts.scroll_mint.key();
    marker.bump = ctx.bumps.scroll_marker;

    Ok(())
}

/// Audit guard: an item wagered on an enhancement roll must not sit in any
/// equip slot. Both PDAs are address-bound by seeds in the context and may be
/// uninitialized (no avatar / no record yet) — only live, program-owned
/// accounts are deserialized and checked.
fn require_item_not_equipped(
    player_avatar: &AccountInfo,
    equipment_record: &AccountInfo,
    item_mint: &Pubkey,
) -> Result<()> {
    if player_avatar.owner == &crate::ID && !player_avatar.data_is_empty() {
        let data = player_avatar.try_borrow_data()?;
        let avatar = PlayerAvatar::try_deserialize(&mut &**data)?;
        require!(
            !avatar.equipped.iter().any(|slot| slot == item_mint),
            ArenaRegistryError::ItemEquipped
        );
    }
    if equipment_record.owner == &crate::ID && !equipment_record.data_is_empty() {
        let data = equipment_record.try_borrow_data()?;
        let record = crate::state::EquipmentRecord::try_deserialize(&mut &**data)?;
        require!(
            !record.slots.iter().any(|slot| slot == item_mint),
            ArenaRegistryError::ItemEquipped
        );
    }
    Ok(())
}

/// `commit_enhance`: persist the attempt intent, lock a FUTURE slot, and
/// escrow BOTH the scroll AND the item into the commit PDA's ATAs (v1.2 —
/// the item escrow makes the permissionless failure burn irrevocable; an SPL
/// delegate could be revoked or cleared by the owner to dodge a peeked loss).
/// No roll happens here — same revert-grind-resistant shape as `commit_mint`.
pub fn commit_enhance(ctx: Context<CommitEnhance>, nonce: u64) -> Result<()> {
    use anchor_spl::token::{transfer_checked, TransferChecked};

    let item_mint = ctx.accounts.item_mint.key();

    // No gambling with equipped gear (audit guard).
    require_item_not_equipped(
        &ctx.accounts.player_avatar,
        &ctx.accounts.equipment_record,
        &item_mint,
    )?;

    let enhancement = &mut ctx.accounts.enhancement;
    require_enhance_committable(enhancement.level)?;
    require!(!enhancement.pending, ArenaRegistryError::EnhancePending);
    // Idempotent identity stamp — first commit initializes, later ones re-affirm.
    enhancement.item_mint = item_mint;
    enhancement.pending = true;
    enhancement.bump = ctx.bumps.enhancement;

    // Escrow the scroll: it can no longer be sold or re-committed while the
    // outcome is pending.
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.scroll_token_account.to_account_info(),
                mint: ctx.accounts.scroll_mint.to_account_info(),
                to: ctx.accounts.scroll_escrow.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        ),
        1,
        0,
    )?;

    // Escrow the item (v1.2): hard-lock — the owner cannot transfer it away
    // or shield it from the failure burn once the odds are locked in.
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.item_token_account.to_account_info(),
                mint: ctx.accounts.item_mint.to_account_info(),
                to: ctx.accounts.item_escrow.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        ),
        1,
        0,
    )?;

    let target_slot = Clock::get()?
        .slot
        .checked_add(REVEAL_DELAY_SLOTS)
        .ok_or(ArenaRegistryError::NumericalOverflow)?;

    let commit = &mut ctx.accounts.enhance_commit;
    commit.owner = ctx.accounts.owner.key();
    commit.nonce = nonce;
    commit.item_mint = item_mint;
    commit.scroll_mint = ctx.accounts.scroll_mint.key();
    commit.target_slot = target_slot;
    commit.bump = ctx.bumps.enhance_commit;

    Ok(())
}

/// `reveal_enhance` — PERMISSIONLESS: any signer may reveal a pending commit
/// once its target slot passes, so a peeked loss can be forced through by a
/// rival or keeper. The item sits in escrow (v1.2), out of the owner's reach.
/// Success bumps the level (mirrored into `ArenaItem.enhance_level`) and
/// returns the item; a risky-zone failure (only reachable from +3 up, where
/// `SUCCESS_BPS[level] < 1000`) burns the item NFT from escrow and closes its
/// `ArenaItem` and `ItemEnhancement` PDAs (rent → owner). The scroll burns
/// from escrow REGARDLESS of outcome. Single-shot.
pub fn reveal_enhance(ctx: Context<RevealEnhance>, nonce: u64) -> Result<()> {
    use anchor_lang::AccountsClose;
    use anchor_spl::metadata::{burn_nft, BurnNft};
    use anchor_spl::token::{close_account, transfer_checked, CloseAccount, TransferChecked};

    // 1. Reveal window: identical policy to reveal_mint.
    let target_slot = ctx.accounts.enhance_commit.target_slot;
    let now = Clock::get()?.slot;
    require!(now > target_slot, ArenaRegistryError::RevealTooEarly);
    require!(
        now <= commit_expires_after(target_slot)?,
        ArenaRegistryError::RevealWindowExpired
    );
    let slothash_u64 = slothash_at_or_after(&ctx.accounts.slot_hashes, target_slot)?;

    // 2. Seed = splitmix64_mix(target_slothash ^ owner_first8 ^ item_first8 ^
    //    nonce) — the same first-8-LE-bytes mixing style as reveal_mint, over
    //    the spec's (slot_hash ++ owner ++ item_mint ++ nonce) inputs. The
    //    SDK/tests re-derive the roll from the same sysvar entry.
    let owner_key = ctx.accounts.owner.key();
    let owner_first8 = u64::from_le_bytes(owner_key.to_bytes()[0..8].try_into().unwrap());
    let item_mint_key = ctx.accounts.item_mint.key();
    let item_first8 = u64::from_le_bytes(item_mint_key.to_bytes()[0..8].try_into().unwrap());
    let seed = crate::affix::splitmix64_mix(slothash_u64 ^ owner_first8 ^ item_first8 ^ nonce);

    // 3. Roll against the one-const per-mille table, indexed by CURRENT level.
    let level_before = ctx.accounts.enhancement.level;
    require!(
        level_before < MAX_ENHANCE_LEVEL,
        ArenaRegistryError::EnhanceLevelMaxed
    );
    let roll = (seed % ENHANCE_ROLL_DENOMINATOR) as u16;
    let success = roll < SUCCESS_BPS[level_before as usize];

    // 4. Burn the scroll from escrow regardless of outcome (supply sink). The
    //    commit PDA owns the escrow ATA and signs the CPI; the token-account/
    //    metadata/edition refunds credit the commit PDA, whose `close = owner`
    //    constraint then forwards every lamport to the owner.
    let nonce_bytes = nonce.to_le_bytes();
    let commit_bump = ctx.accounts.enhance_commit.bump;
    let commit_signer: &[&[u8]] = &[
        ENHANCE_COMMIT_SEED,
        owner_key.as_ref(),
        &nonce_bytes,
        &[commit_bump],
    ];
    burn_nft(
        CpiContext::new_with_signer(
            ctx.accounts.token_metadata_program.to_account_info(),
            BurnNft {
                metadata: ctx.accounts.scroll_metadata.to_account_info(),
                owner: ctx.accounts.enhance_commit.to_account_info(),
                mint: ctx.accounts.scroll_mint.to_account_info(),
                token: ctx.accounts.scroll_escrow.to_account_info(),
                edition: ctx.accounts.scroll_master_edition.to_account_info(),
                spl_token: ctx.accounts.token_program.to_account_info(),
            },
            &[commit_signer],
        ),
        None,
    )?;

    // 5. Apply the outcome.
    let destroyed = !success;
    let enhancement = &mut ctx.accounts.enhancement;
    enhancement.attempts = enhancement.attempts.saturating_add(1);
    enhancement.pending = false;
    if success {
        enhancement.level = level_before + 1;
        // Mirror into the legacy field so pre-enhancement readers of
        // `ArenaItem` stay coherent; `ItemEnhancement.level` remains the
        // authoritative value (the two are only ever written together here).
        ctx.accounts.arena_item.enhance_level = level_before + 1;

        // Return the escrowed item to the owner and close the escrow ATA
        // (rent → owner). The commit PDA signs both CPIs.
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.item_escrow.to_account_info(),
                    mint: ctx.accounts.item_mint.to_account_info(),
                    to: ctx.accounts.item_token_account.to_account_info(),
                    authority: ctx.accounts.enhance_commit.to_account_info(),
                },
                &[commit_signer],
            ),
            1,
            0,
        )?;
        close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.item_escrow.to_account_info(),
                destination: ctx.accounts.owner.to_account_info(),
                authority: ctx.accounts.enhance_commit.to_account_info(),
            },
            &[commit_signer],
        ))?;
    } else {
        // Risky-zone failure: destroy the item FROM ESCROW — the owner cannot
        // front-run this with a transfer or revoke (v1.2). Full Metaplex
        // teardown: token + escrow ATA + metadata + master edition, refunds
        // credited to the commit PDA → forwarded to the owner. The game-level
        // item dies with its PDAs: `ArenaItem` (source of truth) and
        // `ItemEnhancement` close to the owner.
        burn_nft(
            CpiContext::new_with_signer(
                ctx.accounts.token_metadata_program.to_account_info(),
                BurnNft {
                    metadata: ctx.accounts.item_metadata.to_account_info(),
                    owner: ctx.accounts.enhance_commit.to_account_info(),
                    mint: ctx.accounts.item_mint.to_account_info(),
                    token: ctx.accounts.item_escrow.to_account_info(),
                    edition: ctx.accounts.item_master_edition.to_account_info(),
                    spl_token: ctx.accounts.token_program.to_account_info(),
                },
                &[commit_signer],
            ),
            None,
        )?;
        ctx.accounts
            .arena_item
            .close(ctx.accounts.owner.to_account_info())?;
        ctx.accounts
            .enhancement
            .close(ctx.accounts.owner.to_account_info())?;
    }

    emit!(EnhanceResult {
        item_mint: item_mint_key,
        level_before,
        success,
        destroyed,
    });

    // 6. `EnhanceCommit` + `EnhanceScrollMarker` close via their `close =
    //    owner` constraints. Single-shot: this commit can never re-roll.
    Ok(())
}

/// Permissionless cleanup of an expired enhancement commit: return the
/// escrowed ITEM to the owner (v1.2) but BURN the escrowed scroll — no
/// refund, so peek-and-abandon always costs the full ticket — release the
/// item's `pending` lock, and close the commit + marker (all rent/refunds →
/// owner, never the closer). Fee-free.
pub fn close_expired_enhance_commit(
    ctx: Context<CloseExpiredEnhanceCommit>,
    nonce: u64,
) -> Result<()> {
    use anchor_spl::metadata::{burn_nft, BurnNft};
    use anchor_spl::token::{close_account, transfer_checked, CloseAccount, TransferChecked};

    let expires_after = commit_expires_after(ctx.accounts.enhance_commit.target_slot)?;
    require!(
        Clock::get()?.slot > expires_after,
        ArenaRegistryError::CommitNotExpired
    );

    let owner_key = ctx.accounts.enhance_commit.owner;
    let nonce_bytes = nonce.to_le_bytes();
    let commit_bump = ctx.accounts.enhance_commit.bump;
    let commit_signer: &[&[u8]] = &[
        ENHANCE_COMMIT_SEED,
        owner_key.as_ref(),
        &nonce_bytes,
        &[commit_bump],
    ];

    // Return the escrowed item to the owner, then close the item escrow ATA
    // (rent → owner). The commit PDA signs both CPIs.
    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.item_escrow.to_account_info(),
                mint: ctx.accounts.item_mint.to_account_info(),
                to: ctx.accounts.owner_item_account.to_account_info(),
                authority: ctx.accounts.enhance_commit.to_account_info(),
            },
            &[commit_signer],
        ),
        1,
        0,
    )?;
    close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        CloseAccount {
            account: ctx.accounts.item_escrow.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(),
            authority: ctx.accounts.enhance_commit.to_account_info(),
        },
        &[commit_signer],
    ))?;

    // Burn the forfeited scroll from escrow. The token-account/metadata/
    // edition refunds credit the commit PDA, whose `close = owner` forwards
    // every lamport to the owner.
    burn_nft(
        CpiContext::new_with_signer(
            ctx.accounts.token_metadata_program.to_account_info(),
            BurnNft {
                metadata: ctx.accounts.scroll_metadata.to_account_info(),
                owner: ctx.accounts.enhance_commit.to_account_info(),
                mint: ctx.accounts.scroll_mint.to_account_info(),
                token: ctx.accounts.scroll_escrow.to_account_info(),
                edition: ctx.accounts.scroll_master_edition.to_account_info(),
                spl_token: ctx.accounts.token_program.to_account_info(),
            },
            &[commit_signer],
        ),
        None,
    )?;

    // Release the item lock so the owner can commit again (with a NEW scroll).
    ctx.accounts.enhancement.pending = false;

    // `EnhanceCommit` + `EnhanceScrollMarker` close via their `close = owner`
    // constraints; the closer never captures value.
    Ok(())
}

// ---------------------------------------------------------------------------
// Player avatar: character customization + on-chain equip.
// ---------------------------------------------------------------------------

fn validate_avatar_name(name: &str) -> Result<()> {
    require!(
        !name.trim().is_empty() && name.len() <= PlayerAvatar::MAX_NAME_LEN,
        ArenaRegistryError::InvalidAvatarName
    );
    Ok(())
}

/// The `fighter:` archetype namespace is written only by
/// `reveal_avatar_mint` and selected only by `activate_fighter_v2`.
///
/// Reserving the whole prefix is intentionally stricter than parsing a pubkey:
/// legacy catalog cards must never become indistinguishable from current or
/// future mint-keyed fighter proofs to a downstream reader.
fn require_legacy_catalog_avatar(card: &ArenaAssetData) -> Result<()> {
    require!(
        card.card_kind == ArenaCardKind::Avatar,
        ArenaRegistryError::InvalidAvatarAsset
    );
    require!(
        !card.archetype_id.starts_with("fighter:"),
        ArenaRegistryError::FighterActivationRequired
    );
    Ok(())
}

/// Cosmetic-only skin args allowed on a `PlayerAvatar` (no Stellar accounts in
/// the customize path — Stellar identity enters via the avatar card itself).
fn resolve_cosmetic_skin(skin: MintSkinArg) -> Result<ItemSkin> {
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
        MintSkinArg::Stellar => Err(ArenaRegistryError::InvalidSkin.into()),
    }
}

pub fn create_player_avatar(
    ctx: Context<CreatePlayerAvatar>,
    args: CreatePlayerAvatarArgs,
) -> Result<()> {
    validate_avatar_name(&args.name)?;
    let card = &ctx.accounts.avatar_asset;
    require_legacy_catalog_avatar(card)?;

    let avatar = &mut ctx.accounts.player_avatar;
    avatar.owner = ctx.accounts.owner.key();
    avatar.avatar_asset = card.key();
    avatar.name = args.name;
    avatar.skin_ref = card.skin_ref.clone();
    avatar.slot_mask = card.slot_mask;
    avatar.equipped = [Pubkey::default(); PlayerAvatar::SLOT_COUNT];
    avatar.bump = ctx.bumps.player_avatar;
    Ok(())
}

/// Create or switch the wallet's active protocol-minted fighter.
///
/// Account constraints prove current NFT ownership and the mint-keyed Avatar
/// PDA before this handler runs. The exact `fighter:<mint>` identity check is
/// redundant with the PDA seed by design: it catches corrupt/stale data and
/// gives downstream clients one stable identity string.
pub fn activate_fighter_v2(
    ctx: Context<ActivateFighterV2>,
    args: ActivateFighterV2Args,
) -> Result<()> {
    validate_avatar_name(&args.name)?;

    let owner = ctx.accounts.owner.key();
    let player_avatar_key = ctx.accounts.player_avatar.key();
    let fighter_mint = ctx.accounts.fighter_mint.key();
    require!(
        ctx.accounts.avatar_asset.archetype_id == format!("fighter:{fighter_mint}"),
        ArenaRegistryError::InvalidFighterAvatar
    );

    let avatar = &mut ctx.accounts.player_avatar;
    avatar.owner = owner;
    avatar.avatar_asset = ctx.accounts.avatar_asset.key();
    avatar.name = args.name;
    avatar.skin_ref = ctx.accounts.avatar_asset.skin_ref.clone();
    avatar.slot_mask = ctx.accounts.avatar_asset.slot_mask;
    avatar.equipped = [Pubkey::default(); PlayerAvatar::SLOT_COUNT];
    avatar.bump = ctx.bumps.player_avatar;

    let record = &mut ctx.accounts.equipment_record;
    record.avatar = player_avatar_key;
    record.owner = owner;
    record.slots = [Pubkey::default(); crate::state::EQUIPMENT_RECORD_SLOTS];
    record.bump = ctx.bumps.equipment_record;
    Ok(())
}

pub fn customize_avatar(ctx: Context<CustomizeAvatar>, args: CustomizeAvatarArgs) -> Result<()> {
    // Swap the base card first: it resets skin/slots, which the explicit
    // name/skin args below may then override in the same call.
    if let Some(card) = &ctx.accounts.new_avatar_asset {
        require_legacy_catalog_avatar(card)?;
        let avatar = &mut ctx.accounts.player_avatar;
        avatar.avatar_asset = card.key();
        avatar.skin_ref = card.skin_ref.clone();
        avatar.slot_mask = card.slot_mask;
        // The new card may support different slots — drop everything rather
        // than leave items in slots the character no longer has.
        avatar.equipped = [Pubkey::default(); PlayerAvatar::SLOT_COUNT];
    }

    let avatar = &mut ctx.accounts.player_avatar;
    if let Some(name) = args.name {
        validate_avatar_name(&name)?;
        avatar.name = name;
    }
    if let Some(skin) = args.skin {
        avatar.skin_ref = resolve_cosmetic_skin(skin)?;
    }
    Ok(())
}

pub fn equip_item(ctx: Context<EquipItem>) -> Result<()> {
    let slot = ctx.accounts.arena_item.base_type.slot_index();
    let avatar = &mut ctx.accounts.player_avatar;
    require!(
        avatar.slot_mask & (1u8 << slot) != 0,
        ArenaRegistryError::InvalidEquipSlot
    );
    avatar.equipped[slot as usize] = ctx.accounts.mint.key();
    Ok(())
}

pub fn unequip_item(ctx: Context<UnequipItem>, slot: u8) -> Result<()> {
    require!(
        (slot as usize) < PlayerAvatar::SLOT_COUNT,
        ArenaRegistryError::InvalidEquipSlot
    );
    ctx.accounts.player_avatar.equipped[slot as usize] = Pubkey::default();
    Ok(())
}

// ---------------------------------------------------------------------------
// v2 equip ("Lineage tribute"): explicit 7-slot equipped set, the battle read.
// ---------------------------------------------------------------------------

/// Stamp the record's identity fields. Idempotent — safe on both the lazy
/// `init_if_needed` creation and every subsequent call.
fn touch_equipment_record(
    record: &mut Account<crate::state::EquipmentRecord>,
    avatar: Pubkey,
    owner: Pubkey,
    bump: u8,
) {
    record.avatar = avatar;
    record.owner = owner;
    record.bump = bump;
}

/// Move an NFT mint into exactly one active v2 slot and keep the legacy mirror
/// coherent. Clearing every old occurrence also repairs records written by the
/// pre-uniqueness implementation the next time that mint is equipped.
fn move_equipped_mint(
    record_slots: &mut [Pubkey; crate::state::EQUIPMENT_RECORD_SLOTS],
    legacy_slots: &mut [Pubkey; crate::state::EQUIP_SLOT_COUNT],
    mint: Pubkey,
    target_slot: u8,
) {
    for current in record_slots
        .iter_mut()
        .take(crate::state::ACTIVE_EQUIP_SLOT_COUNT as usize)
    {
        if *current == mint {
            *current = Pubkey::default();
        }
    }
    // A legacy-only equip may predate creation of EquipmentRecord, so clear
    // the mint from that view even when no v2 occurrence was found.
    for current in legacy_slots.iter_mut() {
        if *current == mint {
            *current = Pubkey::default();
        }
    }

    record_slots[target_slot as usize] = mint;
    if let Some(legacy) = crate::state::legacy_equipped_index(target_slot) {
        legacy_slots[legacy] = mint;
    }
}

/// Equip an owned item NFT into an explicit v2 slot (0..6).
///
/// Checks, in order:
///   1. `slot` is an active slot (< `ACTIVE_EQUIP_SLOT_COUNT`).
///   2. The item's base type fits the slot (`allowed_in_equip_slot`: Weapon→
///      Weapon, Head→Head, Armor→Body/Gloves/Boots, Charm→Amulet/Ring).
///   3. The avatar card supports the governing item class (`slot_mask` bit of
///      `base_type.slot_index()` — so slotMask semantics are unchanged).
///   4. NFT ownership — enforced by the context (signer's ATA holds the token).
///
/// Canonical slots also mirror into the legacy `PlayerAvatar::equipped` so
/// pre-v2 readers keep working.
pub fn equip_item_v2(ctx: Context<EquipItemV2>, slot: u8) -> Result<()> {
    require!(
        slot < crate::state::ACTIVE_EQUIP_SLOT_COUNT,
        ArenaRegistryError::InvalidEquipSlot
    );
    let base_type = ctx.accounts.arena_item.base_type;
    require!(
        base_type.allowed_in_equip_slot(slot),
        ArenaRegistryError::ItemSlotMismatch
    );

    let avatar = &mut ctx.accounts.player_avatar;
    require!(
        avatar.slot_mask & (1u8 << base_type.slot_index()) != 0,
        ArenaRegistryError::InvalidEquipSlot
    );

    let mint = ctx.accounts.mint.key();
    let record = &mut ctx.accounts.equipment_record;
    touch_equipment_record(
        record,
        avatar.key(),
        ctx.accounts.owner.key(),
        ctx.bumps.equipment_record,
    );
    move_equipped_mint(&mut record.slots, &mut avatar.equipped, mint, slot);
    Ok(())
}

/// Clear one v2 slot (0..6) — and its legacy mirror, if the slot has one.
pub fn unequip_item_v2(ctx: Context<UnequipItemV2>, slot: u8) -> Result<()> {
    require!(
        slot < crate::state::ACTIVE_EQUIP_SLOT_COUNT,
        ArenaRegistryError::InvalidEquipSlot
    );
    let avatar = &mut ctx.accounts.player_avatar;
    let record = &mut ctx.accounts.equipment_record;
    touch_equipment_record(
        record,
        avatar.key(),
        ctx.accounts.owner.key(),
        ctx.bumps.equipment_record,
    );
    record.slots[slot as usize] = Pubkey::default();
    if let Some(legacy) = crate::state::legacy_equipped_index(slot) {
        avatar.equipped[legacy] = Pubkey::default();
    }
    Ok(())
}

#[cfg(test)]
mod registry_security_tests {
    use super::*;

    fn slot_hashes_data(entries: &[(u64, u64)]) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + entries.len() * 40);
        data.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        for (slot, hash_first8) in entries {
            data.extend_from_slice(&slot.to_le_bytes());
            data.extend_from_slice(&hash_first8.to_le_bytes());
            data.extend_from_slice(&[0u8; 24]);
        }
        data
    }

    #[test]
    fn reveal_entropy_uses_first_produced_slot_at_or_after_target() {
        let exact = slot_hashes_data(&[(110, 11), (105, 22), (104, 33)]);
        assert_eq!(slothash_at_or_after_data(&exact, 105).unwrap(), 22);

        // 105..107 were skipped: 108, not newest 111, is canonical.
        let skipped = slot_hashes_data(&[(111, 44), (108, 55), (104, 66)]);
        assert_eq!(slothash_at_or_after_data(&skipped, 105).unwrap(), 55);

        // No produced slot at/after target yet.
        let too_early = slot_hashes_data(&[(104, 77), (103, 88)]);
        assert!(slothash_at_or_after_data(&too_early, 105).is_err());

        // All retained entries are newer: the canonical boundary aged out, so
        // selecting the oldest retained value would create a timing reroll.
        let aged_out = slot_hashes_data(&[(700, 99), (600, 111)]);
        assert!(slothash_at_or_after_data(&aged_out, 100).is_err());

        let mut truncated = slot_hashes_data(&[(110, 1), (104, 2)]);
        truncated.pop();
        assert!(slothash_at_or_after_data(&truncated, 105).is_err());
    }

    #[test]
    fn legacy_registry_decoder_accepts_only_the_exact_v1_layout() {
        let expected_index = 42u64;
        let mut data = vec![0u8; LEGACY_REGISTRY_ACCOUNT_SPACE];
        data[..8].copy_from_slice(ArenaRegistry::DISCRIMINATOR);
        data[8..16].copy_from_slice(&expected_index.to_le_bytes());
        assert_eq!(legacy_registry_next_index(&data).unwrap(), expected_index);

        let mut wrong_discriminator = data.clone();
        wrong_discriminator[0] ^= 0xff;
        assert!(legacy_registry_next_index(&wrong_discriminator).is_err());

        data.push(0);
        assert!(legacy_registry_next_index(&data).is_err());
    }

    #[test]
    fn migrated_registry_serialization_writes_a_current_anchor_account() {
        let registry = ArenaRegistry {
            next_index: 42,
            configuration_authority: Pubkey::new_unique(),
            treasury: Pubkey::new_unique(),
            sink: Pubkey::new_unique(),
            commit_fee_lamports: 2_000_000,
            creator_bps: 5_000,
            platform_bps: 4_000,
            sink_bps: 1_000,
            bump: 254,
        };
        let mut data = vec![0u8; 8 + ArenaRegistry::INIT_SPACE];
        registry.try_serialize(&mut &mut data[..]).unwrap();
        assert_eq!(&data[..8], ArenaRegistry::DISCRIMINATOR);
        let decoded = ArenaRegistry::try_deserialize(&mut data.as_slice()).unwrap();
        assert_eq!(decoded.next_index, registry.next_index);
        assert_eq!(
            decoded.configuration_authority,
            registry.configuration_authority
        );
        assert_eq!(decoded.treasury, registry.treasury);
        assert_eq!(decoded.sink, registry.sink);
        assert_eq!(decoded.commit_fee_lamports, registry.commit_fee_lamports);
        assert_eq!(decoded.creator_bps, registry.creator_bps);
        assert_eq!(decoded.platform_bps, registry.platform_bps);
        assert_eq!(decoded.sink_bps, registry.sink_bps);
        assert_eq!(decoded.bump, registry.bump);
    }

    #[test]
    fn commitment_expiry_boundary_is_checked_and_overflow_safe() {
        assert_eq!(
            commit_expires_after(7).unwrap(),
            7 + COMMIT_REVEAL_WINDOW_SLOTS
        );
        assert!(commit_expires_after(u64::MAX).is_err());
    }

    #[test]
    fn fighter_metadata_uses_the_actual_arena_asset_storage_boundary() {
        assert_eq!(
            MintArenaItemArgs::MAX_URI_LEN,
            ArenaAssetData::MAX_METADATA_HASH_LEN
        );
        let max_uri = format!(
            "https://{}",
            "a".repeat(ArenaAssetData::MAX_METADATA_HASH_LEN - "https://".len())
        );
        assert_eq!(max_uri.len(), ArenaAssetData::MAX_METADATA_HASH_LEN);
        assert!(validate_fighter_metadata("Canonical Fighter", &max_uri).is_ok());

        let oversized = format!("{max_uri}x");
        assert!(validate_fighter_metadata("Canonical Fighter", &oversized).is_err());
        assert!(validate_fighter_metadata(" Fighter", "https://example.test/f.json").is_err());
        assert!(validate_fighter_metadata("Fighter", "data:application/json,{}").is_err());
        assert!(validate_fighter_metadata("Fighter\n", "https://example.test/f.json").is_err());
    }

    #[test]
    fn maximum_fighter_payload_serializes_into_arena_asset_space() {
        let mint = Pubkey::new_unique();
        let archetype_id = format!("fighter:{mint}");
        assert!(archetype_id.len() <= ArenaAssetData::MAX_ARCHETYPE_ID_LEN);
        let metadata_ipfs_hash = format!(
            "https://{}",
            "a".repeat(ArenaAssetData::MAX_METADATA_HASH_LEN - "https://".len())
        );
        let fighter = ArenaAssetData {
            metadata_ipfs_hash,
            creator: Pubkey::new_unique(),
            index: u64::MAX,
            card_kind: ArenaCardKind::Avatar,
            archetype_id,
            base_stats: ArenaStats {
                hp: i16::MAX,
                attack: i16::MAX,
                armor: i16::MAX,
                speed: i16::MAX,
            },
            stat_delta: ArenaStats::default(),
            slot_mask: 0b1111,
            rarity: ArenaRarity::Mythic,
            element: ArenaElement::None,
            skill_ids: vec!["jewelry_focus".to_string(), "heavy_guard".to_string()],
            skin_ref: ItemSkin::Ipfs("x".repeat(ItemSkin::MAX_IPFS_LEN)),
            bump: u8::MAX,
        };
        let mut account_data = vec![0u8; 8 + ArenaAssetData::INIT_SPACE];
        fighter
            .try_serialize(&mut &mut account_data[..])
            .expect("fighter must fit the declared ArenaAssetData allocation");
    }

    #[test]
    fn legacy_avatar_paths_reserve_the_protocol_fighter_namespace() {
        let mut card = ArenaAssetData {
            metadata_ipfs_hash: "https://example.test/avatar.json".to_string(),
            creator: Pubkey::new_unique(),
            index: 1,
            card_kind: ArenaCardKind::Avatar,
            archetype_id: "catalog-avatar".to_string(),
            base_stats: ArenaStats::default(),
            stat_delta: ArenaStats::default(),
            slot_mask: 0b1111,
            rarity: ArenaRarity::Common,
            element: ArenaElement::None,
            skill_ids: Vec::new(),
            skin_ref: ItemSkin::Builtin(0),
            bump: 255,
        };
        assert!(require_legacy_catalog_avatar(&card).is_ok());

        card.archetype_id = format!("fighter:{}", Pubkey::new_unique());
        assert!(require_legacy_catalog_avatar(&card).is_err());

        // Reserve malformed/future values too: prefix identity must never be
        // accepted through an instruction that carries no mint/ATA proof.
        card.archetype_id = "fighter:not-a-pubkey".to_string();
        assert!(require_legacy_catalog_avatar(&card).is_err());

        card.archetype_id = "catalog-avatar".to_string();
        card.card_kind = ArenaCardKind::Modifier;
        assert!(require_legacy_catalog_avatar(&card).is_err());
    }

    #[test]
    fn v2_equip_move_keeps_one_active_occurrence_and_repairs_legacy() {
        let mint = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let mut record = [Pubkey::default(); crate::state::EQUIPMENT_RECORD_SLOTS];
        let mut legacy = [Pubkey::default(); crate::state::EQUIP_SLOT_COUNT];

        // Model a pre-fix duplicate Armor plus a legacy-only occurrence.
        record[crate::state::EQUIP_SLOT_BODY as usize] = mint;
        record[crate::state::EQUIP_SLOT_GLOVES as usize] = mint;
        record[crate::state::EQUIP_SLOT_WEAPON as usize] = other;
        legacy[2] = mint;
        move_equipped_mint(
            &mut record,
            &mut legacy,
            mint,
            crate::state::EQUIP_SLOT_BOOTS,
        );

        assert_eq!(
            record[crate::state::EQUIP_SLOT_BODY as usize],
            Pubkey::default()
        );
        assert_eq!(
            record[crate::state::EQUIP_SLOT_GLOVES as usize],
            Pubkey::default()
        );
        assert_eq!(record[crate::state::EQUIP_SLOT_BOOTS as usize], mint);
        assert_eq!(record[crate::state::EQUIP_SLOT_WEAPON as usize], other);
        assert_eq!(legacy[2], Pubkey::default());
        assert_eq!(
            record
                .iter()
                .take(crate::state::ACTIVE_EQUIP_SLOT_COUNT as usize)
                .filter(|&&value| value == mint)
                .count(),
            1
        );

        // Moving back to canonical Body creates exactly its one legacy mirror.
        move_equipped_mint(
            &mut record,
            &mut legacy,
            mint,
            crate::state::EQUIP_SLOT_BODY,
        );
        assert_eq!(
            record[crate::state::EQUIP_SLOT_BOOTS as usize],
            Pubkey::default()
        );
        assert_eq!(record[crate::state::EQUIP_SLOT_BODY as usize], mint);
        assert_eq!(legacy[2], mint);
    }

    #[test]
    fn enhance_success_table_matches_the_spec_ladder() {
        // KEEP THE TABLE IN ONE CONST (enhancement spec) — this pins it.
        assert_eq!(
            SUCCESS_BPS,
            [1000, 1000, 1000, 700, 500, 350, 250, 175, 120, 80]
        );
        assert_eq!(SUCCESS_BPS.len(), MAX_ENHANCE_LEVEL as usize);
        // +1..+3 are the safe zone; from +4 every attempt can fail.
        for &per_mille in SUCCESS_BPS.iter().take(3) {
            assert_eq!(per_mille, 1000);
        }
        for &per_mille in SUCCESS_BPS.iter().skip(3) {
            assert!(per_mille < 1000);
        }
    }

    #[test]
    fn enhance_commit_is_rejected_at_the_level_cap() {
        // Reaching +10 honestly in E2E is a ~5e-5 event per item (every risky
        // failure burns it), so the cap guard is pinned at the unit level: the
        // exact check `commit_enhance` runs before accepting a scroll.
        for level in 0..MAX_ENHANCE_LEVEL {
            assert!(require_enhance_committable(level).is_ok());
        }
        assert!(require_enhance_committable(MAX_ENHANCE_LEVEL).is_err());
        assert!(require_enhance_committable(u8::MAX).is_err());
    }
}
