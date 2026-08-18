# Playable Fighters — minting a pack avatar into a character you play as

Status: DESIGN + SCAFFOLD-SPEC (devnet only). No deploy, no wiring yet.
Author: staff-eng design pass. Companion to `pvp-ladder-design.md`,
`enhancement-design.md`, `onchain-state.md`.

---

## 0. The problem (validated)

Pack **avatars** are the CHARACTER you play AS. **Gear** (weapons/armor/
jewelry/amulets) is what you EQUIP onto that character. Today the universe mint
flow (`MintCardButton.tsx` → `commitRevealMint.ts`) mints *everything*, avatars
included, as a generic equippable **Charm/Amulet `ArenaItem`** — an item skin you
can neither play as nor even equip until the Amulet slot unlocks at level 4.
That is nonsense for an avatar. The avatar item-mint was just disabled ("Mint as
fighter · soon"); this doc specifies the real thing.

Only the **12 hardcoded archetypes** in `content/avatars.ts` are playable. The
other three shipped universes — neko-samurai, aetherlings, glasswrights — have
avatars with `{id, name, faction, portrait, ipfsHash, stellarAddress?}` and **no
combat definition at all** (`universeRegistry.ts:19-26`).

### Validation — is the founder's framing correct?

Yes, and the on-chain schema already agrees with it. The deployed program has a
first-class **`ArenaCardKind { Avatar, Modifier }`** discriminator
(`state.rs:3-7`), a `PlayerAvatar` "who am I playing as" PDA, and
`create_player_avatar` that **rejects any card whose `card_kind != Avatar`**
(`handlers.rs:1541-1561`). The data model was built for avatars-as-characters;
the web client simply never wired the avatar path and instead routed avatars
through the item mint. The bug is a wiring gap, not a schema gap.

**Confirmed root-cause bug:** `chainAssetToModifier()`
(`ekza-stellar-sdk/src/assets/resolveSkin.ts:122-135`) folds **every**
`ArenaAssetData` — *including `cardKind: "Avatar"`* — into a gear `ItemCard`,
re-rolling item affixes from `index` and **ignoring `cardKind`, `baseStats`,
`stat_delta`, `skillIds`, `archetypeId`**. Any registered Avatar card is
currently mis-surfaced as an equippable modifier. This must be gated on
`cardKind === "Modifier"` regardless of which path below ships (see §7).

---

## 1. Where do a minted fighter's combat stats + skills come from?

### What the sim actually consumes

The battle sim (`combat.ts`) and PvP both consume a single flat value object:

```ts
// domain/ekza/types.ts:210
type CombatantSnapshot = {
  mint: string; name: string;
  stats: { hp; attack; armor; speed };   // Stats
  element?: "Fire"|"Ice"|"Poison";
  skillIds: string[];                     // drives skill_mask
};
```

`playerSnapshot(avatar, items)` (`combat.ts:19`) builds it from
`avatar.totalStats` + `activeSkillIds`. PvP publishes the **raw** total fold
(`avatar.totalStats`) plus a `skill_mask`; the resolver re-applies
glass-cannon/quickstep from the mask (`PVP-WEB-NOTES.md`). So a "playable
fighter" is fully described by **(stat vector, skill id set, optional element,
identity mint)** — nothing more. Skills are a **closed, balanced set of 7**:
`moss_skin, stone_oath, fire_opener, glass_cannon, heavy_guard, quickstep,
jewelry_focus` (canonical bit order, byte-identical to Rust `sim.rs`).

### What data we actually have for pack avatars

The manifest gives only **`faction` + art** (`universeRegistry.ts:19-26`). The
`generate-universe-set` skill produces *art only* — no authored stats. So any
solution that requires **hand-authoring combat defs per avatar (option b) does
not scale** to creator-generated universes and is rejected for pack avatars
(kept only for the 12 built-ins, which already have authored defs).

### Decision — (c) faction base vector + seed-rolled variance + faction skill set

A minted fighter's combat identity is **derived deterministically from
(faction, seed)**, exactly like items are derived from (seed, base_type):

```
base_stats  = FACTION_BASE_VECTOR[faction]  ⊕  rolledVariance(seed)
skill_ids   = FACTION_DEFAULT_SKILLS[faction]  (+ bonus skill on high rolls)
rarity      = rarityFromRoll(seed)           // Common..Mythic
element     = None (MVP; faction-flavored later)
```

Reasoning:

- **Fits the sim with zero new machinery** — it emits precisely the
  `{stats, skillIds, element}` the sim already eats. `FACTION_BASE_VECTOR`
  reuses the exact anchors the 12 built-ins were authored around (Moss = tanky
  HP, Spark = attack, Void = speed, Stone = armor; see the base-stat spreads in
  `content/avatars.ts:57-348` and the `FACTION_BONUSES` in
  `content/factions.ts`).
- **Fits the manifest data we have** — needs only `faction` (present) + a seed.
- **Provably-fair unique fighters** — the seed is the commit-reveal slot-hash
  entropy (the same source that makes item Mythics ungrindable). Two players who
  mint "Neko Ronin" get *different* stat rolls and rarities, so a rare roll has
  scarcity value — the property the founder wants and the shared-catalog path
  (§2, P1) cannot give.
- **Mirrors the existing roll contract** — `rollItem(seed, baseType)` already
  exists for gear; we add a sibling `rollFighter(seed, faction)` with the same
  deterministic, reproducible-on-both-sides shape.

The **seed is the fighter's NFT mint pubkey** (MVP) or the on-chain rolled
`base_stats` (hardening) — so the derivation is reproducible client-side *and*
verifiable against chain. `FACTION_BASE_VECTOR` and the variance→rarity ladder
are authored once (4 factions) and become the balance surface for all
creator universes.

> The 12 built-in archetypes keep their **hand-authored** defs (option b) — they
> are free guest content and their identities are load-bearing. Only *minted*
> pack fighters use the faction-template derivation.

---

## 2. On-chain model + the minimal change

### The three shapes the program actually offers

| Shape | Owns | Stats source | Rolled/unique | Tradeable NFT | Program change |
|---|---|---|---|---|---|
| **P1 catalog + PlayerAvatar** | a per-wallet `PlayerAvatar` PDA | shared `ArenaAssetData.base_stats` (one card per pack-avatar) | ❌ everyone identical | ❌ (it's a PDA, not an NFT) | **none** (fully wired) |
| **P2 item-machinery reuse** | a real SPL NFT (commit-reveal) | client-derived from mint seed + faction | ✅ | ✅ | **none** (client-only) |
| **P3 avatar-mint** | a real SPL NFT + a rolled `card_kind:Avatar` PDA | on-chain rolled `base_stats` | ✅ | ✅ | **one new reveal branch** |

Key facts that constrain the choice (all confirmed in the deployed program):

- **`PlayerAvatar` stores no stats/skills** — only `avatar_asset: Pubkey`
  (points at an `ArenaAssetData` card), `name`, `skin_ref`, `slot_mask`,
  `equipped: [Pubkey;4]` (`state.rs:495-522`). One per wallet, seed
  `[player_avatar_v1, owner]`.
- **Avatar combat stats live on the `ArenaAssetData` card**, gated
  `card_kind == Avatar` (`base_stats`, `stat_delta`, `element`, `skill_ids`;
  `state.rs:280-320`). Registering that card is the ONLY place avatar stats
  enter the chain today.
- **There is no instruction that mints a per-player *owned, rolled* avatar
  NFT.** `register_arena_asset` makes ONE shared card (permissionless payer =
  creator, takes explicit `base_stats`). `register_arena_asset_from_stellar`
  makes a shared card too but **zeroes stats / Common / skin-only**, gated to
  the Stellar universe owner (`handlers.rs:154-247`). `create_player_avatar`
  just points a wallet at an existing card. So P1 gives *owned-but-shared-stats*;
  it can never give a scarce rolled fighter.
- **`mint_arena_item` (1-tx) is authority-gated** (`QuickMintRestricted`,
  `handlers.rs`), so the only public mint is **commit-reveal**, and only
  `reveal_mint` can roll Mythic. `reveal_mint` today writes an `ArenaItem`
  (`base_type ∈ {Weapon,Head,Armor,Charm}`, affixes) — it has **no Avatar
  branch**.
- **The creator-royalty split already fires at `commit_mint`** — `creator_bps`
  of the fee is CPI-deposited into the Stellar release **vault** iff the mint
  carries a Stellar skin, and Metaplex `seller_fee_basis_points` /
  `royalty_recipient` are set to the release authority (`handlers.rs:744-835`).
  A pack author already earns when their pack's assets are minted through this
  path — no new economics needed.

### Recommendation — the fastest correct path

**MVP ships with ZERO program change, via P2.** Mint the pack fighter through
the *existing* commit-reveal NFT machinery (a real, owned, provably-rolled SPL
NFT that already pays the creator royalty), and **derive the fighter's
`CombatantSnapshot` client-side** from `(faction from manifest, mint pubkey as
seed)` using `rollFighter()` (§1). It appears in the picker, is playable, and
equips gear — all client-side, exactly the way the 12 built-ins already do.

Why P2 and not "just wire the already-built P1 path":

- P1 is genuinely zero-change and puts stats on-chain, **but** every "Neko
  Ronin" is stat-identical and the player owns a PDA, not a scarce NFT — it
  kills the "rare fighter has value" pillar and the tradeability the founder
  asked for. We keep P1 available as the **on-chain-equip hardening** layer
  (§3, Phase 2), not as the mint.
- P2 reuses a **battle-tested** path (commit-reveal, fee split, royalty,
  SlotHashes entropy, Metaplex CPI) and already makes the pack author money.
- P2's identity (the NFT mint) is **already** what PvP keys on:
  `avatarRefForAvatar()` uses the real mint verbatim when it parses as a pubkey
  (`pvpIx.ts:225-234`), so a minted fighter's `CharRecord` and ghost snapshot
  work with **no PvP change**.

The one honest gap in P2: the reveal writes an `ArenaItem` (a *gear* PDA), so
without care `loadOwnedArenaItems` would surface the fighter as an equippable
Charm — the original bug. P2 closes that with a **soft on-chain marker + a
client filter**, no program change:

- Mint the fighter with a reserved **Metaplex symbol** `EKZAFTR` (vs `EKZAITEM`)
  and a name prefix, and an **IPFS skin** = the avatar portrait CID.
- The read hop routes NFTs by symbol: `EKZAFTR` → **fighter roster**
  (`fighterFromMint`), everything else → item bag (`ownedItemToModifier`). The
  `ArenaItem.base_type` of a fighter mint is irrelevant to gameplay (we ignore
  its affixes and derive from faction+seed instead); it exists only because the
  item PDA schema requires one. Use `Charm` as the inert carrier.

This is spoofable in the sense that a hand-crafted NFT could claim the symbol —
harmless on devnet, and eliminated in Phase 3 when the on-chain `card_kind`
becomes the authority.

### The exact minimal on-chain change (Phase 3 hardening — NOT MVP)

When we want the stats to be **on-chain truth** (anti-cheat for PvP, cross-verify
in the sim), add exactly **one reveal branch** — no new PDA type, no new account
layout, reuse `ArenaAssetData`:

> **New instruction `reveal_avatar_mint`** (sibling of `reveal_mint`): same
> `MintCommit`, same fee already paid at commit, same SlotHashes entropy, same
> Metaplex NFT CPI. Instead of writing an `ArenaItem`, it writes an
> **`ArenaAssetData { card_kind: Avatar }`** PDA *keyed by the fresh mint*
> (new seed `[b"arena_avatar_v1", mint]`), with:
> `base_stats = faction_template(faction) ⊕ roll_variance(slot_hash)`,
> `skill_ids = faction_skills(faction)`, `rarity = roll(slot_hash)`,
> `skin_ref = Ipfs(cid)`, `archetype_id = "<universe>:<avatarId>"`,
> `creator = pack author`. `faction` is bound at `commit_mint` (add a
> `card_kind: ArenaCardKind` + `faction: ArenaFaction` to `CommitMintArgs`, or a
> parallel `commit_avatar_mint`).

Then `create_player_avatar` accepts that per-mint Avatar PDA as its
`avatar_asset` **unchanged** (it already validates `card_kind == Avatar`), and
`PlayerAvatar.avatar_asset` points at the player's own rolled fighter. This
**unifies P1+P2+P3**: rolled + tradeable NFT + on-chain stats + selectable via
`create_player_avatar` + equippable via `equip_item_v2` + royalty via the
existing fee split + PvP via mint-keyed `avatar_ref` (which then equals
`PlayerAvatar.avatar_asset`, closing the design loop that `pvpIx.ts:205-211`
already anticipates).

Net program change for the full end-state: **one new instruction + one enum/arg
on the commit**, ~1 handler, ~1 context. Everything else (PlayerAvatar,
EquipmentRecord, equip_item_v2, CharRecord, fee split, royalty) is already
deployed and needs only web wiring.

---

## 3. Selection + equip flow

### Fighter appears in the picker alongside the 12 free archetypes

Today `AvatarPicker.tsx` / `OnboardingOverlay.tsx` map over
`AVATAR_ARCHETYPES` (the 12). We make the picker render **`AVATAR_ARCHETYPES ∪
ownedFighters`**, where `ownedFighters: AvatarCard[]` come from the read hop
(`fighterFromMint` over `EKZAFTR` NFTs). A minted fighter is just an
`AvatarCard` (`types.ts:136`) — same shape as an archetype card, so the picker
renders it with no structural change; only the data source widens and a "Minted
· <rarity>" badge is added.

### "Set as my active fighter"

`chooseAvatar` currently keys on `archetypeId` and rebuilds from
`avatarFromArchetype` (`reducer.ts:189-214`). Extend the action to carry an
optional owned-fighter identity:

```ts
| { type: "chooseAvatar"; archetypeId: string; fighterMint?: string }
```

- If `fighterMint` is set → resolve the owned `AvatarCard` from the fighter
  roster (its stats already derived), carry over equipment, `recalculateAvatar`.
- Else → the existing archetype path (guest 12) is untouched.

Gear carry-over already works because minted fighters expose the full 7-slot
`slots` array and stats recompute from base + equipment
(`recalculateAvatar` in `equipment.ts`), exactly as the archetype swap does.

**Optional on-chain selection (Phase 2):** additionally call
`create_player_avatar { avatar_asset }` (or `customize_avatar` to swap) so the
choice is durable on-chain and PvP/`CharRecord` bind to the same
`avatar_asset`. MVP keeps selection client-local (reducer) — additive.

### Equip gear onto a minted fighter

- **MVP:** equip stays **reducer-local** (`equipSelected`/`unequipSlot` in
  `reducer.ts:466-535`) — identical to how the 12 built-ins equip today. A
  minted fighter is an `AvatarCard`; nothing special is required.
- **Phase 2 (hardening):** wire `equip_item_v2 { player_avatar,
  equipment_record, arena_item, mint, token_account, owner, slot:u8 }`
  (`handlers.rs:1640-1672`). It requires a real `PlayerAvatar` **and** that the
  signer currently holds the gear NFT (ATA amount == 1) — it does NOT escrow the
  NFT, so it's a cheap authoritative mirror of the client equip. `EquipmentRecord`
  has 7 active slots (Weapon0 Head1 Body2 Gloves3 Boots4 Amulet5 Ring6) matching
  the web slots 1:1.

### Free 12 archetypes keep working (guest play)

The archetype path in `chooseAvatar`, `baseAvatar`, `OnboardingOverlay` is
untouched — minted fighters are strictly **additive** to the roster. Guests with
no wallet still play the 12 for free.

---

## 4. Migration / compat

No breaking changes. Everything below is additive:

- **`ArenaItem` / gear mint** (`mintArenaItem.ts`, `commitRevealMint.ts`) stays
  exactly as-is — gear still mints as gear.
- **`PlayerStats` / leaderboard / PvP** untouched. PvP `avatar_ref` already
  handles real-mint identities (`pvpIx.ts:225-234`); a minted fighter slots in
  with zero PvP change and its `CharRecord` persists across swaps.
- **The 12 archetypes** are unchanged content. Old saves that reference an
  archetype re-hydrate from the catalog (`reducer.ts:426-437`) as before.
  Minted-fighter saves persist the derived `AvatarCard` (self-contained; no
  catalog dependency), so `hydratePersisted`'s "unknown archetype keeps
  persisted fields" branch already covers them.
- **The one required fix** (bug, not migration): gate `chainAssetToModifier` on
  `cardKind === "Modifier"` so Avatar cards stop leaking into the item bag (§7).
  This is compatible — there are no shipped Avatar cards in the registry yet.
- **Phase-3 program change** is a *new* instruction + additive args; existing
  `reveal_mint` / `ArenaItem` are unmodified, so already-minted gear and any P2
  fighters remain valid. A P2 fighter (client-derived) and a P3 fighter
  (on-chain-derived) can coexist because both resolve through the same
  `rollFighter(seed, faction)` contract — the seed source just moves from mint
  pubkey to the on-chain `base_stats`.

---

## 5. Economics / UX

- **Price:** reuse the existing `commit_fee_lamports` from `ArenaRegistry`
  (currently surfaced as `Mint · 0.01 SOL` in `MintCardButton.tsx`). One fee
  knob for the whole platform; no separate fighter price for MVP. If fighters
  should cost more than gear, add a `avatar_fee_multiplier` to the registry in
  Phase 3 (mirrors the existing `SCROLL_FEE_MULTIPLIER = 2` pattern) rather than
  a second fee field.
- **Rarity of fighters:** `rollFighter(seed)` uses the **same rarity ladder** as
  items (Common..Mythic, Mythic only via commit-reveal). Rarity scales the stat
  variance magnitude (a Mythic fighter rolls near the top of its faction's stat
  band + a bonus skill), giving a rare fighter concrete combat value on top of
  bragging rights.
- **Where value comes from:** the fighter is a real 1/1 SPL NFT (tradeable),
  provably rolled from ungrindable slot-hash entropy, with a scarce
  rarity+stat+skill combination. This is the P2/P3 property that the
  shared-catalog P1 path structurally cannot provide.
- **Creator royalty:** **already wired.** When the fighter mint carries the
  pack's **Stellar skin** (release + vault), `commit_mint` splits `creator_bps`
  of the fee into the release **vault** and stamps Metaplex royalties to the
  release authority (`handlers.rs:744-835`). A set author earns on every fighter
  minted from their pack — no new code, just mint fighters through the Stellar
  skin path the same universes already use. (IPFS-only skins pay platform-only,
  same as gear.)
- **UX copy:** the mint modal changes from "equippable item skin (Amulet slot)"
  to "a playable fighter — select it in the Character tab and equip gear onto
  it." The "Mint as fighter · soon" placeholder becomes the live P2 mint.

---

## 6. Anti-abuse

- **God-roll grinding:** solved by construction — fighters mint only through
  commit-reveal, whose future-slot entropy makes grinding a Mythic fighter cost
  ~1000× the fee, identical to items.
- **Stat spoofing (P2 window):** in MVP, stats are client-derived, so a modified
  client could publish inflated PvP stats. This is **already the PvP trust model
  today** — `publish_snapshot` explicitly trusts client-captured stats
  (`arena-leaderboard/handlers.rs:161-162`), and the 12 built-ins have the same
  exposure. Phase 3 (`reveal_avatar_mint`) makes `base_stats` on-chain and lets
  the resolver cross-verify, closing this for fighters ahead of gear.
- **Fake-fighter NFTs (symbol spoof):** a hand-crafted NFT could claim the
  `EKZAFTR` symbol to appear in the roster. Harmless on devnet (no economic
  effect beyond a cosmetic roster entry); Phase 3's `card_kind == Avatar` PDA
  check makes the on-chain kind the authority and retires the symbol heuristic.
- **Equip integrity:** `equip_item_v2` already enforces holder ownership (ATA
  amount == 1), slot/base-type match, and the avatar's `slot_mask`
  (`handlers.rs:1640-1672`); enhancement guards that equipped items can't be
  gambled (`require_item_not_equipped`). Nothing new required.

---

## 7. The required bug fix (ship regardless of path)

`ekza-stellar-sdk/src/assets/resolveSkin.ts` + its callers
(`loadArenaSkinItems` in `src/chain/index.ts:29-43`) must skip Avatar cards:

```ts
// index.ts loadArenaSkinItems — fold ONLY Modifier cards into the item bag.
.filter((entry) => entry.data.cardKind === "Modifier")
.map((entry) => chainAssetToModifier(entry.data, owner, skinOptions))
```

and a sibling `chainAssetToFighter(asset, owner)` folds `cardKind === "Avatar"`
cards into `AvatarCard`s for the roster (Phase 2, when on-chain Avatar cards
exist). Without this gate, any registered Avatar card mis-renders as gear.

---

## 8. Phased plan

### MVP (P2, zero program change) — "buy a fighter, play as it, equip gear"

1. **`rollFighter(seed, faction)`** in `domain/ekza` — pure, deterministic,
   mirrors `rollItem`; emits `{ baseStats, skillIds, rarity, element }`.
   `FACTION_BASE_VECTOR` + variance→rarity ladder authored here. Unit-tested
   with fixed-seed vectors (parity file, like `gen-pvp-sim-vectors.ts`).
2. **`fighterFromMint(mint, faction, ipfsCid, name)` → `AvatarCard`** — wraps
   `rollFighter(mint-as-seed)` into a playable, self-contained `AvatarCard`.
3. **Fighter mint** — a `commitRevealMint`-based path with symbol `EKZAFTR`, the
   avatar portrait as the IPFS/Stellar skin, `baseType: Charm` inert carrier.
   Reuses the entire existing commit-reveal + royalty machinery.
4. **Read hop** — route owned NFTs by symbol: `EKZAFTR` → fighter roster,
   else → item bag. Apply the §7 `cardKind` gate.
5. **Picker + reducer** — render `AVATAR_ARCHETYPES ∪ ownedFighters`; extend
   `chooseAvatar` with `fighterMint?`. Equip stays reducer-local.
6. **UX** — flip "Mint as fighter · soon" to live; update mint-modal copy.

Result: a wallet buys a fighter (real rolled NFT, creator paid), it shows in the
picker, plays in PvE + PvP (CharRecord auto-keys on its mint), and equips gear —
**with no Solana program change**.

### Phase 2 — on-chain durability (still no program change)

7. Wire `create_player_avatar` / `customize_avatar` so "active fighter" is a
   durable on-chain `PlayerAvatar` (for the built-ins, register their authored
   defs once via `register_arena_asset(Avatar)`; P1 catalog path).
8. Wire `equip_item_v2` / `unequip_item_v2` as an authoritative mirror of the
   client equip.
9. `chainAssetToFighter` surfaces on-chain Avatar cards in the roster.

### Phase 3 — on-chain rolled fighters (the one program change)

10. Add `commit_avatar_mint` (or `card_kind`+`faction` on `CommitMintArgs`) and
    **`reveal_avatar_mint`** → writes a per-mint `ArenaAssetData{Avatar}` with
    rolled `base_stats`/`skill_ids`/`rarity` (§2). `PlayerAvatar.avatar_asset`
    then points at the player's own rolled fighter; the sim/PvP can
    cross-verify against on-chain stats. Retire the `EKZAFTR` symbol heuristic
    in favor of `card_kind`.

---

## 9. Scaffold-spec (interfaces / pseudocode only — no logic)

### 9.1 TypeScript — pure domain (`domain/ekza`)

```ts
// domain/ekza/content/fighters.ts  (NEW — mirrors content/factions.ts anchors)

/** Full base band per faction (pre-variance). Anchored on the 12 built-ins'
 *  spreads in content/avatars.ts + FACTION_BONUSES in content/factions.ts. */
export const FACTION_BASE_VECTOR: Record<Faction, Stats> = {
  Moss:  { hp: 11, attack: 1, armor: 1, speed: 0 }, // tank/sustain
  Spark: { hp: 9,  attack: 3, armor: 0, speed: 1 }, // burst
  Void:  { hp: 9,  attack: 2, armor: 0, speed: 2 }, // tempo/speed
  Stone: { hp: 11, attack: 2, armor: 1, speed: 0 }, // armor/counter
};

/** Default skill kit per faction, from the 12 built-ins' usage. */
export const FACTION_DEFAULT_SKILLS: Record<Faction, string[]> = {
  Moss:  ["moss_skin"],
  Spark: ["fire_opener"],
  Void:  ["quickstep"],
  Stone: ["stone_oath"],
};

export type RolledFighter = {
  baseStats: Stats;
  skillIds: string[];
  rarity: Rarity;
  element?: Element;
};

/** Deterministic fighter roll — the avatar analogue of rollItem(seed, baseType).
 *  Same seed → same fighter on client AND (Phase 3) on-chain.
 *  TODO(impl): rarity ← ladder(seed); variance magnitude scales with rarity;
 *  high rolls append a faction-appropriate bonus skill (e.g. jewelry_focus). */
export declare function rollFighter(seed: bigint, faction: Faction): RolledFighter;

// domain/ekza/content/avatars.ts (extend) — build a playable card from a mint.
/** TODO(impl): seed = fnv/splitmix over the base58 mint; fold rollFighter into
 *  a self-contained AvatarCard (mint=nftMint, owner, totalStats=baseStats,
 *  equipment:{}, version:1). No catalog dependency (survives persistence). */
export declare function fighterFromMint(args: {
  mint: string; owner: string; faction: Faction; ipfsCid: string; name: string;
}): AvatarCard;
```

```ts
// reducer.ts (extend the action only — no new logic shown)
type GameAction =
  | { type: "chooseAvatar"; archetypeId: string; fighterMint?: string }
  // ...existing actions unchanged
```

### 9.2 TypeScript — chain layer (`lib/chain`, offline-gated like siblings)

```ts
// lib/chain/mintFighter.ts (NEW — thin wrapper over commitRevealMint)
export const FIGHTER_SYMBOL = "EKZAFTR"; // vs ARENA_ITEM_SYMBOL "EKZAITEM"

/** TODO(impl): call commitRevealMint with symbol=FIGHTER_SYMBOL,
 *  baseType:"Charm" (inert carrier), skin = the avatar portrait
 *  (Ipfs cid or Stellar release/vault for creator royalty), name = fighter name.
 *  Returns the same CommitRevealMintResult; the mint pubkey is the roll seed. */
export declare function mintFighter(params: {
  connection; wallet; faction: Faction; skin: MintSkinSelection; name: string;
}): Promise<CommitRevealMintResult>;

// ekza-stellar-sdk/src/assets/resolveSkin.ts (NEW sibling of chainAssetToModifier)
/** TODO(impl): fold a cardKind==="Avatar" ArenaAssetData into an AvatarCard by
 *  reading base_stats/skill_ids/skin_ref directly (NOT re-rolling item affixes). */
export declare function chainAssetToFighter(
  asset: ArenaAssetData, owner: string, options?: ResolveSkinOptions
): AvatarCard;
```

### 9.3 Rust — Phase-3 program change (clearly-marked TODO stubs)

```rust
// programs/solana-ekza-arena/src/state.rs  (extend — cargo-shaped stubs)

/// Faction identity for a rolled avatar (mirrors web Faction).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum ArenaFaction { Moss, Spark, Void, Stone }
// TODO(impl): impl ArenaFaction { fn base_vector(&self) -> ArenaStats; fn default_skills(&self) -> Vec<String>; }

/// Extend the commit args so the reveal knows it is rolling an Avatar, not gear.
/// TODO(impl): add these two fields to the EXISTING CommitMintArgs
/// (state.rs:641), OR introduce a parallel CommitAvatarMintArgs. Additive.
// pub card_kind: ArenaCardKind,
// pub faction:   ArenaFaction,

// programs/solana-ekza-arena/src/constants.rs
// TODO(impl): pub const ARENA_AVATAR_SEED: &[u8] = b"arena_avatar_v1";
```

```rust
// programs/solana-ekza-arena/src/contexts.rs  (NEW — mirrors RevealMint)
// TODO(impl): #[derive(Accounts)] pub struct RevealAvatarMint<'info> {
//   registry, mint_commit (close = minter), mint (init NFT),
//   avatar_asset: Account<'info, ArenaAssetData> @ seeds [ARENA_AVATAR_SEED, mint],
//   minter_token_account, minter (Signer), slot_hashes,
//   metadata_account, master_edition, token/ata/system/rent,
//   optional stellar_* for royalty (same as RevealMint). }

// programs/solana-ekza-arena/src/handlers.rs  (NEW — no body; contract only)
// TODO(impl): pub fn reveal_avatar_mint(ctx, nonce) -> Result<()> {
//   // reuse reveal_mint's slot-hash read + splitmix64 seed + Metaplex CPI;
//   // instead of ArenaItem, write ArenaAssetData {
//   //   card_kind: Avatar,
//   //   base_stats: faction.base_vector() ⊕ roll_variance(seed),
//   //   skill_ids:  faction.default_skills() (+ bonus on high roll),
//   //   rarity:     roll_rarity(seed),        // Mythic-capable (commit-reveal)
//   //   element:    ArenaElement::None,       // MVP
//   //   skin_ref:   ItemSkin::Ipfs(cid) | StellarAsset(..),
//   //   archetype_id: "<universe>:<avatarId>",
//   //   creator: committed pack author,
//   // } keyed by [ARENA_AVATAR_SEED, mint].
//   //   Fee already charged at commit; royalty already stamped. }
```

> With `reveal_avatar_mint`, the existing `create_player_avatar` accepts the
> per-mint Avatar PDA as `avatar_asset` **unchanged** (it already checks
> `card_kind == Avatar`), so `PlayerAvatar.avatar_asset` becomes the player's
> own rolled fighter and equals the PvP `CharRecord.avatar_ref` — closing the
> loop with no further program change.

---

## 10. TL;DR

- **Stats/skills** come from a **faction base vector + seed-rolled variance +
  faction skill kit** (`rollFighter(seed, faction)`), the avatar analogue of the
  item roll. It emits exactly the `CombatantSnapshot` the sim eats and needs only
  `faction` (which the manifest has). The 12 built-ins keep their authored defs.
- **On-chain model:** the schema already models avatars-as-characters
  (`ArenaCardKind::Avatar`, `PlayerAvatar`, `create_player_avatar`,
  `equip_item_v2`) — the web client just never wired it and routed avatars
  through the item mint.
- **Minimal change / zero-change MVP:** **ZERO program change.** Mint the fighter
  as a real rolled NFT through the existing commit-reveal path (creator royalty
  already fires), derive its combat stats client-side from the mint seed +
  faction, soft-flag it with symbol `EKZAFTR`, surface it in the picker, equip
  client-side. PvP + royalties work unchanged.
- **One program change, later:** `reveal_avatar_mint` writes a per-mint
  `ArenaAssetData{Avatar}` with rolled `base_stats`, making stats on-chain truth
  and unifying rolled + owned NFT + on-chain-selectable + equippable.
- **Required fix now:** gate `chainAssetToModifier` on `cardKind==="Modifier"` so
  Avatar cards stop mis-surfacing as gear.
- **Phasing:** MVP (client-derived rolled fighter NFT, no program change) →
  Phase 2 (wire the already-deployed PlayerAvatar/equip_item_v2 for on-chain
  durability, no program change) → Phase 3 (the single `reveal_avatar_mint`
  instruction for on-chain rolled stats).
