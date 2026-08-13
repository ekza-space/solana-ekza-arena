# Ekza Arena — Async PvP Ladder + Per-Character History (Design + Scaffold-Spec)

Status: DESIGN + SCAFFOLD-SPEC. No Rust/web implemented here. Devnet only. No deploy.
Scope: two features — (1) real asynchronous PvP ladder **without stakes/wagering**,
(2) per-character win/loss history. Both are strict, additive extensions of the
mechanics that already ship.

---

## 0. The insight, validated

The arena already fights against **published opponent snapshots**. The 5 PvE
"bots" are hand-authored snapshots:

- `src/domain/ekza/content/opponents.ts:41` — `PVE_OPPONENTS` is 5 fixed
  `{ avatar, rating, rewards, counterplay }` entries. Each `avatar` is a plain
  `AvatarCard` (`content/avatars.ts:392 trainingOpponent`, `arenaOpponent`).
- `src/components/arena/OpponentRoster.tsx:12` renders that static array; a fight
  turns the chosen opponent into a `CombatantSnapshot` via
  `opponentSnapshot()` (`combat.ts:33`).
- The fight itself is `resolveBattle(avatarA, avatarB, nonce)` (`combat.ts:42`),
  **pure and deterministic** in exactly `(snapshotA, snapshotB, nonce)`.
- Server-authoritative path `/api/battle` (`app/api/battle/route.ts`) rolls an
  honest nonce and returns `{ result, serverSeed, resultHash }`; the client
  re-runs the same resolver to verify (`domain/ekza/serverBattle.ts:91`).
- The result is written to ONE wallet's `PlayerStats` via
  `recordBattleResult` (`lib/chain/leaderboardClient.ts:273`) →
  `record_battle(win, opponent_is_bot)`
  (`arena-leaderboard/src/handlers.rs:65`), with `opponentIsBot` **hardcoded
  true** at `leaderboardIx.ts:252 deriveBattleOutcome`.

**Conclusion:** real async PvP = let a player **publish their own build as an
on-chain snapshot**, draw the opponent roster from **other players' live
snapshots** ("ghosts", à la Marvel Snap / auto-battlers), resolve
deterministically, and record the result on-chain **for both players**. Nothing
about the sim, the roster shape, or the record flow needs reinventing — only
extending. There is **no wager**: the only thing at stake is ladder rating and
per-character W/L, exactly as PvE already is.

---

## 1. THE CRUX — can the sim run on-chain to make resolve trustless?

**Yes. Run it on-chain. This is the chosen trust model.** The sim is cheap
enough that the program itself is the oracle: it recomputes the winner from the
two published snapshots + a slot-hash-derived seed. Nobody submits a result, so
there is no "winner only reports wins they like" problem — the outcome is
*computed*, not *asserted*.

### Why the sim is trivially portable to Rust/BPF

Read `combat.ts:42-200`:

- **Bounded loop:** `for round in 1..=10` — hard cap 10 rounds, two `strike()`
  calls + one end-of-round recovery per round. Worst case ≈ 20 strikes.
- **Pure integer arithmetic:** every op is `+ - * max min` on small ints
  (`strike()` `combat.ts:260`, recovery `combat.ts:349`). No floats, no
  allocation in the hot path.
- **Fixed, tiny skill set:** 7 skills total (`content/skills.ts:15`):
  `moss_skin, stone_oath, fire_opener, glass_cannon, heavy_guard, quickstep,
  jewelry_focus`. Fits a **`u8` bitmask**. All magnitudes are `COMBAT_SKILL_VALUES`
  constants (`skills.ts:3`), portable verbatim.
- **The only "hash":** one 64-bit FNV-1a used purely for tie-breaks
  (`deterministicTieBreak` `combat.ts:497`), invoked for the initiative tie
  (`firstAttackerIsA` `combat.ts:248`) and the final HP+ATK tie
  (`combat.ts:179`). Re-implemented in Rust in a dozen lines.
- Winner rule: remaining HP → effective ATK → FNV tie-break (`combat.ts:179`).

**Compute budget:** a few hundred integer ops + one short FNV loop. Default CU
limit is 200k (raisable to 1.4M); this is comfortably inside the default with
room for the account I/O. This is **cheaper than the mint/enhance rolls already
on-chain** (`solana-ekza-arena/handlers.rs` splitmix64 + affix generation).

### What must be canonicalized (parity requirement)

The TS resolver feeds combatant **identity strings** (`snapshot.mint`,
`\`${avatarB.mint}:${round}\``) into the FNV tie-break. The on-chain resolver
must use a stable identity too. Rule:

- On-chain identity = the **`ArenaSnapshot` account pubkey** (32 bytes), fed into
  the same FNV-1a byte-for-byte.
- The web **preview** sim for a ghost fight MUST set `CombatantSnapshot.mint` to
  the ghost's snapshot pubkey (base58) and derive `nonce` with the same
  `splitmix64(slothash ^ ...)` formula, so the preview matches the chain
  bit-for-bit. Tie-breaks are rare (require exact HP *and* exact ATK equality)
  but for provable-fairness parity the encoding must agree. **This is the single
  correctness gotcha of the on-chain port** — call it out in the port's test
  vectors.

### The one integrity boundary the on-chain sim does NOT close

On-chain re-resolution proves the *winner is correct for the two snapshots*. It
does **not** by itself prove the snapshots' **stats are legitimate** (a client
could publish an inflated stat vector). That is a *publish-time* integrity
question, addressed in §4/§6, not a resolve-time one. Note this is **no weaker
than today**: `/api/battle` already trusts client-supplied snapshots for PvE.

### Cheapest alternative if we ever refuse the on-chain sim

For the record, ranked from best:

1. **On-chain re-resolution (CHOSEN).** Trustless resolve, no submitter.
2. **Slot-hash-committed + optimistic + fraud proof.** Off-chain resolver posts
   `winner + resultHash`; a challenge window lets anyone submit a one-tx
   on-chain replay that slashes a liar. More moving parts, needs bonds.
3. **Trusted attestor (arena server signs the result).** Simple, but
   reintroduces a trusted party and a key to protect. Only acceptable as a
   stop-gap.

Since (1) is *cheaper to build than (2)* and strictly more trustless than (3),
we build (1).

---

## 2. On-chain changes (all in the `arena-leaderboard` program)

We keep PvP inside `arena-leaderboard` because it already owns `PlayerStats` +
rating + the heap. Snapshots store a **self-contained, engine-ready** stat
vector + skill mask captured at publish, so `resolve_challenge` needs **no CPI**
into `solana-ekza-arena` for the MVP. `record_battle` / `PlayerStats` /
`Leaderboard` are **untouched** (PvE bots keep using them).

### 2.1 New PDA: `ArenaSnapshot` — a player's published build ("ghost")

Seeds: `["arena_snapshot_v1", owner]` (one live snapshot per wallet; matches
today's one-`PlayerAvatar`-per-wallet reality, `solana-ekza-arena/state.rs:496`).
Republishing overwrites in place.

```rust
// SCAFFOLD STUB — no handler logic. Layout only.
#[account]
pub struct ArenaSnapshot {
    pub owner: Pubkey,          // PDA seed; the ghost's wallet
    pub avatar_ref: Pubkey,     // ArenaAssetData avatar-card pubkey (== PlayerAvatar.avatar_asset).
                                // Forward-compat: becomes the avatar NFT mint when avatars are NFTs.
    pub archetype_id: [u8; 32], // display handle, utf-8 zero-padded (e.g. "ember_witch")
    pub stats: ArenaStatsLite,  // ENGINE-READY total stats (base + equipment folded), captured at publish
    pub skill_mask: u8,         // bit N set => skill N active (see §2.6 canonical skill order)
    pub element: u8,            // 0 None, 1 Fire, ... (mirrors ArenaElement)
    pub skin_ref: [u8; 32],     // cosmetic ref (avatar skin) — display only, ignored by resolve
    pub rating_at_publish: i32, // owner rating snapshot at publish time (matchmaking hint only)
    pub published_slot: u64,    // freshness
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct ArenaStatsLite { pub hp: i16, pub attack: i16, pub armor: i16, pub speed: i16 }
```

Notes:
- `stats` is the **fold already computed client-side** for the live fight
  (`avatar.totalStats`, i.e. base + equipment deltas). Publishing captures it so
  resolve reads a flat vector — the same data `opponentSnapshot()` uses today.
- `avatar_ref` is the **per-character key** (see §3), so a snapshot already
  carries the character identity its record accrues to.

### 2.2 New PDA: `Challenge` — a commit to fight a specific ghost at a future slot

Seeds: `["challenge_v1", challenger, nonce]` (nonce lets a wallet have several
open challenges), same shape as `MintCommit` (`solana-ekza-arena/state.rs:657`).

```rust
// SCAFFOLD STUB.
#[account]
pub struct Challenge {
    pub challenger: Pubkey,        // wallet issuing the challenge (its ArenaSnapshot fights)
    pub nonce: u64,                // PDA seed component
    pub opponent_snapshot: Pubkey, // the ghost ArenaSnapshot chosen AT COMMIT (locks the pairing)
    pub target_slot: u64,          // slot + REVEAL_DELAY_SLOTS; its hash seeds the fight
    pub bump: u8,
}
```

Locking `opponent_snapshot` **at commit** means the challenger cannot re-pick the
opponent after the seed is knowable. Locking `target_slot` to a *future* slot
means the seed is unknown at commit (revert-grind resistant), exactly like the
mint path.

### 2.3 New PDA: `CharRecord` — per-character W/L (see §3)

Seeds: `["char_record_v1", owner, avatar_ref]`.

### 2.4 New PDA: `PairCooldown` — anti-farming pair throttle (see §4)

Seeds: `["pair_cd_v1", key_lo, key_hi]` where `(key_lo, key_hi) =
sort(challenger, opponent_owner)` so the pair is order-independent.

```rust
// SCAFFOLD STUB.
#[account]
pub struct PairCooldown {
    pub key_lo: Pubkey,
    pub key_hi: Pubkey,
    pub last_ranked_slot: u64,  // last slot this pair produced a RATED result
    pub ranked_today: u16,      // per-UTC-day rated count for this pair
    pub utc_day: i64,
    pub bump: u8,
}
```

### 2.5 New instructions

| Instruction | Signer | Writes | Purpose |
|---|---|---|---|
| `publish_snapshot(args)` | owner (wallet, or session key*) | `ArenaSnapshot` (init_if_needed) | capture current build as a ghost |
| `unpublish_snapshot()` | owner (wallet) | close `ArenaSnapshot` | leave the ghost pool (rent → owner) |
| `commit_challenge(nonce, args)` | challenger (wallet or session key) | `Challenge` (init) | lock opponent + future slot; pay rent/fee |
| `resolve_challenge()` | **permissionless** | both `PlayerStats`, both `CharRecord`, `PairCooldown`, `Leaderboard`, close `Challenge` | run on-chain sim, dual-record |
| `close_expired_challenge()` | permissionless | close `Challenge` | reclaim rent if the slot hash aged out unresolved |

`*` session-key signing of `publish_snapshot` is optional MVP polish; the
wallet-signed path is enough for v1.

#### The resolve is permissionless — this is the anti-loss-dodge mechanism

Once `target_slot` passes, the outcome is deterministic and the slot hash is
public (~512-slot window in `SlotHashes`). A cheating challenger could compute
the result locally and **withhold a losing reveal**. We defeat this by making
`resolve_challenge` **callable by anyone**: the defender, a keeper bot, or the
challenger — all produce the identical result (the program computes it). The
challenger gains nothing by withholding because someone else pushes it. Combine
with a reveal window `< 512 slots` and (hardening) a keeper that sweeps open
challenges. Net: **you cannot dodge a loss.**

### 2.6 On-chain sim contract (the Rust port of `resolveBattle`)

```
// SCAFFOLD PSEUDOCODE — port of combat.ts:42, no code here.
canonical skill bit order (u8 mask):
  0 moss_skin  1 stone_oath  2 fire_opener  3 glass_cannon
  4 heavy_guard 5 quickstep  6 jewelry_focus   (bit 7 reserved)

fn resolve_onchain(a: &Snapshot, b: &Snapshot, nonce: u64) -> WinnerIsA(bool):
    // effective stats: combat.ts effectiveAttack/effectiveMaxHp/maxMana/effectiveInitiative
    // loop rounds 1..=10: firstAttackerIsA (init, then FNV tie), strike x2, break on <=0,
    //   end-of-round moss/jewelry recovery. All from COMBAT_SKILL_VALUES (skills.ts:3).
    // winner: hp -> effective attack -> FNV-1a(snapshotPubkeyA ++ snapshotPubkeyB, nonce)
    // IDENTITY = ArenaSnapshot pubkey bytes (see §1 parity requirement).

fn seed(target_slot_hash: u64, challenger: Pubkey, opp_snapshot: Pubkey, commit_nonce: u64) -> u64:
    splitmix64_mix(target_slot_hash
        ^ first8(challenger) ^ first8(opp_snapshot) ^ commit_nonce)   // mirrors handlers.rs mint seed
```

The Rust port ships with a **cross-impl test vector suite**: N random snapshot
pairs + nonces, asserted equal between `combat.ts resolveBattle` and the Rust
`resolve_onchain` (extend the existing `validate-leaderboard.ts` proof harness).

### 2.7 `resolve_challenge` account list (the heavy tx)

```
resolve_challenge accounts:
  challenge            (mut, close = anyone_rent_dest? -> challenger)  [Challenge PDA]
  challenger_snapshot  (read)   ["arena_snapshot_v1", challenge.challenger]
  opponent_snapshot    (read)   = challenge.opponent_snapshot
  challenger_stats     (mut)    ["player_stats_v1", challenge.challenger]      (init_if_needed)
  opponent_stats       (mut)    ["player_stats_v1", opponent_snapshot.owner]  (init_if_needed)
  challenger_char      (mut)    ["char_record_v1", challenger, challenger_snapshot.avatar_ref]
  opponent_char        (mut)    ["char_record_v1", opp_owner, opponent_snapshot.avatar_ref]
  pair_cooldown        (mut)    ["pair_cd_v1", sort(challenger, opp_owner)]    (init_if_needed)
  leaderboard          (mut)    the board account
  slot_hashes          (read)   SysvarS1otHashes111...
  payer                (mut, signer)   pays init_if_needed rents; ANY key (permissionless)
  system_program
```

~11 accounts — well within the 64-account tx limit. `payer` is the only signer;
it is **not** trusted for anything (it just funds lazy inits and pushes the tx).
Writing `opponent_stats`/`opponent_char` for a wallet that did not sign is safe:
their PDAs are program-derived, the values are *computed by the program*, and the
opponent's own snapshot (which they signed at publish) is the only opponent input.

---

## 3. Per-character history

**Key: `["char_record_v1", owner, avatar_ref]`.** `avatar_ref` = the
`ArenaAssetData` avatar-card pubkey the character is built on
(`PlayerAvatar.avatar_asset`, `solana-ekza-arena/state.rs:499`).

Why this key, not `(wallet, archetype_id string)` and not a `PlayerAvatar`
counter:
- A wallet holds one `PlayerAvatar` at a time but can **swap** its `avatar_asset`
  (`customize_avatar`, `state.rs:533`). Keying by `(owner, avatar_ref)` means a
  character's history **persists across swaps** — main `ember_witch`, leave it,
  come back, its W/L is still there.
- `avatar_ref` is already a 32-byte on-chain pubkey — a clean PDA seed (a 64-byte
  `archetype_id` string would need hashing).
- **Forward-compat / NFT value:** when avatars become their own NFTs,
  `avatar_ref` becomes the avatar mint with zero migration. Per-character
  provenance ("this specific avatar is 240-31 on the ranked ladder") is exactly
  the signal that raises a tradeable character's worth later.

```rust
// SCAFFOLD STUB.
#[account]
pub struct CharRecord {
    pub owner: Pubkey,
    pub avatar_ref: Pubkey,   // PDA seed
    pub wins: u32,
    pub losses: u32,
    pub games: u32,
    pub streak: u16,
    pub best_streak: u16,
    pub last_played_slot: u64,
    pub bump: u8,
}
```

- **Written** in `resolve_challenge` for *both* characters (winner: wins++,
  streak++, best_streak=max; loser: losses++, streak=0), init_if_needed.
- **Read** in the UI on every character card (loadout screen + ghost roster
  cards): `W–L · streak`. Client derives the PDA from
  `(owner, avatar_ref)` and decodes it (mirror the `fetchPlayerStats` reader in
  `lib/chain/leaderboard.ts`).
- MVP: PvP-only records. (Optionally also write it from the PvE `record_battle`
  path later, but that touches the existing instruction — defer to keep §5's
  no-breaking-change guarantee.)

---

## 4. Anti-abuse

The biggest structural fix: **PvP rating must use real (opponent-scaled) elo**,
not the fixed `+25/-20` deltas `record_battle` uses
(`constants.rs:38`). Fixed deltas make "beat a weak alt for +25" profitable.
Scaled elo makes beating a much weaker opponent worth ≈0 and makes self-play
between two alts ≈ zero-sum (no net inflation).

```
// PvP delta (NEW code path, writes the SAME PlayerStats.rating field):
expected_a = 1 / (1 + 10^((rating_b - rating_a)/400))     // fixed-point on-chain
delta_a    = round(K * (score_a - expected_a))            // K≈24, score∈{0,1}
delta_b    = -delta_a-ish (compute symmetrically)         // floored at RATING_FLOOR
```

| Vector | Mitigation |
|---|---|
| **Self-play** (two wallets farming each other) | `PairCooldown`: a given pair yields a **rated** result at most once per cooldown / M per UTC-day; extra fights resolve as **exhibitions** (W/L + CharRecord update but **no rating change**, or skip both — MVP: no-rating). Combined with scaled elo, a farm loop nets ≈0 rating. |
| **Sybil rating inflation** | Scaled elo is ~zero-sum → minting alts cannot manufacture rating. **Min-games gate**: a wallet is hidden from the *ranked ladder heap* / matchmaking pool until `games >= MIN_RANKED_GAMES`. Commit rent + (hardening) a small anti-sybil deposit raise the cost of alt fleets. |
| **Picking weak opponents to farm rating** | Scaled elo: crushing a low-rated ghost pays ≈0; losing to it costs a lot. Matchmaking (§5) samples a **rating band** around the challenger so the roster mostly offers fair fights anyway. |
| **Loss-dodging** (withholding a losing reveal) | **Permissionless `resolve_challenge`** (§2.5) + short reveal window + (hardening) keeper sweep. You cannot avoid a computed loss. |
| **Snapshot staleness gaming** | `published_slot` on every snapshot; matchmaking prefers fresh (`< STALE_SLOTS`); the client nudges "republish" when the live build differs from the published one. A stale ghost simply fights with its old build (acceptable — it *is* a ghost of that build). |
| **Inflated published stats** | MVP (devnet): trust the captured client fold (no weaker than today's `/api/battle`). **Hardening**: `publish_snapshot` CPI-verifies stats against `PlayerAvatar` + `EquipmentRecord` + per-item `ArenaItem` **holder checks** (owner must currently hold each equipped mint — the exact rule already documented at `solana-ekza-arena/state.rs:486`). |
| **Same-slot / batch replay** | Reuse the existing per-player throttle intuition; `commit_challenge` + future-slot `resolve` already spread a fight across ≥`REVEAL_DELAY_SLOTS`. Optionally consume the existing `BattleRateLimit` daily cap on the challenger. |

---

## 5. Migration / compatibility

**No breaking changes. New PDAs + new instructions only.**

- `record_battle`, `PlayerStats`, `BattleRateLimit`, `Leaderboard`,
  `register_session_key`, `set_profile` — **all untouched**. Deployed accounts
  keep their exact byte layout.
- `deriveBattleOutcome`'s hardcoded `opponentIsBot: true`
  (`leaderboardIx.ts:252`) stays valid: the **PvE bot** path still calls
  `record_battle`. The **PvP** path goes through the new
  `commit_challenge`/`resolve_challenge` flow and never touches
  `record_battle`, so the two rating models (fixed vs scaled elo) coexist by
  code path, both writing the same `rating: i32` field with no layout change.
- The **5 PvE bots stay** as the always-available tutorial floor
  (`opponents.ts` unchanged). Ghosts are an *additional* ranked tier.
- New instructions are additive to `lib.rs:27 #[program]`; new `#[account]`
  structs are additive to `state.rs`. `cargo build` stays green.

### Roster-sampling decision (Q1)

| Option | How | Verdict |
|---|---|---|
| **A. `getProgramAccounts` scan** | client filters `ArenaSnapshot` accounts by discriminator + `memcmp` on a rating band + freshness, samples client-side | **MVP.** Zero infra, works on devnet today. Heavy-ish RPC, no true random sampling — fine at devnet scale. |
| B. Off-chain indexer | Helius/custom indexes `ArenaSnapshot`, serves rating-banded matchmaking | **Hardening.** Best UX/scale; needs infra. |
| C. Rotating on-chain pool account | ring buffer of recent snapshot pubkeys, updated on publish | Rejected for MVP: write contention on publish, not rating-aware. |

**Reconciliation with `OpponentRoster`:** the roster becomes
`PveOpponent[] ∪ GhostOpponent[]`. A `GhostOpponent` is decoded from an
`ArenaSnapshot` into the **same `AvatarCard`/`CombatantSnapshot` shape** the
component already consumes (`opponentSnapshot()` `combat.ts:33`). Bots render as
tiers 1–5 ("Training"); ghosts render in a new "Ranked" section with
rating + freshness + that character's `CharRecord` W/L. **No new render shape** —
the component just receives a longer list plus a section flag.

---

## 6. UX

- **Publish your build:** a "Publish to ranked" button on the loadout/arena
  screen → one `publish_snapshot` tx (wallet-signed). Shows current rating +
  the exact stat fold that will be captured. The client watches the live build
  and nudges "Your build changed — republish" when the snapshot is stale.
- **Roster:** two sections. "Training" = the 5 bots (always present, guaranteed
  floor). "Ranked ghosts" = live `ArenaSnapshot`s sampled in the challenger's
  rating band, each card showing name, rating, character W/L (`CharRecord`), and
  freshness ("published 2h ago").
- **Fighting a ghost (smooth flow):** `commit_challenge` then
  `resolve_challenge`, both signable by the **session key** (the burner from
  `sessionKey.ts`), so the hot path is **popup-free** exactly like
  `recordBattleResult` today (`leaderboardClient.ts:273`). Dual-writing the
  opponent's stats needs no popup from the opponent — the signer only pushes a
  program-computed result. Client shows the same verify-and-replay preview it
  shows for `/api/battle` (with the parity encoding from §1), so the player sees
  the fight animate and can independently re-derive the outcome.
- **Per-character W/L** appears on every character card (loadout + ghost cards),
  giving specific characters provenance and setting up future NFT-avatar value.

---

## 7. Phased milestones

### MVP (Phase 1) — devnet
Goal: publish a build, fight real ghosts, dual-record, per-character W/L.
- `ArenaSnapshot` PDA + `publish_snapshot` / `unpublish_snapshot`
  (captured-stats integrity — see §4 boundary).
- `Challenge` PDA + `commit_challenge` + **permissionless** `resolve_challenge`
  running the **on-chain deterministic sim** (the trustless resolve) + cross-impl
  test vectors vs `combat.ts`.
- Dual `PlayerStats` write with **scaled elo** (new code path; `record_battle`
  untouched).
- `CharRecord` PDA, dual-written at resolve; read on character cards.
- `PairCooldown` + min-ranked-games gate.
- Web: getProgramAccounts roster sampling mixed with the 5 bots; publish button;
  ghost cards; per-char W/L; session-key smooth flow for commit+resolve.

### Phase 2 — hardening
- Publish integrity: `publish_snapshot` CPI-verifies stats + equipment **holder**
  checks against `solana-ekza-arena` (`PlayerAvatar`/`EquipmentRecord`/`ArenaItem`).
- Off-chain rating-banded matchmaking indexer.
- Keeper bot sweeping open/expired challenges (backstops permissionless reveal).
- Snapshot freshness decay in matchmaking; `close_expired_challenge` UX.
- Season resets (new `Leaderboard` per season already supported by the
  per-authority seed, `constants.rs:6`); seasonal `CharRecord` variants.
- Optional anti-sybil deposit on `commit_challenge`.

---

## 8. Scaffold-spec — client call sketches

```ts
// PURE ix layer (mirror lib/chain/leaderboardIx.ts) — TODO, signatures only.
arenaSnapshotPda(programId, owner): PublicKey            // ["arena_snapshot_v1", owner]
challengePda(programId, challenger, nonce): PublicKey    // ["challenge_v1", challenger, u64le(nonce)]
charRecordPda(programId, owner, avatarRef): PublicKey    // ["char_record_v1", owner, avatarRef]
pairCooldownPda(programId, a, b): PublicKey              // ["pair_cd_v1", ...sortPubkeys(a,b)]

publishSnapshotAccounts({programId, owner, avatarRef}): {...}
commitChallengeAccounts({programId, challenger, nonce, opponentSnapshot}): {...}
resolveChallengeAccounts({programId, challenge, challengerSnapshot, opponentSnapshot,
                          challenger, opponentOwner, board, payer}): {...}   // §2.7

// SEND layer (mirror lib/chain/leaderboardClient.ts) — TODO.
publishSnapshot({connection, wallet, build})              // wallet-signed
commitAndResolveGhost({connection, sessionKeypair, wallet?, challenger,
                       opponentSnapshot, board})           // smooth flow:
   //   1. commit_challenge (session key)   2. poll to target_slot+delay
   //   3. resolve_challenge (session key or keeper)   -> {winner, sig}
   //   never throws -> {mode:"skipped", reason} like recordBattleResult

// ROSTER (mirror content/opponents.ts consumption)
sampleGhosts({connection, programId, ratingBand, limit}): GhostOpponent[]  // getProgramAccounts + memcmp
ghostToCombatant(snapshot): CombatantSnapshot   // sets mint = snapshotPubkey (§1 parity!)
```

```
// Program entry points (mirror arena-leaderboard/src/lib.rs #[program]) — TODO stubs, no bodies.
pub fn publish_snapshot(ctx, args: PublishSnapshotArgs) -> Result<()>
pub fn unpublish_snapshot(ctx) -> Result<()>
pub fn commit_challenge(ctx, nonce: u64, args: CommitChallengeArgs) -> Result<()>
pub fn resolve_challenge(ctx) -> Result<()>            // permissionless; runs resolve_onchain
pub fn close_expired_challenge(ctx) -> Result<()>
```

> Rust struct stubs above (§2.1–2.4, §3) are layout-only and would compile as
> unused `#[account]` types (warnings, not errors). They are intentionally left
> in this doc rather than added to `state.rs` so the workspace stays untouched
> until the build phase; drop them in verbatim when scaffolding begins.

---

## 9. Open questions for the build phase
- Exact `K`, `MIN_RANKED_GAMES`, `PairCooldown` window, `STALE_SLOTS`,
  `REVEAL_DELAY_SLOTS` reuse (`solana-ekza-arena/constants.rs:45` = 5) — pick at
  scaffold time.
- Fixed-point elo expected-score implementation (lookup table vs rational
  approximation) — keep it deterministic and identical in the TS preview.
- Whether exhibition (cooldown-exceeded) fights still write `CharRecord` (lean
  yes — character W/L is not the ladder) but never rating (yes).
- Whether to also dual-write `CharRecord` from the PvE `record_battle` path
  (defer; would touch an existing instruction).
```

---

## Build decisions (2026-08-13) — LOCKED

1. **PvP lives inside `arena-leaderboard`.** Snapshot/Challenge/CharRecord PDAs + the on-chain sim + dual-resolve all in the leaderboard program, denormalizing the engine-ready stat vector into `ArenaSnapshot` so `resolve_challenge` needs NO CPI to `solana-ekza-arena`. Keeps resolve cheap and self-contained. `PlayerStats`/`record_battle`/`Leaderboard` layout untouched.
2. **MVP trusts client-captured publish stats.** `publish_snapshot` stores the stat vector the client provides — same trust boundary as today's `/api/battle` (which already trusts client build state). Hardening phase adds CPI stat + equipment-holder verification against `solana-ekza-arena`. Ship MVP first.
3. **Resolve = on-chain re-resolution (trustless).** The program recomputes the winner from both snapshots + slot-hash seed; `resolve_challenge` is permissionless. NON-NEGOTIABLE build requirement: the Rust sim must be BYTE-IDENTICAL to the TS `resolveBattle` — including the FNV-1a tie-break over CANONICALIZED combatant identity (use each side's `ArenaSnapshot` pubkey bytes as the identity input, and make the web preview feed the identical bytes). Build order: generate a shared JSON test-vector suite from the TS sim (many seeds, incl. tie-break cases), commit it, then make the Rust port reproduce every vector exactly in an anchor/cargo test. A single divergent vector blocks the feature.
