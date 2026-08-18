import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { expect } from "chai";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ArenaLeaderboard } from "../target/types/arena_leaderboard";
import { deriveSeed, resolveWinner, SimSnap } from "./pvp-sim";

/**
 * arena-leaderboard — async PvP ladder (ghost snapshots + trustless commit/
 * reveal challenges + per-character history). Exercises the full MVP:
 *   - publish two ghosts from two wallets;
 *   - commit + PERMISSIONLESS resolve (signed by an unrelated third wallet);
 *   - the on-chain winner equals the TS-derived winner for the resolve seed;
 *   - dual PlayerStats + dual CharRecord writes; opponent-scaled near-zero-sum elo;
 *   - PairCooldown exhibition (repeat pairing = no rating change);
 *   - min-ranked-games heap gate; self-snapshot rejection; expired-challenge close.
 *
 * The TS predictor `./pvp-sim` is itself validated against the shared parity
 * fixtures at the top of this suite, so winner parity is transitive:
 *   web combat.ts == fixtures == Rust sim (cargo) == this helper == on-chain.
 */
describe("arena-leaderboard :: async PvP", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const program = anchor.workspace
    .arenaLeaderboard as Program<ArenaLeaderboard>;
  const connection = provider.connection;

  const PVP_COMMIT_WINDOW_SLOTS = 300; // must match constants.rs
  const SLOT_HASHES_SYSVAR = new anchor.web3.PublicKey(
    "SysvarS1otHashes111111111111111111111111111"
  );
  const LEADERBOARD_LEN = 8 + 32 + 2 + 2 + 1 + 3 + 1000 * 40;

  // --- PDA helpers ---------------------------------------------------------
  const arenaSnapshotPda = (owner: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_snapshot_v1"), owner.toBuffer()],
      program.programId
    )[0];
  const challengePda = (challenger: anchor.web3.PublicKey, nonce: anchor.BN) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("challenge_v1"),
        challenger.toBuffer(),
        nonce.toArrayLike(Buffer, "le", 8),
      ],
      program.programId
    )[0];
  const charRecordPda = (
    owner: anchor.web3.PublicKey,
    avatarRef: anchor.web3.PublicKey
  ) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("char_record_v1"), owner.toBuffer(), avatarRef.toBuffer()],
      program.programId
    )[0];
  const playerStatsPda = (player: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("player_stats_v1"), player.toBuffer()],
      program.programId
    )[0];
  const pairCooldownPda = (
    lo: anchor.web3.PublicKey,
    hi: anchor.web3.PublicKey
  ) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("pair_cd_v1"), lo.toBuffer(), hi.toBuffer()],
      program.programId
    )[0];

  const sortPair = (
    a: anchor.web3.PublicKey,
    b: anchor.web3.PublicKey
  ): [anchor.web3.PublicKey, anchor.web3.PublicKey] =>
    Buffer.compare(a.toBuffer(), b.toBuffer()) <= 0 ? [a, b] : [b, a];

  const airdrop = async (to: anchor.web3.PublicKey, sol = 10) => {
    const sig = await connection.requestAirdrop(
      to,
      sol * anchor.web3.LAMPORTS_PER_SOL
    );
    const bh = await connection.getLatestBlockhash();
    await connection.confirmTransaction({ signature: sig, ...bh });
  };

  const waitForSlotAfter = async (target: number) => {
    for (;;) {
      const slot = await connection.getSlot();
      if (slot > target) return slot;
      await new Promise((r) => setTimeout(r, 200));
    }
  };

  // First 8 LE bytes of the earliest produced slot at/after the committed
  // target. This mirrors the skip-safe on-chain entropy selection.
  const slotHashFirst8 = async (targetSlot: number): Promise<bigint> => {
    const info = await connection.getAccountInfo(SLOT_HASHES_SYSVAR);
    const data = info!.data;
    const len = Number(data.readBigUInt64LE(0));
    let candidate: bigint | null = null;
    for (let i = 0; i < len; i++) {
      const base = 8 + i * 40;
      const producedSlot = Number(data.readBigUInt64LE(base));
      if (producedSlot === targetSlot) {
        return data.readBigUInt64LE(base + 8);
      }
      if (producedSlot > targetSlot) {
        candidate = data.readBigUInt64LE(base + 8);
      } else if (candidate !== null) {
        return candidate;
      } else {
        break;
      }
    }
    throw new Error(
      `first produced slot at/after ${targetSlot} is not provable from SlotHashes`
    );
  };

  const zeros32 = () => Array(32).fill(0);

  const publishSnapshot = async (
    owner: anchor.web3.Keypair,
    avatarRef: anchor.web3.PublicKey,
    stats: { hp: number; attack: number; armor: number; speed: number },
    skillMask: number
  ) =>
    program.methods
      .publishSnapshot({
        avatarRef,
        archetypeId: zeros32(),
        stats,
        skillMask,
        element: 0,
        skinRef: zeros32(),
        ratingAtPublish: 1000,
      })
      .accountsPartial({
        arenaSnapshot: arenaSnapshotPda(owner.publicKey),
        owner: owner.publicKey,
      })
      .signers([owner])
      .rpc();

  const commitChallenge = async (
    challenger: anchor.web3.Keypair,
    opponentOwner: anchor.web3.PublicKey,
    nonce: anchor.BN
  ) =>
    program.methods
      .commitChallenge(nonce)
      .accountsPartial({
        challenge: challengePda(challenger.publicKey, nonce),
        challengerSnapshot: arenaSnapshotPda(challenger.publicKey),
        opponentSnapshot: arenaSnapshotPda(opponentOwner),
        challenger: challenger.publicKey,
      })
      .signers([challenger])
      .rpc();

  const resolveChallenge = async (
    challenger: anchor.web3.PublicKey,
    challengerAvatar: anchor.web3.PublicKey,
    opponentOwner: anchor.web3.PublicKey,
    opponentAvatar: anchor.web3.PublicKey,
    nonce: anchor.BN,
    board: anchor.web3.PublicKey,
    payer: anchor.web3.Keypair
  ) => {
    const [lo, hi] = sortPair(challenger, opponentOwner);
    return program.methods
      .resolveChallenge(nonce, lo, hi)
      .accountsPartial({
        challenge: challengePda(challenger, nonce),
        challenger,
        challengerSnapshot: arenaSnapshotPda(challenger),
        opponentSnapshot: arenaSnapshotPda(opponentOwner),
        challengerStats: playerStatsPda(challenger),
        opponentStats: playerStatsPda(opponentOwner),
        challengerChar: charRecordPda(challenger, challengerAvatar),
        opponentChar: charRecordPda(opponentOwner, opponentAvatar),
        pairCooldown: pairCooldownPda(lo, hi),
        leaderboard: board,
        slotHashes: SLOT_HASHES_SYSVAR,
        payer: payer.publicKey,
      })
      .signers([payer])
      .rpc();
  };

  const expectErrorCode = async (p: Promise<unknown>, code: string) => {
    try {
      await p;
    } catch (err) {
      expect(String(err)).to.include(code);
      return;
    }
    expect.fail(`expected ${code}, but the instruction succeeded`);
  };

  // Cast: challenger (w1), opponent (w2), keeper (unrelated permissionless signer).
  const w1 = anchor.web3.Keypair.generate();
  const w2 = anchor.web3.Keypair.generate();
  const keeper = anchor.web3.Keypair.generate();
  const boardAuthority = anchor.web3.Keypair.generate();
  const boardKp = anchor.web3.Keypair.generate();
  const board = boardKp.publicKey;
  // Stand-ins for ArenaAssetData avatar-card pubkeys (the per-character keys).
  const avatarA = anchor.web3.Keypair.generate().publicKey;
  const avatarB = anchor.web3.Keypair.generate().publicKey;

  const statsA = { hp: 24, attack: 9, armor: 3, speed: 5 };
  const statsB = { hp: 22, attack: 11, armor: 2, speed: 5 };
  const skillA = 1 << 0; // moss_skin
  const skillB = 1 << 4; // heavy_guard

  let nonceCtr = Date.now();
  const nextNonce = () => new anchor.BN(nonceCtr++);

  before(async () => {
    await Promise.all(
      [w1, w2, keeper, boardAuthority].map((k) => airdrop(k.publicKey))
    );
    // Create + init the ranked board (capacity 8).
    const lamports = await connection.getMinimumBalanceForRentExemption(
      LEADERBOARD_LEN
    );
    const createIx = anchor.web3.SystemProgram.createAccount({
      fromPubkey: boardAuthority.publicKey,
      newAccountPubkey: board,
      lamports,
      space: LEADERBOARD_LEN,
      programId: program.programId,
    });
    await program.methods
      .initLeaderboard(8)
      .accountsPartial({
        leaderboard: board,
        authority: boardAuthority.publicKey,
      })
      .preInstructions([createIx])
      .signers([boardAuthority, boardKp])
      .rpc();
  });

  it("TS predictor reproduces every shared parity fixture (helper is trustworthy)", () => {
    const fixture = JSON.parse(
      readFileSync(join(__dirname, "fixtures/pvp-sim-vectors.json"), "utf8")
    );
    let checked = 0;
    for (const v of fixture.vectors) {
      const toSim = (s: any): SimSnap => ({
        identityBase58: new anchor.web3.PublicKey(
          Uint8Array.from(s.identity)
        ).toBase58(),
        hp: s.hp,
        attack: s.attack,
        armor: s.armor,
        speed: s.speed,
        skillMask: s.skillMask,
      });
      const nonce = BigInt(`0x${v.seedHex}`);
      const { winnerIsA, rounds } = resolveWinner(
        toSim(v.a),
        toSim(v.b),
        nonce
      );
      expect(winnerIsA, `winner ${v.note}`).to.equal(v.expectedWinner === "A");
      expect(rounds, `rounds ${v.note}`).to.equal(v.expectedRounds);
      checked += 1;
    }
    expect(checked).to.be.greaterThan(300);
  });

  it("publishes two ghosts from two wallets", async () => {
    await publishSnapshot(w1, avatarA, statsA, skillA);
    await publishSnapshot(w2, avatarB, statsB, skillB);

    const snapA = await program.account.arenaSnapshot.fetch(
      arenaSnapshotPda(w1.publicKey)
    );
    expect(snapA.owner.toBase58()).to.equal(w1.publicKey.toBase58());
    expect(snapA.avatarRef.toBase58()).to.equal(avatarA.toBase58());
    expect(snapA.stats.hp).to.equal(statsA.hp);
    expect(snapA.skillMask).to.equal(skillA);
    const snapB = await program.account.arenaSnapshot.fetch(
      arenaSnapshotPda(w2.publicKey)
    );
    expect(snapB.owner.toBase58()).to.equal(w2.publicKey.toBase58());
  });

  it("rejects a challenge against your own snapshot (no-record vs self)", async () => {
    const nonce = nextNonce();
    await expectErrorCode(
      program.methods
        .commitChallenge(nonce)
        .accountsPartial({
          challenge: challengePda(w1.publicKey, nonce),
          challengerSnapshot: arenaSnapshotPda(w1.publicKey),
          opponentSnapshot: arenaSnapshotPda(w1.publicKey), // own ghost
          challenger: w1.publicKey,
        })
        .signers([w1])
        .rpc(),
      "SelfSnapshotNotAllowed"
    );
  });

  it("commit + PERMISSIONLESS resolve: on-chain winner == TS, dual-write, near-zero-sum elo, heap gate", async () => {
    const nonce = nextNonce();
    await commitChallenge(w1, w2.publicKey, nonce);

    const challenge = await program.account.challenge.fetch(
      challengePda(w1.publicKey, nonce)
    );
    const targetSlot = challenge.targetSlot.toNumber();
    await waitForSlotAfter(targetSlot);

    // Independently derive the resolve seed + winner from the real slot hash.
    const slothash = await slotHashFirst8(targetSlot);
    const seed = deriveSeed(
      slothash,
      w1.publicKey,
      arenaSnapshotPda(w2.publicKey),
      BigInt(challenge.nonce.toString())
    );
    const simA: SimSnap = {
      identityBase58: arenaSnapshotPda(w1.publicKey).toBase58(),
      ...statsA,
      skillMask: skillA,
    };
    const simB: SimSnap = {
      identityBase58: arenaSnapshotPda(w2.publicKey).toBase58(),
      ...statsB,
      skillMask: skillB,
    };
    const predicted = resolveWinner(simA, simB, seed);

    // Resolve is pushed by an UNRELATED third wallet (permissionless).
    await resolveChallenge(
      w1.publicKey,
      avatarA,
      w2.publicKey,
      avatarB,
      nonce,
      board,
      keeper
    );

    const statsW1 = await program.account.playerStats.fetch(
      playerStatsPda(w1.publicKey)
    );
    const statsW2 = await program.account.playerStats.fetch(
      playerStatsPda(w2.publicKey)
    );
    const charW1 = await program.account.charRecord.fetch(
      charRecordPda(w1.publicKey, avatarA)
    );
    const charW2 = await program.account.charRecord.fetch(
      charRecordPda(w2.publicKey, avatarB)
    );

    // On-chain winner (w1 won iff its wins incremented) must match the predictor.
    const w1WonOnChain = statsW1.wins === 1;
    expect(w1WonOnChain).to.equal(
      predicted.winnerIsA,
      "on-chain winner must equal the TS-derived winner for the seed"
    );

    // Dual PlayerStats write.
    expect(statsW1.games).to.equal(1);
    expect(statsW2.games).to.equal(1);
    expect(statsW1.wins + statsW1.losses).to.equal(1);
    expect(statsW2.wins).to.equal(statsW1.losses); // exactly one winner
    expect(statsW2.losses).to.equal(statsW1.wins);

    // Dual CharRecord write, mirroring the outcome.
    expect(charW1.games).to.equal(1);
    expect(charW2.games).to.equal(1);
    expect(charW1.wins).to.equal(statsW1.wins);
    expect(charW2.wins).to.equal(statsW2.wins);

    // Opponent-scaled elo from equal 1000/1000: winner +12, loser -12 -> zero-sum.
    const winnerRating = w1WonOnChain ? statsW1.rating : statsW2.rating;
    const loserRating = w1WonOnChain ? statsW2.rating : statsW1.rating;
    expect(winnerRating).to.equal(1012);
    expect(loserRating).to.equal(988);
    expect(winnerRating - 1000 + (loserRating - 1000)).to.equal(0);

    // Min-ranked-games gate: 1 game (< 3) keeps both wallets off the heap.
    const boardAcc = await program.account.leaderboard.fetch(board);
    expect(boardAcc.size).to.equal(0);

    // PairCooldown created and this fight consumed the pair's rated allowance.
    const [lo, hi] = sortPair(w1.publicKey, w2.publicKey);
    const pcd = await program.account.pairCooldown.fetch(
      pairCooldownPda(lo, hi)
    );
    expect(pcd.rankedToday).to.equal(1);
    expect(pcd.lastRankedSlot.toNumber()).to.be.greaterThan(0);
  });

  it("repeat pairing within cooldown resolves as a no-rating exhibition", async () => {
    const before1 = await program.account.playerStats.fetch(
      playerStatsPda(w1.publicKey)
    );
    const before2 = await program.account.playerStats.fetch(
      playerStatsPda(w2.publicKey)
    );

    const nonce = nextNonce();
    await commitChallenge(w1, w2.publicKey, nonce);
    const challenge = await program.account.challenge.fetch(
      challengePda(w1.publicKey, nonce)
    );
    await waitForSlotAfter(challenge.targetSlot.toNumber());
    await resolveChallenge(
      w1.publicKey,
      avatarA,
      w2.publicKey,
      avatarB,
      nonce,
      board,
      keeper
    );

    const after1 = await program.account.playerStats.fetch(
      playerStatsPda(w1.publicKey)
    );
    const after2 = await program.account.playerStats.fetch(
      playerStatsPda(w2.publicKey)
    );

    // W/L still accrue, but rating is FROZEN (exhibition — no rating change).
    expect(after1.games).to.equal(2);
    expect(after2.games).to.equal(2);
    expect(after1.rating).to.equal(before1.rating);
    expect(after2.rating).to.equal(before2.rating);

    // CharRecord still counts the exhibition.
    const charW1 = await program.account.charRecord.fetch(
      charRecordPda(w1.publicKey, avatarA)
    );
    expect(charW1.games).to.equal(2);

    // The pair's rated allowance was NOT consumed again.
    const [lo, hi] = sortPair(w1.publicKey, w2.publicKey);
    const pcd = await program.account.pairCooldown.fetch(
      pairCooldownPda(lo, hi)
    );
    expect(pcd.rankedToday).to.equal(1);
  });

  it("rejects resolving before the target slot (RevealTooEarly)", async () => {
    const nonce = nextNonce();
    await commitChallenge(w1, w2.publicKey, nonce);
    // Do NOT wait — target_slot = commit_slot + 5 is still in the future.
    await expectErrorCode(
      resolveChallenge(
        w1.publicKey,
        avatarA,
        w2.publicKey,
        avatarB,
        nonce,
        board,
        keeper
      ),
      "RevealTooEarly"
    );
    // Clean up this open challenge so it doesn't linger (resolve it properly).
    const challenge = await program.account.challenge.fetch(
      challengePda(w1.publicKey, nonce)
    );
    await waitForSlotAfter(challenge.targetSlot.toNumber());
    await resolveChallenge(
      w1.publicKey,
      avatarA,
      w2.publicKey,
      avatarB,
      nonce,
      board,
      keeper
    );
  });

  it("closes an expired challenge and refunds rent (permissionless)", async () => {
    const nonce = nextNonce();
    await commitChallenge(w1, w2.publicKey, nonce);
    const challenge = await program.account.challenge.fetch(
      challengePda(w1.publicKey, nonce)
    );
    // Let the reveal window age out entirely, then any signer can reclaim rent.
    await waitForSlotAfter(
      challenge.targetSlot.toNumber() + PVP_COMMIT_WINDOW_SLOTS
    );
    await program.methods
      .closeExpiredChallenge(nonce)
      .accountsPartial({
        challenge: challengePda(w1.publicKey, nonce),
        challenger: w1.publicKey,
        closer: keeper.publicKey,
      })
      .signers([keeper])
      .rpc();

    const info = await connection.getAccountInfo(
      challengePda(w1.publicKey, nonce)
    );
    expect(info).to.equal(null);
  });

  it("lets a wallet unpublish its ghost (rent back to owner)", async () => {
    // Use a throwaway wallet so w1/w2 ghosts stay live for other cases.
    const w3 = anchor.web3.Keypair.generate();
    await airdrop(w3.publicKey, 2);
    await publishSnapshot(w3, avatarA, statsA, 0);
    expect(
      await connection.getAccountInfo(arenaSnapshotPda(w3.publicKey))
    ).to.not.equal(null);
    await program.methods
      .unpublishSnapshot()
      .accountsPartial({
        arenaSnapshot: arenaSnapshotPda(w3.publicKey),
        owner: w3.publicKey,
      })
      .signers([w3])
      .rpc();
    expect(
      await connection.getAccountInfo(arenaSnapshotPda(w3.publicKey))
    ).to.equal(null);
  });
});
