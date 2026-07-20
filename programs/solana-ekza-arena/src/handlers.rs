use anchor_lang::prelude::*;
use anchor_spl::metadata::{
    create_master_edition_v3, create_metadata_accounts_v3,
    mpl_token_metadata::types::DataV2, CreateMasterEditionV3, CreateMetadataAccountsV3,
};
use anchor_spl::token::{mint_to, MintTo};

use solana_stellar::state::ReleaseStatus;

use crate::{
    affix::{roll_item, roll_item_capped, RARITY_LEGENDARY},
    constants::{MAX_BUILTIN_SKINS, RELEASE_DEPLOYMENT_PROJECT_ARENA, REVEAL_DELAY_SLOTS},
    contexts::{
        CommitMint, ConfigureRegistry, CreatePlayerAvatar, CustomizeAvatar, EquipItem,
        EquipItemV2, MintArenaItem, RegisterArenaAsset, RegisterArenaAssetFromStellar, RevealMint,
        ScrapArenaItem, UnequipItem, UnequipItemV2,
    },
    error::ArenaRegistryError,
    state::{
        ArenaAffix, ArenaAssetData, ArenaCardKind, ArenaElement, ArenaRarity, ArenaStats,
        CommitMintArgs, ConfigureRegistryArgs, CreatePlayerAvatarArgs, CustomizeAvatarArgs,
        ItemSkin, MintArenaItemArgs, MintSkinArg, PlayerAvatar, RegisterArenaAssetArgs,
        RegisterArenaAssetFromStellarArgs,
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

/// First 8 bytes (LE u64) of the hash of EXACTLY `target_slot` from the
/// SlotHashes sysvar (spec §12.1). SlotHashes holds ~512 recent entries, newest
/// first: `[len: u64][ (slot: u64, hash: [u8;32]) ; len ]`. Returns
/// `SlotHashNotFound` if `target_slot` has aged out (or never existed).
fn slothash_of_slot(slot_hashes: &AccountInfo, target_slot: u64) -> Result<u64> {
    let data = slot_hashes.try_borrow_data()?;
    require!(data.len() >= 8, ArenaRegistryError::InvalidSlotHashes);
    let len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    // Each entry = 8 (slot) + 32 (hash) = 40 bytes, starting at offset 8.
    for i in 0..len {
        let base = 8 + i * 40;
        if data.len() < base + 40 {
            break;
        }
        let slot = u64::from_le_bytes(data[base..base + 8].try_into().unwrap());
        if slot == target_slot {
            let hash_off = base + 8;
            return Ok(u64::from_le_bytes(
                data[hash_off..hash_off + 8].try_into().unwrap(),
            ));
        }
    }
    Err(ArenaRegistryError::SlotHashNotFound.into())
}

/// Resolve the requested skin into the persisted `ItemSkin` (spec §2).
/// Stellar skins reuse the validated-release pattern; they contribute looks
/// only (the resolved value is the Stellar *asset* pubkey), never balance.
///
/// Account-based so both `mint_arena_item` and `reveal_mint` can call it with
/// their (optional) Stellar accounts.
fn resolve_skin_accounts<'info>(
    skin: MintSkinArg,
    stellar_program: Option<&AccountInfo<'info>>,
    stellar_release: Option<&AccountInfo<'info>>,
    stellar_vault: Option<&AccountInfo<'info>>,
) -> Result<ItemSkin> {
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
            let stellar_program =
                stellar_program.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_release =
                stellar_release.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;
            let stellar_vault =
                stellar_vault.ok_or(ArenaRegistryError::MissingStellarSkinAccounts)?;

            let origin =
                validate_stellar_release(stellar_program, stellar_release, stellar_vault)?;
            require!(
                matches!(
                    origin.status,
                    ReleaseStatus::Finalized | ReleaseStatus::Linked
                ),
                ArenaRegistryError::InvalidStellarRelease
            );
            Ok(ItemSkin::StellarAsset(origin.asset))
        }
    }
}

fn resolve_skin(ctx: &Context<MintArenaItem>, skin: MintSkinArg) -> Result<ItemSkin> {
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

    // Resolve the skin first (and validate Stellar accounts if required).
    let skin_ref = resolve_skin(&ctx, args.skin)?;

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
        seller_fee_basis_points: 0,
        creators: None,
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

/// Configure the registry's commit-reveal economics (spec §12.1).
pub fn configure_registry(
    ctx: Context<ConfigureRegistry>,
    args: ConfigureRegistryArgs,
) -> Result<()> {
    let registry = &mut ctx.accounts.registry;
    registry.bump = ctx.bumps.registry;
    registry.treasury = args.treasury;
    registry.commit_fee_lamports = args.commit_fee_lamports;
    Ok(())
}

/// `commit_mint` (spec §12.1): persist the intent, lock a FUTURE slot, and
/// charge the non-refundable commit fee to the treasury. No roll happens here.
pub fn commit_mint(ctx: Context<CommitMint>, args: CommitMintArgs) -> Result<()> {
    validate_nft_metadata(&args.name, &args.symbol, &args.uri)?;

    let registry = &ctx.accounts.registry;
    // The registry must have been configured with a real treasury + fee.
    require!(
        registry.treasury != Pubkey::default(),
        ArenaRegistryError::RegistryNotConfigured
    );
    require_keys_eq!(
        ctx.accounts.treasury.key(),
        registry.treasury,
        ArenaRegistryError::InvalidTreasury
    );

    // Validate the skin arg cheaply at commit time (Stellar accounts are not
    // required here — they are supplied at reveal). Builtin/Ipfs are fully
    // checked; Stellar is accepted and re-validated at reveal.
    match &args.skin {
        MintSkinArg::Builtin(id) => {
            require!(*id < MAX_BUILTIN_SKINS, ArenaRegistryError::InvalidSkin)
        }
        MintSkinArg::Ipfs(hash) => require!(
            !hash.is_empty() && hash.len() <= ItemSkin::MAX_IPFS_LEN,
            ArenaRegistryError::InvalidSkin
        ),
        MintSkinArg::Stellar => {}
    }

    // Charge the non-refundable commit fee: minter -> treasury (system transfer).
    let fee = registry.commit_fee_lamports;
    if fee > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.minter.to_account_info(),
                    to: ctx.accounts.treasury.to_account_info(),
                },
            ),
            fee,
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
    commit.bump = ctx.bumps.mint_commit;

    Ok(())
}

/// `reveal_mint` (spec §12.1): roll from the committed slot's now-known hash,
/// mint the NFT (spec §11), write `ArenaItem`, and close the `MintCommit`.
/// This is the ONLY path that can roll Mythic. Single-shot.
pub fn reveal_mint(ctx: Context<RevealMint>, _nonce: u64) -> Result<()> {
    // 1. Enforce the reveal window: the target slot must have PASSED, and its
    //    hash must still be retrievable from SlotHashes (spec §12.1).
    let target_slot = ctx.accounts.mint_commit.target_slot;
    let now = Clock::get()?.slot;
    require!(now > target_slot, ArenaRegistryError::RevealTooEarly);
    let slothash_u64 = slothash_of_slot(&ctx.accounts.slot_hashes, target_slot)?;

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
    let skin_ref = resolve_skin_accounts(
        skin_arg,
        ctx.accounts.stellar_program.as_ref(),
        ctx.accounts.stellar_release.as_ref(),
        ctx.accounts.stellar_vault.as_ref(),
    )?;

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
        seller_fee_basis_points: 0,
        creators: None,
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
    require!(
        card.card_kind == ArenaCardKind::Avatar,
        ArenaRegistryError::InvalidAvatarAsset
    );

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

pub fn customize_avatar(ctx: Context<CustomizeAvatar>, args: CustomizeAvatarArgs) -> Result<()> {
    // Swap the base card first: it resets skin/slots, which the explicit
    // name/skin args below may then override in the same call.
    if let Some(card) = &ctx.accounts.new_avatar_asset {
        require!(
            card.card_kind == ArenaCardKind::Avatar,
            ArenaRegistryError::InvalidAvatarAsset
        );
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

/// Equip an owned item NFT into an explicit v2 slot (0..6).
///
/// Checks, in order:
///   1. `slot` is an active slot (< `ACTIVE_EQUIP_SLOT_COUNT`).
///   2. The item's base type fits the slot (`allowed_in_equip_slot`: Weapon→
///      Weapon, Head→Head, Armor→Body/Gloves/Boots, Charm→Amulet/Ring).
///   3. The avatar card supports the governing item class (`slot_mask` bit of
///      `base_type.slot_index()` — so slotMask semantics are unchanged).
///   4. NFT ownership — enforced by the context (signer's ATA holds the token).
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
    record.slots[slot as usize] = mint;

    // Keep the legacy 4-slot view in sync for canonical slots.
    if let Some(legacy) = crate::state::legacy_equipped_index(slot) {
        avatar.equipped[legacy] = mint;
    }
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
