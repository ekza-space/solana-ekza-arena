# Item Enhancement («Заточка») — on-chain design

Status: SPEC v1 (2026-08-10). Implements the Lineage-style enhancement loop as
the second provably-fair luck surface after commit-reveal minting.

## Product intent

- Enhancement scrolls are **consumable NFTs sold by the protocol** — a ticket
  granting one upgrade attempt on a weapon / armor / charm item.
- Levels **+1..+3 are always safe**. From **+4 onward every attempt can
  destroy (burn) the item**. Higher level → stronger item → higher break risk.
- Randomness must be **on-chain and snipe-proof**: two-phase commit-reveal off
  a future SlotHashes entry, exactly like `commit_mint`/`reveal_mint`. A
  single-tx roll would let a bot simulate the outcome locally and submit only
  winning slots.

## On-chain model

### ItemEnhancement PDA — `["enhancement", item_mint]`

```rust
pub struct ItemEnhancement {
    pub item_mint: Pubkey,
    pub level: u8,        // current enhancement level, 0..=10
    pub attempts: u16,    // lifetime attempts (analytics)
    pub bump: u8,
}
```

Created lazily on the first commit for an item. Kept **separate from
`ArenaItem`** so the deployed account layout does not change — no migration.

### EnhanceScroll — consumable NFT

- `mint_enhance_scroll`: mints an SPL NFT (supply 1, Metaplex metadata,
  symbol `EKZASCROLL`) plus marker PDA `["scroll", scroll_mint]`.
- Price: **reuse `registry.commit_fee_lamports * SCROLL_FEE_MULTIPLIER`**
  (constant, start ×2) with the same treasury/sink split accounts as
  `commit_mint`. **Do not change the ArenaRegistry account layout.**
- Scroll is burned on `reveal_enhance` regardless of outcome (supply sink).

### Instructions

1. `commit_enhance { nonce: u64 }`
   - accounts: owner, item ArenaItem PDA, item token account (must hold 1),
     scroll marker PDA + scroll token account (must hold 1), enhancement PDA
     (init_if_needed), commit PDA `["enhance_commit", owner, nonce]`,
     escrow: transfer the scroll token AND the item token (v1.2) into
     commit-PDA-owned ATAs (hard-locks both).
   - guards: level < MAX_LEVEL (10); no open commit for this item (store
     `pending: bool` or check commit PDA existence via seeds incl. item);
     item not referenced by any equip slot of the owner's avatar/equipment
     record (address-derived, may be uninitialized).
   - stores `target_slot = clock.slot + REVEAL_DELAY (5)`.
2. `reveal_enhance { nonce: u64 }` — PERMISSIONLESS
   - seed = hash(slot_hash(target_slot) ++ owner ++ item_mint ++ nonce)
   - success: `roll_u16 % 1000 < SUCCESS_BPS[level]` (table below)
   - on success: `level += 1` (mirrored into `ArenaItem.enhance_level`), item
     returned from escrow to the owner's ATA.
   - on failure (only possible at level >= 3 attempting +4 and above):
     **burn the item NFT from escrow** (Metaplex BurnNft, commit PDA signs),
     close its ArenaItem PDA and the ItemEnhancement PDA (rent → owner).
   - either way: burn the scroll from escrow, close commit PDA (rent → owner).
   - emit event `EnhanceResult { item_mint, level_before, success, destroyed }`.
3. `close_expired_enhance_commit` — after N slots (reuse mint expiry policy),
   returns the escrowed item to the owner but **burns the escrowed scroll**
   (no refund) and closes the commit, unlocking the item. Permissionless.

### Peek-abandon resistance (v1.1/v1.2 revisions)

The outcome is derivable off-chain once the target slot passes. If reveal were
owner-only and expiry refunded the scroll, a patient owner could peek, abandon
losing commits, and re-roll risk-free — the item would never burn. Therefore:

- **`reveal_enhance` is permissionless**: anyone may reveal any pending commit
  once the target slot passes (rent refunds still go to the item owner). A
  rival — or our keeper cron — can force a peeked loss through, so abandoning
  is not a reliable escape.
- **Expiry burns the scroll**: abandoning always costs the full ticket, same
  economics as an abandoned mint commit (fee lost).
- **The item is escrowed at commit (v1.2)**: an SPL-delegate scheme was
  auditably broken — `approve` is unilaterally revocable and cleared by a
  transfer, so the owner could front-run a permissionless reveal and shield
  the item from the failure burn. With the token in a commit-PDA escrow no
  owner action can save a peeked loss.

### Success table (per-mille)

| attempt (level → level+1) | success | note |
|---|---|---|
| 0→1, 1→2, 2→3 | 1000 | safe zone |
| 3→4 | 700 | risk starts |
| 4→5 | 500 | |
| 5→6 | 350 | |
| 6→7 | 250 | |
| 7→8 | 175 | |
| 8→9 | 120 | |
| 9→10 | 80 | cap |

Failure at any risky step destroys the item. Expected scrolls-to-+10 ≈ 46 —
whales chase it, casuals stop at +3..+5. Tune later via program upgrade;
KEEP THE TABLE IN ONE CONST.

### Stat effect (read side)

`effective_stat = base_stat * (100 + 8 * level) / 100` — +8% per level to the
item's rolled affix values, computed at read time by the SDK/web from
`ItemEnhancement.level`. Nothing stored twice; battle sim and UI both read the
PDA. (Web/SDK wiring is a separate follow-up task.)

## Anti-abuse

- Commit-reveal: outcome fixed by a future slot hash — no client-side
  outcome-sniping, no retry-until-lucky (item is locked while committed).
- Scroll escrowed at commit — can't sell the scroll after seeing the odds.
- Item escrowed at commit (v1.2) — it cannot be traded, re-committed, or
  shielded from the failure burn while the outcome is pending; equipped
  items are rejected at commit.
- No free scrolls: only the fee-paying mint instruction issues valid marker
  PDAs; `reveal_enhance` requires the marker.

## Testing bar (Anchor E2E)

- safe +1..+3 always succeed; scroll burned; level increments.
- risky success and risky failure paths (deterministic via test slot-hash
  control): failure burns item NFT + closes ArenaItem + ItemEnhancement.
- double-commit rejected; reveal without commit rejected; foreign scroll
  (no marker PDA) rejected; expired commit returns scroll.
- fee split of scroll mint matches registry values to the lamport.
- level cap: commit at level 10 rejected.
