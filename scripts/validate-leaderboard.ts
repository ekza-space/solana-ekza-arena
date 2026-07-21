/**
 * validate-leaderboard.ts — standalone, repeatable end-to-end proof of the
 * battle → leaderboard loop for Ekza Arena against a RUNNING localnet.
 *
 * What it proves (the full circuit, not just a compile):
 *   1. A client-created board account is allocated (SystemProgram.createAccount
 *      at full Leaderboard::LEN) and claimed via init_leaderboard.
 *   2. A player registers a SESSION (burner) key from its wallet.
 *   3. Several battles are recorded THROUGH THE SAME ix-building code the web
 *      app uses — the pure builders in
 *      `ekza-arena-web/src/lib/chain/leaderboardIx.ts` (imported here) build the
 *      account maps + args; the burner signs the hero's battles with no wallet
 *      involvement (the smooth flow), other players self-sign.
 *   4. The hero lands in the min-heap top list with the exact rating/wins the
 *      elo-lite math predicts; the min-heap invariant (parent.rating <= child)
 *      holds after every op; a weaker challenger evicts the weakest incumbent.
 *   5. set_profile (a top-list perk) writes name + link for the hero and the
 *      values read back byte-for-byte.
 *
 * Only the arena_leaderboard program is required on the validator (no Stellar /
 * Metaplex), so a bare `solana-test-validator --bpf-program <id> <so>` is enough.
 *
 * Run (against an already-running localnet):
 *   yarn validate:leaderboard
 *   (optionally: RPC_URL=http://127.0.0.1:8899 WALLET=~/.config/solana/id.json)
 *
 * Exit code 0 = PASS, 1 = FAIL.
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

// The web app's SHARED, dependency-light ix builders (web3.js only). Importing
// them here is what makes this a proof of "the same code path", not a replica:
// the account layout, PDA seeds and arg order are literally the web client's.
import {
  buildRecordBattleIx,
  buildRegisterSessionKeyIx,
  buildSetProfileIx,
  deriveBattleOutcome,
  playerStatsPda,
} from "../../ekza-arena-web/src/lib/chain/leaderboardIx";

const ARENA_LEADERBOARD_PROGRAM_ID = new anchor.web3.PublicKey(
  "9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU"
);

const LEADERBOARD_IDL = require("../target/idl/arena_leaderboard.json");

const RPC_URL = process.env.RPC_URL || "http://127.0.0.1:8899";
const WALLET_PATH = (
  process.env.WALLET ||
  process.env.ANCHOR_WALLET ||
  path.join(os.homedir(), ".config/solana/id.json")
).replace(/^~(?=$|\/)/, os.homedir());

// entries = 8 disc + 32 authority + 2 cap + 2 size + 1 bump + 3 pad + 1000*40.
const LEADERBOARD_LEN = 8 + 32 + 2 + 2 + 1 + 3 + 1000 * 40;

// ---- tiny assertion harness (no test framework needed) --------------------
let failures = 0;
function check(label: string, ok: boolean, detail?: string) {
  const tag = ok ? "PASS" : "FAIL";
  if (!ok) failures++;
  console.log(`  [${tag}] ${label}${detail ? " — " + detail : ""}`);
}

function loadWallet(): anchor.web3.Keypair {
  const raw = JSON.parse(fs.readFileSync(WALLET_PATH, "utf8"));
  return anchor.web3.Keypair.fromSecretKey(Uint8Array.from(raw));
}

async function main() {
  console.log("=== Ekza Arena battle → leaderboard loop validation ===");
  console.log("RPC:    " + RPC_URL);
  console.log("WALLET: " + WALLET_PATH);

  const connection = new anchor.web3.Connection(RPC_URL, "confirmed");
  const wallet = new anchor.Wallet(loadWallet());
  const provider = new anchor.AnchorProvider(connection, wallet, {
    commitment: "confirmed",
  });
  anchor.setProvider(provider);
  console.log("PAYER:  " + wallet.publicKey.toBase58());

  // Sanity: the program must be deployed on this localnet.
  const progInfo = await connection.getAccountInfo(ARENA_LEADERBOARD_PROGRAM_ID);
  if (!progInfo || !progInfo.executable) {
    console.error(
      `FATAL: arena_leaderboard (${ARENA_LEADERBOARD_PROGRAM_ID.toBase58()}) is ` +
        `not deployed/executable on ${RPC_URL}. Bring up a localnet first, e.g.:\n` +
        `  solana-test-validator --reset \\\n` +
        `    --bpf-program ${ARENA_LEADERBOARD_PROGRAM_ID.toBase58()} ` +
        `target/deploy/arena_leaderboard.so`
    );
    process.exit(1);
  }

  const program = new Program(
    LEADERBOARD_IDL as anchor.Idl,
    provider
  ) as Program;
  const programId = ARENA_LEADERBOARD_PROGRAM_ID;

  const airdrop = async (to: anchor.web3.PublicKey, sol = 5) => {
    const sig = await connection.requestAirdrop(
      to,
      sol * anchor.web3.LAMPORTS_PER_SOL
    );
    const bh = await connection.getLatestBlockhash();
    await connection.confirmTransaction({ signature: sig, ...bh }, "confirmed");
  };

  // Fund the payer if the validator handed it nothing.
  if ((await connection.getBalance(wallet.publicKey)) < anchor.web3.LAMPORTS_PER_SOL) {
    await airdrop(wallet.publicKey, 100).catch(() => {});
  }

  // Send a shared-built ix, letting the payer wallet cover fees and `signers`
  // provide any additional required signatures (the burner / self-signing).
  const sendIx = async (
    ix: anchor.web3.TransactionInstruction,
    signers: anchor.web3.Keypair[]
  ) => {
    const tx = new anchor.web3.Transaction().add(ix);
    return provider.sendAndConfirm(tx, signers);
  };

  type Board = Awaited<ReturnType<typeof program.account.leaderboard.fetch>>;
  const assertMinHeap = (board: Board) => {
    for (let i = 1; i < board.size; i++) {
      const parent = Math.floor((i - 1) / 2);
      if (board.entries[parent].rating > board.entries[i].rating) {
        check(
          `min-heap invariant at index ${i}`,
          false,
          `parent ${board.entries[parent].rating} > child ${board.entries[i].rating}`
        );
      }
    }
  };
  const fetchBoard = async () => {
    const board = await program.account.leaderboard.fetch(boardKp.publicKey);
    assertMinHeap(board);
    return board;
  };
  const topPlayers = (board: Board) =>
    board.entries.slice(0, board.size).map((e: any) => e.player.toBase58());

  // =========================================================================
  // STEP 1 — create + init the board (capacity 3 to exercise eviction fast).
  // =========================================================================
  console.log("\n[1] Creating + initializing the board (capacity 3)…");
  const CAPACITY = 3;
  const boardKp = anchor.web3.Keypair.generate();
  const lamports = await connection.getMinimumBalanceForRentExemption(
    LEADERBOARD_LEN
  );
  const createIx = anchor.web3.SystemProgram.createAccount({
    fromPubkey: wallet.publicKey,
    newAccountPubkey: boardKp.publicKey,
    lamports,
    space: LEADERBOARD_LEN,
    programId,
  });
  await program.methods
    .initLeaderboard(CAPACITY)
    .accountsPartial({
      leaderboard: boardKp.publicKey,
      authority: wallet.publicKey,
    })
    .preInstructions([createIx])
    .signers([boardKp])
    .rpc();

  let board = await fetchBoard();
  check("board authority == payer", board.authority.toBase58() === wallet.publicKey.toBase58());
  check("board capacity == 3", board.capacity === CAPACITY, String(board.capacity));
  check("board starts empty", board.size === 0, String(board.size));
  console.log("    board = " + boardKp.publicKey.toBase58());

  // =========================================================================
  // STEP 2 — the cast. hero uses the smooth (session-key) flow.
  // =========================================================================
  const hero = anchor.web3.Keypair.generate();
  const sessionKey = anchor.web3.Keypair.generate();
  const p2 = anchor.web3.Keypair.generate();
  const p3 = anchor.web3.Keypair.generate();
  const p4 = anchor.web3.Keypair.generate();
  await Promise.all(
    [hero, sessionKey, p2, p3, p4].map((kp) => airdrop(kp.publicKey, 5))
  );

  console.log("\n[2] hero registers a session (burner) key from its wallet…");
  const registerIx = await buildRegisterSessionKeyIx(program as any, {
    programId,
    player: hero.publicKey,
    sessionKey: sessionKey.publicKey,
  });
  await sendIx(registerIx, [hero]);
  const heroStatsPda = playerStatsPda(programId, hero.publicKey);
  {
    const stats = await program.account.playerStats.fetch(heroStatsPda);
    check(
      "hero.session_key == burner",
      stats.sessionKey?.toBase58() === sessionKey.publicKey.toBase58(),
      stats.sessionKey?.toBase58()
    );
  }
  console.log("    hero        = " + hero.publicKey.toBase58());
  console.log("    session key = " + sessionKey.publicKey.toBase58());

  // =========================================================================
  // STEP 3 — record battles via the SHARED web ix builders.
  //   hero: 3 wins vs bot → 1000 + 3*10 = 1030, wins 3 (burner-signed, no popup)
  //   p2:   win vs player  → 1025
  //   p3:   win vs bot     → 1010   (board now full at 3: hero, p2, p3)
  // =========================================================================
  console.log("\n[3] Recording battles through the web client's ix builders…");

  // Drive the hero's wins from a synthetic BattleResult so the SAME
  // win-derivation the web UI uses (`deriveBattleOutcome`) picks the outcome.
  const heroWinResult = { winnerMint: "HERO", avatarA: { mint: "HERO" } };
  const { win: heroWin } = deriveBattleOutcome(heroWinResult);
  for (let i = 0; i < 3; i++) {
    const ix = await buildRecordBattleIx(program as any, {
      programId,
      board: boardKp.publicKey,
      player: hero.publicKey,
      signer: sessionKey.publicKey, // burner signs — smooth flow
      win: heroWin,
      opponentIsBot: true,
    });
    await sendIx(ix, [sessionKey]);
    await fetchBoard(); // invariant after every op
  }

  const selfBattle = async (
    player: anchor.web3.Keypair,
    win: boolean,
    opponentIsBot: boolean
  ) => {
    const ix = await buildRecordBattleIx(program as any, {
      programId,
      board: boardKp.publicKey,
      player: player.publicKey,
      signer: player.publicKey,
      win,
      opponentIsBot,
    });
    await sendIx(ix, [player]);
    await fetchBoard();
  };

  await selfBattle(p2, true, false); // +25 → 1025
  await selfBattle(p3, true, true); //  +10 → 1010

  board = await fetchBoard();
  check("board full (size == 3)", board.size === 3, String(board.size));
  check(
    "top list == {hero, p2, p3}",
    JSON.stringify(topPlayers(board).sort()) ===
      JSON.stringify(
        [hero, p2, p3].map((k) => k.publicKey.toBase58()).sort()
      )
  );
  check(
    "root is the weakest of the top (p3 @ 1010)",
    board.entries[0].player.toBase58() === p3.publicKey.toBase58() &&
      board.entries[0].rating === 1010,
    `${board.entries[0].player.toBase58()} @ ${board.entries[0].rating}`
  );

  // hero heap entry mirrors the elo-lite math exactly.
  const heroEntry = board.entries
    .slice(0, board.size)
    .find((e: any) => e.player.equals(hero.publicKey));
  check("hero in top list", !!heroEntry);
  check("hero rating == 1030", heroEntry?.rating === 1030, String(heroEntry?.rating));
  check("hero wins == 3", heroEntry?.wins === 3, String(heroEntry?.wins));

  // =========================================================================
  // STEP 4 — a stronger challenger evicts the weakest incumbent.
  //   p4: 2 wins vs player → 1050 > root 1010 → p4 replaces p3.
  // =========================================================================
  console.log("\n[4] Stronger challenger evicts the weakest of the top…");
  await selfBattle(p4, true, false); // 1025 (< root? root 1010 → 1025 evicts p3)
  await selfBattle(p4, true, false); // 1050 (updates p4 in place)
  board = await fetchBoard();
  check("board still size 3", board.size === 3, String(board.size));
  check("p3 evicted", !topPlayers(board).includes(p3.publicKey.toBase58()));
  check("p4 now in top list", topPlayers(board).includes(p4.publicKey.toBase58()));
  check(
    "no duplicate p4 entry (in-place update)",
    topPlayers(board).filter((p) => p === p4.publicKey.toBase58()).length === 1
  );
  // p3 keeps its PlayerStats even after eviction.
  const p3Stats = await program.account.playerStats.fetch(
    playerStatsPda(programId, p3.publicKey)
  );
  check("evicted p3 keeps rating 1010", p3Stats.rating === 1010, String(p3Stats.rating));

  // =========================================================================
  // STEP 5 — set_profile (top-list perk) for the hero, then read it back.
  // =========================================================================
  console.log("\n[5] hero sets a profile (name + link) and we read it back…");
  const NAME = "Wotori the Brave";
  const URI = "https://ekza.io/u/wotori";
  const profileIx = await buildSetProfileIx(program as any, {
    programId,
    board: boardKp.publicKey,
    player: hero.publicKey,
    name: NAME,
    uri: URI,
  });
  await sendIx(profileIx, [hero]);
  const fixedUtf8 = (bytes: number[]) =>
    Buffer.from(bytes).toString("utf8").replace(/\0+$/, "");
  const heroStats = await program.account.playerStats.fetch(heroStatsPda);
  check("profile_name reads back", fixedUtf8(heroStats.profileName as number[]) === NAME, fixedUtf8(heroStats.profileName as number[]));
  check("profile_uri reads back", fixedUtf8(heroStats.profileUri as number[]) === URI, fixedUtf8(heroStats.profileUri as number[]));

  // =========================================================================
  // VERDICT
  // =========================================================================
  const finalBoard = await fetchBoard();
  console.log("\n=== PROOF (on-chain addresses) ===");
  console.log("leaderboard_board  = " + boardKp.publicKey.toBase58());
  console.log("hero_wallet        = " + hero.publicKey.toBase58());
  console.log("hero_session_key   = " + sessionKey.publicKey.toBase58());
  console.log("hero_stats_pda     = " + heroStatsPda.toBase58());
  console.log("top_list           = " + JSON.stringify(topPlayers(finalBoard)));
  console.log(
    "top_ratings        = " +
      JSON.stringify(
        finalBoard.entries.slice(0, finalBoard.size).map((e: any) => e.rating)
      )
  );

  if (failures === 0) {
    console.log("\n==================== BATTLE→LEADERBOARD: PASS ====================");
    process.exit(0);
  } else {
    console.log(`\n============== BATTLE→LEADERBOARD: FAIL (${failures}) ==============`);
    process.exit(1);
  }
}

main().catch((e) => {
  console.error("\nBATTLE→LEADERBOARD: FAIL (exception)");
  console.error(e);
  process.exit(1);
});
