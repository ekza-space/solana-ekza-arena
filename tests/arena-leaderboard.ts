import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { expect } from "chai";
import { ArenaLeaderboard } from "../target/types/arena_leaderboard";

/**
 * arena-leaderboard — separate program (own id): zero-copy MIN-heap top-N with
 * auto-eviction, per-player battle stats, session-key battle recording and the
 * top-list-only `set_profile` perk.
 *
 * The board under test uses a tiny capacity (4) so eviction is exercised with a
 * handful of battles; the min-heap invariant (parent ≤ children by rating) is
 * re-verified after every single operation.
 */
describe("arena-leaderboard", () => {
  anchor.setProvider(anchor.AnchorProvider.env());

  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const program = anchor.workspace
    .arenaLeaderboard as Program<ArenaLeaderboard>;

  // The board is a large zero-copy account (~40 KB) — too big for a single-CPI
  // `init`, so the client creates it with a top-level SystemProgram.createAccount
  // and the program claims it via `#[account(zero)]`. LEN mirrors the Rust
  // layout: 8 disc + 32 authority + 2 capacity + 2 size + 1 bump + 3 pad
  // + 1000 * (32 + 4 + 4) entries.
  const LEADERBOARD_LEN = 8 + 32 + 2 + 2 + 1 + 3 + 1000 * 40;

  /** Create the board account, then init it, atomically. */
  const createAndInitBoard = async (
    authority: anchor.web3.Keypair,
    boardKp: anchor.web3.Keypair,
    capacity: number
  ) => {
    const lamports =
      await provider.connection.getMinimumBalanceForRentExemption(
        LEADERBOARD_LEN
      );
    const createIx = anchor.web3.SystemProgram.createAccount({
      fromPubkey: authority.publicKey,
      newAccountPubkey: boardKp.publicKey,
      lamports,
      space: LEADERBOARD_LEN,
      programId: program.programId,
    });
    return program.methods
      .initLeaderboard(capacity)
      .accountsPartial({
        leaderboard: boardKp.publicKey,
        authority: authority.publicKey,
      })
      .preInstructions([createIx])
      .signers([authority, boardKp])
      .rpc();
  };

  const playerStatsPda = (player: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("player_stats_v1"), player.toBuffer()],
      program.programId
    )[0];

  const battleRateLimitPda = (player: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("battle_rate_limit_v1"), player.toBuffer()],
      program.programId
    )[0];

  const waitForNextSlot = async () => {
    const start = await provider.connection.getSlot("processed");
    const deadline = Date.now() + 10_000;
    while ((await provider.connection.getSlot("processed")) <= start) {
      if (Date.now() >= deadline)
        throw new Error("validator slot did not advance");
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  };

  const airdrop = async (to: anchor.web3.PublicKey, sol = 10) => {
    const sig = await provider.connection.requestAirdrop(
      to,
      sol * anchor.web3.LAMPORTS_PER_SOL
    );
    const blockhash = await provider.connection.getLatestBlockhash();
    await provider.connection.confirmTransaction({
      signature: sig,
      ...blockhash,
    });
  };

  type Board = Awaited<ReturnType<typeof program.account.leaderboard.fetch>>;

  /** entries[0..size] must satisfy: parent.rating <= child.rating (min-heap). */
  const assertMinHeap = (board: Board) => {
    for (let i = 1; i < board.size; i++) {
      const parent = Math.floor((i - 1) / 2);
      expect(board.entries[parent].rating).to.be.at.most(
        board.entries[i].rating,
        `min-heap violated at index ${i} (parent ${parent})`
      );
    }
  };

  const topPlayers = (board: Board) =>
    board.entries.slice(0, board.size).map((e) => e.player.toBase58());

  const fetchBoardChecked = async (pda: anchor.web3.PublicKey) => {
    const board = await program.account.leaderboard.fetch(pda);
    assertMinHeap(board);
    return board;
  };

  /** Fixed-byte utf-8 field ([u8;N], zero-padded) → string. */
  const fixedUtf8 = (bytes: number[]) =>
    Buffer.from(bytes).toString("utf8").replace(/\0+$/, "");

  const expectErrorCode = async (p: Promise<unknown>, code: string) => {
    try {
      await p;
    } catch (err) {
      expect(String(err)).to.include(code);
      return;
    }
    expect.fail(`expected ${code}, but the instruction succeeded`);
  };

  // The 4-slot board every heap test runs against.
  const boardAuthority = anchor.web3.Keypair.generate();
  const board4Kp = anchor.web3.Keypair.generate();
  const board4 = board4Kp.publicKey;
  const CAPACITY = 4;

  // Ladder cast: p1..p4 fill the board, p5 evicts, p6 exercises the daily cap.
  const p1 = anchor.web3.Keypair.generate();
  const p2 = anchor.web3.Keypair.generate();
  const p3 = anchor.web3.Keypair.generate();
  const p4 = anchor.web3.Keypair.generate();
  const p5 = anchor.web3.Keypair.generate();
  const p6 = anchor.web3.Keypair.generate();
  const sessionKey = anchor.web3.Keypair.generate();
  const strangerKey = anchor.web3.Keypair.generate();
  const throttlePlayer = anchor.web3.Keypair.generate();

  before(async () => {
    await Promise.all(
      [
        p1,
        p2,
        p3,
        p4,
        p5,
        p6,
        boardAuthority,
        sessionKey,
        strangerKey,
        throttlePlayer,
      ].map((kp) => airdrop(kp.publicKey))
    );
  });

  const recordBattle = async (
    player: anchor.web3.Keypair,
    win: boolean,
    opponentIsBot: boolean,
    board: anchor.web3.PublicKey = board4,
    signer: anchor.web3.Keypair = player
  ) => {
    // Confirmed transactions can still be submitted within the same validator
    // slot. Normal test writes wait one slot; the dedicated cooldown test below
    // deliberately batches two raw instructions without this helper.
    await waitForNextSlot();
    return program.methods
      .recordBattle(win, opponentIsBot)
      .accountsPartial({
        leaderboard: board,
        playerStats: playerStatsPda(player.publicKey),
        battleRateLimit: battleRateLimitPda(player.publicKey),
        player: player.publicKey,
        signer: signer.publicKey,
      })
      .signers([signer])
      .rpc();
  };

  it("initializes a leaderboard (small board, capacity 4)", async () => {
    await createAndInitBoard(boardAuthority, board4Kp, CAPACITY);

    const board = await fetchBoardChecked(board4);
    expect(board.authority.toBase58()).to.equal(
      boardAuthority.publicKey.toBase58()
    );
    expect(board.capacity).to.equal(CAPACITY);
    expect(board.size).to.equal(0);
  });

  it("supports the max capacity (1000) — the ~40KB zero-copy alloc works", async () => {
    const bigAuthority = anchor.web3.Keypair.generate();
    await airdrop(bigAuthority.publicKey, 2);
    const bigBoardKp = anchor.web3.Keypair.generate();
    await createAndInitBoard(bigAuthority, bigBoardKp, 1000);
    const board = await fetchBoardChecked(bigBoardKp.publicKey);
    expect(board.capacity).to.equal(1000);
    expect(board.entries.length).to.equal(1000);
  });

  it("rejects out-of-range capacities", async () => {
    const other = anchor.web3.Keypair.generate();
    await airdrop(other.publicKey, 2);
    for (const capacity of [0, 1, 1001]) {
      // A fresh board account per attempt (the create+init tx is atomic, so a
      // rejected init leaves nothing behind).
      const boardKp = anchor.web3.Keypair.generate();
      await expectErrorCode(
        createAndInitBoard(other, boardKp, capacity),
        "CapacityOutOfRange"
      );
    }
  });

  it("rejects a second init of the same board (one-time)", async () => {
    try {
      // No createAccount this time — the account already exists and is claimed,
      // so `#[account(zero)]` rejects the re-init.
      await program.methods
        .initLeaderboard(CAPACITY)
        .accountsPartial({
          leaderboard: board4,
          authority: boardAuthority.publicKey,
        })
        .signers([boardAuthority])
        .rpc();
      expect.fail("re-init should fail");
    } catch (err) {
      // The `zero` constraint fails once the discriminator is set.
      expect(String(err)).to.match(
        /already in use|Discriminator|discriminator|constraint|0x/i
      );
    }
  });

  it("records battles: stats, streaks and elo-lite rating math", async () => {
    // p1: win vs player (+25), win vs bot (+10), loss vs bot (-15) => 1020.
    await recordBattle(p1, true, false);
    let board = await fetchBoardChecked(board4);
    expect(board.size).to.equal(1);

    await recordBattle(p1, true, true);
    board = await fetchBoardChecked(board4);

    await recordBattle(p1, false, true);
    board = await fetchBoardChecked(board4);

    const stats = await program.account.playerStats.fetch(
      playerStatsPda(p1.publicKey)
    );
    expect(stats.player.toBase58()).to.equal(p1.publicKey.toBase58());
    expect(stats.wins).to.equal(2);
    expect(stats.losses).to.equal(1);
    expect(stats.games).to.equal(3);
    expect(stats.streak).to.equal(0); // reset by the loss
    expect(stats.bestStreak).to.equal(2);
    expect(stats.rating).to.equal(1000 + 25 + 10 - 15); // 1020

    // Heap entry mirrors the fresh rating + wins.
    const entry = board.entries
      .slice(0, board.size)
      .find((e) => e.player.equals(p1.publicKey));
    expect(entry).to.not.be.undefined;
    expect(entry!.rating).to.equal(1020);
    expect(entry!.wins).to.equal(2);
  });

  it("fills the board and keeps the min-heap invariant (root = weakest)", async () => {
    // p2: 1050, p3: 980, p4: 1010 — checked after every op.
    await recordBattle(p2, true, false);
    await fetchBoardChecked(board4);
    await recordBattle(p2, true, false);
    await fetchBoardChecked(board4);
    await recordBattle(p3, false, false);
    await fetchBoardChecked(board4);
    await recordBattle(p4, true, true);

    const board = await fetchBoardChecked(board4);
    expect(board.size).to.equal(4);
    expect(topPlayers(board)).to.have.members(
      [p1, p2, p3, p4].map((k) => k.publicKey.toBase58())
    );
    // Min-heap: the ROOT is the weakest of the top (p3 @ 980).
    expect(board.entries[0].player.toBase58()).to.equal(
      p3.publicKey.toBase58()
    );
    expect(board.entries[0].rating).to.equal(980);
  });

  it("evicts the weakest player when a stronger one arrives (board full)", async () => {
    // p5's first win: 1025 > root 980 → p5 replaces p3 at the root, sifts down.
    await recordBattle(p5, true, false);
    let board = await fetchBoardChecked(board4);
    expect(board.size).to.equal(4);
    expect(topPlayers(board)).to.not.include(p3.publicKey.toBase58());
    expect(topPlayers(board)).to.include(p5.publicKey.toBase58());

    // Second win updates p5 IN PLACE (no duplicate entry), heap re-ordered.
    await recordBattle(p5, true, false);
    board = await fetchBoardChecked(board4);
    expect(board.size).to.equal(4);
    expect(
      topPlayers(board).filter((p) => p === p5.publicKey.toBase58()).length
    ).to.equal(1);
    // New weakest of the top is p4 @ 1010.
    expect(board.entries[0].player.toBase58()).to.equal(
      p4.publicKey.toBase58()
    );
    expect(board.entries[0].rating).to.equal(1010);

    // p3 keeps its PlayerStats even after eviction — only the slot is lost.
    const p3Stats = await program.account.playerStats.fetch(
      playerStatsPda(p3.publicKey)
    );
    expect(p3Stats.rating).to.equal(980);
  });

  it("does NOT evict for a weaker challenger", async () => {
    // p6: loss vs player → 980 < root 1010 → stays off the board.
    await recordBattle(p6, false, false);
    const board = await fetchBoardChecked(board4);
    expect(board.size).to.equal(4);
    expect(topPlayers(board)).to.not.include(p6.publicKey.toBase58());
  });

  it("registers a session key and records battles with it (soft auto-confirm)", async () => {
    await program.methods
      .registerSessionKey(sessionKey.publicKey)
      .accountsPartial({
        playerStats: playerStatsPda(p1.publicKey),
        battleRateLimit: battleRateLimitPda(p1.publicKey),
        player: p1.publicKey,
      })
      .signers([p1])
      .rpc();

    // The burner signs the battle for p1 — no player-wallet signature.
    await recordBattle(p1, true, true, board4, sessionKey);
    await fetchBoardChecked(board4);

    const stats = await program.account.playerStats.fetch(
      playerStatsPda(p1.publicKey)
    );
    expect(stats.sessionKey!.toBase58()).to.equal(
      sessionKey.publicKey.toBase58()
    );
    expect(stats.rating).to.equal(1030); // 1020 + 10 (win vs bot)
    expect(stats.wins).to.equal(3);
  });

  it("rejects an unregistered signer", async () => {
    await expectErrorCode(
      recordBattle(p1, true, true, board4, strangerKey),
      "SessionKeyMismatch"
    );
  });

  it("rate-limits owner and session-key writes through one player PDA", async () => {
    const throttleSession = anchor.web3.Keypair.generate();

    // Wallet registration creates both PDAs, so the burner does not pay rent.
    await program.methods
      .registerSessionKey(throttleSession.publicKey)
      .accountsPartial({
        playerStats: playerStatsPda(throttlePlayer.publicKey),
        battleRateLimit: battleRateLimitPda(throttlePlayer.publicKey),
        player: throttlePlayer.publicKey,
      })
      .signers([throttlePlayer])
      .rpc();

    await recordBattle(throttlePlayer, true, true);
    let limiter = await program.account.battleRateLimit.fetch(
      battleRateLimitPda(throttlePlayer.publicKey)
    );
    expect(limiter.battlesToday).to.equal(1);

    await waitForNextSlot();
    const sessionIx = await program.methods
      .recordBattle(true, true)
      .accountsPartial({
        leaderboard: board4,
        playerStats: playerStatsPda(throttlePlayer.publicKey),
        battleRateLimit: battleRateLimitPda(throttlePlayer.publicKey),
        player: throttlePlayer.publicKey,
        signer: throttleSession.publicKey,
      })
      .instruction();
    const ownerIx = await program.methods
      .recordBattle(true, true)
      .accountsPartial({
        leaderboard: board4,
        playerStats: playerStatsPda(throttlePlayer.publicKey),
        battleRateLimit: battleRateLimitPda(throttlePlayer.publicKey),
        player: throttlePlayer.publicKey,
        signer: throttlePlayer.publicKey,
      })
      .instruction();

    // Both instructions execute in one slot. The session write consumes the
    // allowance first and the owner write sees the same player limiter.
    await expectErrorCode(
      provider.sendAndConfirm(
        new anchor.web3.Transaction().add(sessionIx, ownerIx),
        [throttleSession, throttlePlayer]
      ),
      "BattleCooldownActive"
    );

    // Failed transactions are atomic: the first instruction is rolled back.
    limiter = await program.account.battleRateLimit.fetch(
      battleRateLimitPda(throttlePlayer.publicKey)
    );
    expect(limiter.battlesToday).to.equal(1);

    // Recovery needs no authority bypass: normal slot progression is enough.
    await recordBattle(throttlePlayer, true, true, board4, throttleSession);
    limiter = await program.account.battleRateLimit.fetch(
      battleRateLimitPda(throttlePlayer.publicKey)
    );
    expect(limiter.battlesToday).to.equal(2);
  });

  it("revokes the session key (wallet-only) and then rejects it", async () => {
    await program.methods
      .revokeSessionKey()
      .accountsPartial({
        playerStats: playerStatsPda(p1.publicKey),
        player: p1.publicKey,
      })
      .signers([p1])
      .rpc();

    const stats = await program.account.playerStats.fetch(
      playerStatsPda(p1.publicKey)
    );
    expect(stats.sessionKey).to.be.null;

    await expectErrorCode(
      recordBattle(p1, true, true, board4, sessionKey),
      "SessionKeyMismatch"
    );
  });

  it("set_profile: allowed for a player currently in the top list", async () => {
    const name = "Wotori the Brave";
    const uri = "https://ekza.io/u/wotori";
    await program.methods
      .setProfile(name, uri)
      .accountsPartial({
        leaderboard: board4,
        playerStats: playerStatsPda(p2.publicKey),
        player: p2.publicKey,
      })
      .signers([p2])
      .rpc();

    const stats = await program.account.playerStats.fetch(
      playerStatsPda(p2.publicKey)
    );
    expect(fixedUtf8(stats.profileName as number[])).to.equal(name);
    expect(fixedUtf8(stats.profileUri as number[])).to.equal(uri);
  });

  it("set_profile: rejected for an evicted player (NotInTopList)", async () => {
    await expectErrorCode(
      program.methods
        .setProfile("Evicted", "https://ekza.io/u/evicted")
        .accountsPartial({
          leaderboard: board4,
          playerStats: playerStatsPda(p3.publicKey),
          player: p3.publicKey,
        })
        .signers([p3])
        .rpc(),
      "NotInTopList"
    );
  });

  it("set_profile: rejects an over-long name", async () => {
    await expectErrorCode(
      program.methods
        .setProfile("x".repeat(33), "https://ekza.io")
        .accountsPartial({
          leaderboard: board4,
          playerStats: playerStatsPda(p2.publicKey),
          player: p2.publicKey,
        })
        .signers([p2])
        .rpc(),
      "InvalidProfileName"
    );
  });

  it("enforces the per-UTC-day cap without mutating stats on rejection", async () => {
    // p6 already recorded one loss above. Fill the remaining 19 allowances.
    for (let i = 1; i < 20; i++) {
      await recordBattle(p6, false, false);
    }

    let stats = await program.account.playerStats.fetch(
      playerStatsPda(p6.publicKey)
    );
    expect(stats.games).to.equal(20);
    expect(stats.losses).to.equal(20);
    expect(stats.rating).to.equal(600);

    const limiterBefore = await program.account.battleRateLimit.fetch(
      battleRateLimitPda(p6.publicKey)
    );
    expect(limiterBefore.battlesToday).to.equal(20);

    await expectErrorCode(
      recordBattle(p6, false, false),
      "DailyBattleLimitReached"
    );

    stats = await program.account.playerStats.fetch(
      playerStatsPda(p6.publicKey)
    );
    expect(stats.games).to.equal(20);
    expect(stats.losses).to.equal(20);
    expect(stats.rating).to.equal(600);
    await fetchBoardChecked(board4);
  });
});
