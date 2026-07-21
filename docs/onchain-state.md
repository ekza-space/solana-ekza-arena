# On-chain state guarantees (Ekza Arena)

This document pins down the three properties the owner asked to verify, each tied
to the exact instruction that writes the state and the passing test that proves
it. Two programs are involved:

- **`solana_ekza_arena`** (`D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ`) —
  the registry: item NFTs, avatars, equipment.
- **`arena_leaderboard`** (`9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU`) — a
  SEPARATE program: per-player battle stats + the top-N min-heap.

All file/line references are as of this commit.

---

## (a) Equipment lives on-chain — equipping = strengthening the character on-chain

**Where it lives.** Each avatar has an `EquipmentRecord` PDA seeded
`["equipment", player_avatar]` (`programs/solana-ekza-arena/src/constants.rs:21`,
state in `state.rs`). It holds the avatar's full equipped set: 7 active named
slots (Weapon, Head, Body, Gloves, Boots, Amulet, Ring) plus reserved slots.

**What writes it.** `equip_item_v2(slot)`
(`programs/solana-ekza-arena/src/lib.rs:95`, handler in `handlers.rs` around
`pub fn equip_item_v2`) writes `record.slots[slot] = mint` — the equipped item's
real NFT mint is stored on chain, so the character's loadout is on-chain state,
not a client-side cosmetic. The handler is **holder-gated**: it requires the
owner's token account to actually hold the mint being equipped, and enforces the
item's base-type ↔ slot compatibility and the avatar's `slot_mask`. So "equipped"
provably means "this wallet holds this NFT and has bound it to this slot on
chain" — i.e. equipping strengthens the character in verifiable on-chain state.

**Tests that prove it** (`tests/solana-ekza-arena.ts`):

- `:1276` — *"equips the full 7-slot set (Armor→Body/Gloves/Boots,
  Charm→Amulet/Ring)"* — writes all 7 slots into the `EquipmentRecord` PDA and
  reads them back.
- `:1378` — *"rejects v2 equip after the NFT was traded away (holder rule)"* —
  once the NFT moves to another wallet, the prior owner can no longer equip it,
  proving the equip binding tracks real on-chain ownership.
- `:1408` — *"unequips a v2 slot (record + legacy mirror both clear)"*.
- Legacy 4-slot path (still on-chain): `:1076` *"equips an owned item NFT into
  its base-type slot"*, `:1098` *"rejects equip after the NFT was traded away
  (holder rule)"*.

**Web read path.** `ekza-arena-web/src/lib/chain/web3/programAccounts.ts`
(`equipmentRecordPda`, `fetchEquippedSet`) reads the `EquipmentRecord` PDA to show
"what is equipped = what counts in the fight".

---

## (b) Items are real, tradeable NFTs

**What they are.** Items are minted as genuine SPL-token NFTs with Metaplex
metadata (`mint_arena_item` / the commit-reveal mint path in `lib.rs`/`handlers.rs`,
spec §11), 1:1 with an `ArenaItem` PDA (`["arena_item_v1", mint]`). Because they
are ordinary NFTs, they transfer with any standard SPL transfer / marketplace —
the program does not lock them.

**Tests that prove it** (`tests/solana-ekza-arena.ts`):

- `:518` — *"mints a real tradeable item NFT with rolled affixes (builtin skin,
  spec §11)"* — asserts a real mint + token account + metadata.
- `:622` — **the tradeability proof** — *"TRADEABILITY: transfers the item NFT to
  a second wallet then scraps by the new owner (spec §11.3/§11.5)"*. It does a
  plain SPL token transfer of the item NFT to a **second wallet**, then has that
  **new owner** scrap it — proving the item is a bearer NFT whose control (equip,
  scrap, trade) follows the actual token holder, not the original minter.

**Sink.** `scrap_arena_item` (`lib.rs:43`) burns the NFT (token + metadata/edition)
and closes the `ArenaItem` PDA, returning rent to the current holder — holder-gated,
so only the present owner can scrap.

---

## (c) Battle stats + leaderboard rating live on-chain

**Where it lives.** In `arena_leaderboard`:

- `PlayerStats` PDA `["player_stats_v1", player]` — `wins`, `losses`, `games`,
  `streak`, `best_streak`, and an elo-lite `rating` (starts 1000, floor 0)
  (`programs/arena-leaderboard/src/state.rs`).
- `Leaderboard` — a zero-copy binary **MIN-heap** of the top-N `(player, rating,
  wins)` entries; the root is the weakest of the top (auto-eviction design).

**What writes it.** `record_battle(win, opponent_is_bot)`
(`programs/arena-leaderboard/src/handlers.rs`, `lib.rs:55`) updates the tally +
streaks, applies the elo-lite delta (+25/-20 vs player, +10/-15 vs bot, floored
at 0), and `upsert`s the player into the min-heap. It is signed by the player
wallet **or** its registered session (burner) key — the web app's smooth,
popup-free flow. `set_profile(name, uri)` stores a display name + link as a
**top-list perk** (rejected with `NotInTopList` for non-members).

**Anchor tests that prove it** (`tests/arena-leaderboard.ts`, all passing):

- `:205` — *"records battles: stats, streaks and elo-lite rating math"* — exact
  rating/wins/losses/streak arithmetic (p1 → 1020).
- `:237` — *"fills the board and keeps the min-heap invariant (root = weakest)"*.
- `:257` — *"evicts the weakest player when a stronger one arrives (board full)"*.
- `:291` — *"registers a session key and records battles with it (soft
  auto-confirm)"* — the burner records for the wallet with no wallet signature.
- `:343` / `:363` — `set_profile` allowed for a top-list member, rejected
  (`NotInTopList`) for an evicted player.
- `:393` — *"rating never drops below the floor (0)"*.

**Live end-to-end proof.** `scripts/validate-leaderboard.ts`
(`yarn validate:leaderboard`) exercises the FULL loop against a running localnet
**through the web app's own instruction builders**
(`ekza-arena-web/src/lib/chain/leaderboardIx.ts`, imported by the script): create
board → register session key → record battles (burner-signed for the hero) →
assert the hero lands in the top list at the exact predicted rating/wins → assert
eviction + the min-heap invariant after every op → `set_profile` and read the
name/link back. It prints `PASS` with the on-chain addresses. Latest run:

```
[1] board created + init (capacity 3)                 PASS
[2] hero registers a session (burner) key             PASS
[3] battles recorded via the web ix builders          PASS
    hero rating == 1030, wins == 3, in top list
[4] stronger challenger evicts the weakest incumbent  PASS
[5] set_profile writes + reads back name + link       PASS
==================== BATTLE→LEADERBOARD: PASS ====================
```

---

## How to reproduce the live proof

```bash
# 1. Bring up a bare localnet with just the leaderboard program:
solana-test-validator --reset \
  --bpf-program 9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU \
  target/deploy/arena_leaderboard.so

# 2. In another shell, run the proof:
RPC_URL=http://127.0.0.1:8899 yarn validate:leaderboard
```

(The full arena registry proofs run under `anchor test`, which loads both
programs + the vendored Metaplex Token Metadata program per `Anchor.toml`.)
