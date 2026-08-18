use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaCardKind {
    Avatar,
    Modifier,
}

impl ArenaCardKind {
    pub const INIT_SPACE: usize = 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaRarity {
    Common,
    Rare,
    Legendary,
    Unique,
    Epic,
    /// v4 god-roll (~1/1000). Only the commit-reveal mint can produce it
    /// (spec §12.2). The 1-tx dev mint is clamped below this.
    Mythic,
}

impl ArenaRarity {
    pub const INIT_SPACE: usize = 1;

    /// Map a raw affix-generator rarity id (spec §6/§12.2 order) to the enum.
    pub fn from_roll(rarity: u8) -> Self {
        match rarity {
            crate::affix::RARITY_COMMON => ArenaRarity::Common,
            crate::affix::RARITY_RARE => ArenaRarity::Rare,
            crate::affix::RARITY_EPIC => ArenaRarity::Epic,
            crate::affix::RARITY_LEGENDARY => ArenaRarity::Legendary,
            crate::affix::RARITY_MYTHIC => ArenaRarity::Mythic,
            _ => ArenaRarity::Common,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaBaseType {
    Weapon,
    Head,
    Armor,
    Charm,
}

impl ArenaBaseType {
    pub const INIT_SPACE: usize = 1;

    /// Affix-generator base-type id (spec §2/§5 ordering: Weapon..Charm).
    pub fn to_roll(self) -> u8 {
        match self {
            ArenaBaseType::Weapon => crate::affix::BASE_WEAPON,
            ArenaBaseType::Head => crate::affix::BASE_HEAD,
            ArenaBaseType::Armor => crate::affix::BASE_ARMOR,
            ArenaBaseType::Charm => crate::affix::BASE_CHARM,
        }
    }

    /// Equip-slot index into `PlayerAvatar::equipped` (and bit position in the
    /// avatar card `slot_mask`).
    pub fn slot_index(self) -> u8 {
        match self {
            ArenaBaseType::Weapon => 0,
            ArenaBaseType::Head => 1,
            ArenaBaseType::Armor => 2,
            ArenaBaseType::Charm => 3,
        }
    }

    /// v2 slot-compatibility rule ("Lineage tribute" equip protocol).
    ///
    /// The 7 web equip slots map onto the 4 on-chain item classes:
    ///   Weapon → Weapon; Head → Head;
    ///   Armor  → Body | Gloves | Boots (any Armor-class piece);
    ///   Charm  → Amulet | Ring        (any Charm-class trinket).
    /// Reserved slots (7..15) accept nothing until a future version names them.
    pub fn allowed_in_equip_slot(self, slot: u8) -> bool {
        match slot {
            EQUIP_SLOT_WEAPON => self == ArenaBaseType::Weapon,
            EQUIP_SLOT_HEAD => self == ArenaBaseType::Head,
            EQUIP_SLOT_BODY | EQUIP_SLOT_GLOVES | EQUIP_SLOT_BOOTS => self == ArenaBaseType::Armor,
            EQUIP_SLOT_AMULET | EQUIP_SLOT_RING => self == ArenaBaseType::Charm,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// v2 equip slots ("Lineage tribute"): the equipped set IS the core protocol.
// Order mirrors the web client's `EQUIPMENT_SLOTS` exactly.
// ---------------------------------------------------------------------------

pub const EQUIP_SLOT_WEAPON: u8 = 0;
pub const EQUIP_SLOT_HEAD: u8 = 1;
pub const EQUIP_SLOT_BODY: u8 = 2;
pub const EQUIP_SLOT_GLOVES: u8 = 3;
pub const EQUIP_SLOT_BOOTS: u8 = 4;
pub const EQUIP_SLOT_AMULET: u8 = 5;
pub const EQUIP_SLOT_RING: u8 = 6;

/// Slots currently addressable by `equip_item_v2` / `unequip_item_v2`.
pub const ACTIVE_EQUIP_SLOT_COUNT: u8 = 7;

/// Physical slots reserved in `EquipmentRecord` (room to grow without another
/// layout migration).
pub const EQUIPMENT_RECORD_SLOTS: usize = 16;

/// Legacy `PlayerAvatar::equipped` index mirrored by a v2 slot, so pre-v2
/// readers keep seeing the canonical four. Only the canonical slot of each
/// item class mirrors: Weapon→0, Head→1, Body→2 (Armor), Amulet→3 (Charm).
/// Gloves/Boots/Ring exist only in the `EquipmentRecord`.
pub fn legacy_equipped_index(slot: u8) -> Option<usize> {
    match slot {
        EQUIP_SLOT_WEAPON => Some(0),
        EQUIP_SLOT_HEAD => Some(1),
        EQUIP_SLOT_BODY => Some(2),
        EQUIP_SLOT_AMULET => Some(3),
        _ => None,
    }
}

/// The full equipped set of one `PlayerAvatar` — THE battle-relevant read.
///
/// PDA: `["equipment", player_avatar]`. Created lazily (init_if_needed) by the
/// first `equip_item_v2`/`unequip_item_v2`; a missing record simply means
/// "nothing equipped via v2" (readers fall back to the legacy 4-slot mirror on
/// the avatar). Chosen over resizing `PlayerAvatar.equipped` because that is a
/// fixed `[Pubkey; 4]` baked into every deployed avatar account — a separate
/// PDA needs zero migration and keeps old readers working.
///
/// Same holder rule as the legacy slots: equipping does NOT lock the NFT.
/// A slot is valid for a fight only while `owner` still holds the mint's
/// single token.
#[account]
pub struct EquipmentRecord {
    /// The `PlayerAvatar` this record belongs to (PDA seed).
    pub avatar: Pubkey,
    /// The avatar's owner wallet (denormalized for cheap holder checks).
    pub owner: Pubkey,
    /// Equipped item NFT mints, indexed by `EQUIP_SLOT_*`
    /// (`Pubkey::default()` = empty). Slots 7..15 are reserved.
    pub slots: [Pubkey; EQUIPMENT_RECORD_SLOTS],
    pub bump: u8,
}

impl EquipmentRecord {
    pub const INIT_SPACE: usize = 32 // avatar
        + 32 // owner
        + 32 * EQUIPMENT_RECORD_SLOTS
        + 1; // bump
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaElement {
    None,
    Fire,
    Ice,
    Poison,
    Holy,
}

impl ArenaElement {
    pub const INIT_SPACE: usize = 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaStats {
    pub hp: i16,
    pub attack: i16,
    pub armor: i16,
    pub speed: i16,
}

impl ArenaStats {
    pub const INIT_SPACE: usize = 2 + 2 + 2 + 2;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterArenaAssetArgs {
    pub metadata_ipfs_hash: String,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub base_stats: ArenaStats,
    pub stat_delta: ArenaStats,
    pub slot_mask: u8,
    pub rarity: ArenaRarity,
    pub element: ArenaElement,
    pub skill_ids: Vec<String>,
}

/// Skin-only args for `register_arena_asset_from_stellar` (spec §2/§7/§8b).
///
/// A Stellar publish enters Arena as a SKIN ONLY — it carries zero balance.
/// Deliberately drops `base_stats/stat_delta/rarity/element` so the Stellar
/// caller cannot inject stats; balance comes from a later `mint_arena_item`
/// roll. Only the cosmetic/identity fields cross the bridge.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterArenaAssetFromStellarArgs {
    pub metadata_ipfs_hash: String,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub slot_mask: u8,
    pub skill_ids: Vec<String>,
}

/// Requested skin source for a minted item (cosmetic only, spec §2).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum MintSkinArg {
    /// Built-in skin id (validated against MAX_BUILTIN_SKINS).
    Builtin(u8),
    /// IPFS hash for a self-hosted skin.
    Ipfs(String),
    /// Resolve the skin from the supplied Stellar release accounts.
    Stellar,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct MintArenaItemArgs {
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
    /// NFT Metadata `name` (e.g. "Ekza Arena Item #12"). Client builds it.
    pub name: String,
    /// NFT Metadata `symbol` (e.g. "EKZAITEM").
    pub symbol: String,
    /// Off-chain metadata JSON URI (skin image/animation + stats mirror, spec
    /// §11.2). Client uploads to IPFS and passes the gateway/ipfs URI.
    pub uri: String,
}

impl MintArenaItemArgs {
    pub const MAX_NAME_LEN: usize = 32;
    pub const MAX_SYMBOL_LEN: usize = 10;
    pub const MAX_URI_LEN: usize = 200;
}

#[account]
pub struct ArenaRegistry {
    pub next_index: u64,
    /// Bootstrap is restricted to the program upgrade/genesis authority; all
    /// later configuration and rotation require the authority stored here.
    pub configuration_authority: Pubkey,
    /// Destination of the platform slice of the non-refundable commit fee.
    pub treasury: Pubkey,
    /// Destination of the protocol-sink slice.
    pub sink: Pubkey,
    /// Non-refundable fee charged at `commit_mint`, in lamports.
    pub commit_fee_lamports: u64,
    pub creator_bps: u16,
    pub platform_bps: u16,
    pub sink_bps: u16,
    pub bump: u8,
}

impl ArenaRegistry {
    pub const INIT_SPACE: usize = 8 // next_index
        + 32 // configuration_authority
        + 32 // treasury
        + 32 // sink
        + 8 // commit_fee_lamports
        + 2 // creator_bps
        + 2 // platform_bps
        + 2 // sink_bps
        + 1; // bump
}

/// Args for `configure_registry` — set fee destinations and the complete split.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigureRegistryArgs {
    pub treasury: Pubkey,
    pub sink: Pubkey,
    pub commit_fee_lamports: u64,
    pub creator_bps: u16,
    pub platform_bps: u16,
    pub sink_bps: u16,
}

#[account]
pub struct ArenaAssetData {
    pub metadata_ipfs_hash: String,
    pub creator: Pubkey,
    pub index: u64,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub base_stats: ArenaStats,
    pub stat_delta: ArenaStats,
    pub slot_mask: u8,
    pub rarity: ArenaRarity,
    pub element: ArenaElement,
    pub skill_ids: Vec<String>,
    /// Cosmetic skin source (spec §2/§8b). Stellar-bridged assets store
    /// `StellarAsset(<asset pubkey>)`; direct/manual cards default to a builtin.
    pub skin_ref: ItemSkin,
    pub bump: u8,
}

impl ArenaAssetData {
    pub const MAX_METADATA_HASH_LEN: usize = 200;
    pub const MAX_ARCHETYPE_ID_LEN: usize = 64;
    pub const MAX_SKILL_IDS: usize = 8;
    pub const MAX_SKILL_ID_LEN: usize = 40;
    pub const SKILL_IDS_SPACE: usize = 4 + Self::MAX_SKILL_IDS * (4 + Self::MAX_SKILL_ID_LEN);
    pub const INIT_SPACE: usize = 4
        + Self::MAX_METADATA_HASH_LEN
        + 32
        + 8
        + ArenaCardKind::INIT_SPACE
        + 4
        + Self::MAX_ARCHETYPE_ID_LEN
        + ArenaStats::INIT_SPACE
        + ArenaStats::INIT_SPACE
        + 1
        + ArenaRarity::INIT_SPACE
        + ArenaElement::INIT_SPACE
        + Self::SKILL_IDS_SPACE
        + ItemSkin::INIT_SPACE
        + 1;
}

#[account]
pub struct StellarArenaAssetLink {
    pub arena_asset: Pubkey,
    pub stellar_program: Pubkey,
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub release: Pubkey,
    pub vault: Pubkey,
    pub bump: u8,
}

impl StellarArenaAssetLink {
    pub const INIT_SPACE: usize = 32 + 32 + 32 + 32 + 32 + 32 + 1;
}

#[account]
pub struct StellarReleaseLink {
    pub release: Pubkey,
    pub stellar_program: Pubkey,
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub vault: Pubkey,
    pub arena_asset: Pubkey,
    pub bump: u8,
}

impl StellarReleaseLink {
    pub const INIT_SPACE: usize = 32 + 32 + 32 + 32 + 32 + 32 + 1;
}

// ---------------------------------------------------------------------------
// Rolled gear (spec §7): deterministic, seed-driven ArenaItem PDA.
// ---------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaAffix {
    pub kind: u8,
    pub value: i16,
    pub element: u8,
}

impl ArenaAffix {
    pub const INIT_SPACE: usize = 1 + 2 + 1;
}

/// Forward-compat gem socket (spec §12.3/§4 of rng-economy-upgrades). Reserved
/// now (defaulted empty at mint); the `socket_gem` instruction lands in v5.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Socket {
    pub kind: u8,
    pub value: i16,
}

impl Socket {
    pub const INIT_SPACE: usize = 1 + 2;
    /// Max sockets reserved in `ArenaItem::INIT_SPACE` (spec §12.3).
    pub const MAX_SOCKETS: usize = 3;
}

/// Skin source — purely cosmetic, carries zero balance impact (spec §2).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum ItemSkin {
    Ipfs(String),
    StellarAsset(Pubkey),
    Builtin(u8),
}

impl ItemSkin {
    pub const MAX_IPFS_LEN: usize = 96;
    /// enum tag (1) + largest variant: Ipfs => 4 (len prefix) + MAX_IPFS_LEN.
    pub const INIT_SPACE: usize = 1 + 4 + Self::MAX_IPFS_LEN;
}

#[account]
pub struct ArenaItem {
    pub seed: u64,
    pub base_type: ArenaBaseType,
    pub tier: u8,
    pub rarity: ArenaRarity,
    pub affixes: Vec<ArenaAffix>,
    pub skin_ref: ItemSkin,
    /// Original creator of the item. NOT the owner — ownership is the current
    /// holder of the `mint` NFT token (spec §11.2). Gameplay reads must resolve
    /// the owner from the token holder, never from `minter`.
    pub minter: Pubkey,
    /// The SPL mint of the tradeable NFT this item is bound to (1:1). The PDA is
    /// seeded by this mint (spec §11.2).
    pub mint: Pubkey,
    pub index: u64,
    /// Sharpening level MIRROR. Defaulted 0 at mint; `reveal_enhance` writes
    /// it on every successful attempt, in lockstep with the authoritative
    /// `ItemEnhancement.level` (["enhancement", mint]), so pre-enhancement
    /// readers of `ArenaItem` stay coherent.
    pub enhance_level: u8,
    /// Forward-compat (spec §12.3): gem sockets. Defaulted empty at mint, space
    /// reserved for MAX_SOCKETS; no instruction mutates it yet (v5 `socket_gem`).
    pub sockets: Vec<Socket>,
    pub bump: u8,
}

impl ArenaItem {
    pub const MAX_AFFIXES: usize = crate::affix::MAX_AFFIXES;
    pub const AFFIXES_SPACE: usize = 4 + Self::MAX_AFFIXES * ArenaAffix::INIT_SPACE;
    pub const SOCKETS_SPACE: usize = 4 + Socket::MAX_SOCKETS * Socket::INIT_SPACE;
    pub const INIT_SPACE: usize = 8 // seed
        + ArenaBaseType::INIT_SPACE
        + 1 // tier
        + ArenaRarity::INIT_SPACE
        + Self::AFFIXES_SPACE
        + ItemSkin::INIT_SPACE
        + 32 // minter
        + 32 // mint
        + 8 // index
        + 1 // enhance_level
        + Self::SOCKETS_SPACE
        + 1; // bump

    /// Derived view: fold the flat affixes into the legacy stat block so the
    /// off-chain battle math (`with_delta` semantics) is preserved (spec §7).
    pub fn stat_delta(&self) -> ArenaStats {
        let mut delta = ArenaStats::default();
        for affix in &self.affixes {
            match affix.kind {
                crate::affix::KIND_FLAT_HP => delta.hp = delta.hp.saturating_add(affix.value),
                crate::affix::KIND_FLAT_ATK => {
                    delta.attack = delta.attack.saturating_add(affix.value)
                }
                crate::affix::KIND_FLAT_ARMOR => {
                    delta.armor = delta.armor.saturating_add(affix.value)
                }
                crate::affix::KIND_FLAT_SPEED => {
                    delta.speed = delta.speed.saturating_add(affix.value)
                }
                _ => {}
            }
        }
        delta
    }

    /// First element-bearing affix, mapped to the engine `ArenaElement`.
    pub fn element(&self) -> ArenaElement {
        let raw = self
            .affixes
            .iter()
            .map(|a| a.element)
            .find(|&e| e != crate::affix::ELEM_NONE)
            .unwrap_or(crate::affix::ELEM_NONE);
        match raw {
            crate::affix::ELEM_FIRE => ArenaElement::Fire,
            crate::affix::ELEM_ICE => ArenaElement::Ice,
            crate::affix::ELEM_POISON => ArenaElement::Poison,
            _ => ArenaElement::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Player avatar (character customization + on-chain equip).
// ---------------------------------------------------------------------------

/// A player's character: one per wallet (`[PLAYER_AVATAR_SEED, owner]`).
///
/// Holds the chosen Avatar card, a display name, a cosmetic skin, and the
/// equipped item NFTs — one slot per `ArenaBaseType` (Weapon/Head/Armor/Charm).
/// A slot stores the item's **NFT mint** (`Pubkey::default()` = empty).
///
/// IMPORTANT for gameplay reads: equipping does NOT lock the NFT — the owner can
/// still trade it. Clients must treat a slot as valid only while the avatar
/// owner still holds the mint's single token (same holder-resolution rule as
/// `ArenaItem.minter` vs owner).
/// Equip slots on a `PlayerAvatar` — one per `ArenaBaseType`.
pub const EQUIP_SLOT_COUNT: usize = 4;

#[account]
pub struct PlayerAvatar {
    pub owner: Pubkey,
    /// The `ArenaAssetData` Avatar card this character is based on.
    pub avatar_asset: Pubkey,
    /// Player-chosen display name (cosmetic only).
    pub name: String,
    /// Cosmetic skin override; defaults to the avatar card's `skin_ref`.
    pub skin_ref: ItemSkin,
    /// Copy of the avatar card's `slot_mask` at create/swap time — which equip
    /// slots this character supports (bit N = `ArenaBaseType::slot_index()` N).
    pub slot_mask: u8,
    /// Equipped item NFT mints, indexed by `ArenaBaseType::slot_index()`.
    pub equipped: [Pubkey; EQUIP_SLOT_COUNT],
    pub bump: u8,
}

impl PlayerAvatar {
    pub const SLOT_COUNT: usize = EQUIP_SLOT_COUNT;
    pub const MAX_NAME_LEN: usize = 32;
    pub const INIT_SPACE: usize = 32 // owner
        + 32 // avatar_asset
        + 4 + Self::MAX_NAME_LEN
        + ItemSkin::INIT_SPACE
        + 1 // slot_mask
        + 32 * Self::SLOT_COUNT
        + 1; // bump
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CreatePlayerAvatarArgs {
    pub name: String,
}

/// Holder-gated activation of a protocol-minted P3 fighter. Kept separate from
/// `CreatePlayerAvatarArgs` so the legacy catalog-card create ABI stays intact.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivateFighterV2Args {
    pub name: String,
}

/// Args for `customize_avatar`. All fields optional — only supplied parts
/// change. Passing a new `avatar_asset` account (see context) swaps the base
/// character and clears all equipped slots.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CustomizeAvatarArgs {
    pub name: Option<String>,
    /// Cosmetic skin override. Only `Builtin`/`Ipfs` are accepted here (Stellar
    /// skins enter via the avatar card's own `skin_ref`).
    pub skin: Option<MintSkinArg>,
}

// ---------------------------------------------------------------------------
// Item enhancement («заточка», docs/enhancement-design.md): consumable scroll
// NFTs + a per-item level PDA + a commit-reveal upgrade roll.
// ---------------------------------------------------------------------------

/// Per-item enhancement tracker — PDA `["enhancement", item_mint]`.
///
/// Created lazily on the first `commit_enhance` for an item. Kept SEPARATE
/// from `ArenaItem` so the deployed account layout does not change (no
/// migration). `level` here is AUTHORITATIVE; `ArenaItem.enhance_level` is a
/// mirror written in lockstep on every successful reveal. Read side:
/// `effective_stat = base * (100 + 8 * level) / 100`, computed off-chain by
/// the SDK/web from `level`.
#[account]
pub struct ItemEnhancement {
    /// The item NFT mint this tracker belongs to (PDA seed).
    pub item_mint: Pubkey,
    /// Current enhancement level, 0..=MAX_ENHANCE_LEVEL.
    pub level: u8,
    /// Lifetime attempts (analytics).
    pub attempts: u16,
    /// True while an `EnhanceCommit` is open for this item — blocks a second
    /// concurrent commit (the spec's "no open commit for this item" guard).
    pub pending: bool,
    pub bump: u8,
}

impl ItemEnhancement {
    pub const INIT_SPACE: usize = 32 // item_mint
        + 1 // level
        + 2 // attempts
        + 1 // pending
        + 1; // bump
}

/// Marker PDA `["scroll", scroll_mint]` proving the scroll NFT was issued by
/// the fee-paying `mint_enhance_scroll` — a foreign NFT has no marker and can
/// never enter `commit_enhance` (the spec's "no free scrolls" rule).
#[account]
pub struct EnhanceScrollMarker {
    pub scroll_mint: Pubkey,
    pub bump: u8,
}

impl EnhanceScrollMarker {
    pub const INIT_SPACE: usize = 32 + 1;
}

/// A pending enhancement attempt, committed to a FUTURE slot — same
/// commit-reveal shape as `MintCommit`. The scroll sits escrowed in an ATA
/// owned by this PDA until `reveal_enhance` burns it (any outcome) or
/// `close_expired_enhance_commit` returns it.
#[account]
pub struct EnhanceCommit {
    pub owner: Pubkey,
    pub nonce: u64,
    /// The item NFT mint whose enhancement this commit rolls.
    pub item_mint: Pubkey,
    /// The escrowed scroll NFT mint.
    pub scroll_mint: Pubkey,
    /// Slot whose hash seeds the roll; reveal must wait until `Clock::slot`
    /// has passed it AND the hash is still in the SlotHashes sysvar.
    pub target_slot: u64,
    pub bump: u8,
}

impl EnhanceCommit {
    pub const INIT_SPACE: usize = 32 // owner
        + 8 // nonce
        + 32 // item_mint
        + 32 // scroll_mint
        + 8 // target_slot
        + 1; // bump
}

/// Args for `mint_enhance_scroll` — Metaplex name/uri only; the symbol is
/// forced to `SCROLL_SYMBOL` so every scroll is recognizable.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct MintEnhanceScrollArgs {
    pub name: String,
    pub uri: String,
}

/// Outcome of one `reveal_enhance`.
#[event]
pub struct EnhanceResult {
    pub item_mint: Pubkey,
    pub level_before: u8,
    pub success: bool,
    /// True only on a risky-zone failure: the item NFT was burned and its
    /// `ArenaItem` + `ItemEnhancement` PDAs were closed (rent → owner).
    pub destroyed: bool,
}

// ---------------------------------------------------------------------------
// Commit-reveal mint (spec §12.1): the canonical, grind-resistant mint path.
// ---------------------------------------------------------------------------

/// Args for `commit_mint` (spec §12.1). The roll is NOT performed here — only
/// the intent + future-slot lock is persisted; `reveal_mint` rolls from the
/// then-unknown slot hash.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitMintArgs {
    /// Caller-chosen nonce, part of the `MintCommit` PDA seeds (lets one minter
    /// have many concurrent commits).
    pub nonce: u64,
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

/// A pending mint, committed to a FUTURE slot (spec §12.1). `reveal_mint` reads
/// the first produced hash at/after `target_slot` (unknown at commit time) to
/// derive the seed, making
/// the roll unpredictable and thus revert-grind resistant.
#[account]
pub struct MintCommit {
    pub minter: Pubkey,
    pub nonce: u64,
    /// Slot whose hash seeds the roll; reveal must wait until `Clock::slot` has
    /// passed it AND the hash is still in the SlotHashes sysvar (~512 slots).
    pub target_slot: u64,
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    /// Stellar identity bound and paid at commit time. All-zero for a
    /// Builtin/IPFS skin. Binding it prevents swapping releases at reveal.
    pub stellar_release: Pubkey,
    pub stellar_vault: Pubkey,
    pub stellar_asset: Pubkey,
    /// Metaplex royalty recipient selected at commit: Stellar release
    /// authority for creator-backed skins, platform treasury otherwise.
    pub royalty_recipient: Pubkey,
    pub bump: u8,
}

impl MintCommit {
    /// Largest `MintSkinArg` payload: tag(1) + Ipfs(4 len + MAX_IPFS_LEN).
    pub const SKIN_SPACE: usize = 1 + 4 + ItemSkin::MAX_IPFS_LEN;
    pub const INIT_SPACE: usize = 32 // minter
        + 8 // nonce
        + 8 // target_slot
        + ArenaBaseType::INIT_SPACE
        + Self::SKIN_SPACE
        + 4 + MintArenaItemArgs::MAX_NAME_LEN
        + 4 + MintArenaItemArgs::MAX_SYMBOL_LEN
        + 4 + MintArenaItemArgs::MAX_URI_LEN
        + 32 // stellar_release
        + 32 // stellar_vault
        + 32 // stellar_asset
        + 32 // royalty_recipient
        + 1; // bump
}
