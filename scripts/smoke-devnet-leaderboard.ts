/** One wallet-signed battle against the persistent public devnet board. */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import * as fs from "fs";

const DEVNET_GENESIS_HASH = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const LEADERBOARD_PROGRAM_ID = new anchor.web3.PublicKey(
  "9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU"
);

function requiredEnv(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function loadKeypair(file: string): anchor.web3.Keypair {
  return anchor.web3.Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(fs.readFileSync(file, "utf8")))
  );
}

async function main() {
  const rpcUrl = requiredEnv("RPC_URL");
  const walletKeypair = loadKeypair(requiredEnv("WALLET"));
  const boardAddress = new anchor.web3.PublicKey(requiredEnv("BOARD_ADDRESS"));
  const connection = new anchor.web3.Connection(rpcUrl, "confirmed");
  const genesisHash = await connection.getGenesisHash();
  if (genesisHash !== DEVNET_GENESIS_HASH) {
    throw new Error(`refusing non-devnet genesis ${genesisHash}`);
  }

  const wallet = new anchor.Wallet(walletKeypair);
  const provider = new anchor.AnchorProvider(connection, wallet, {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });
  const idl = require("../target/idl/arena_leaderboard.json");
  const program = new Program(idl as anchor.Idl, provider) as Program;
  if (!program.programId.equals(LEADERBOARD_PROGRAM_ID)) {
    throw new Error(`IDL program mismatch: ${program.programId.toBase58()}`);
  }

  const [playerStats] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("player_stats_v1"), wallet.publicKey.toBuffer()],
    LEADERBOARD_PROGRAM_ID
  );
  const [battleRateLimit] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("battle_rate_limit_v1"), wallet.publicKey.toBuffer()],
    LEADERBOARD_PROGRAM_ID
  );

  const accounts = (program as any).account;
  const before = await accounts.playerStats.fetchNullable(playerStats);
  const signature = await (program.methods as any)
    .recordBattle(true, true)
    .accountsStrict({
      leaderboard: boardAddress,
      playerStats,
      battleRateLimit,
      player: wallet.publicKey,
      signer: wallet.publicKey,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .rpc();

  const after = await accounts.playerStats.fetch(playerStats);
  const board = await accounts.leaderboard.fetch(boardAddress);
  const beforeGames = before?.games || 0;
  const beforeWins = before?.wins || 0;
  if (after.games !== beforeGames + 1 || after.wins !== beforeWins + 1) {
    throw new Error("battle counters did not advance exactly once");
  }
  const present = board.entries
    .slice(0, board.size)
    .some((entry: { player: anchor.web3.PublicKey }) =>
      entry.player.equals(wallet.publicKey)
    );
  if (!present)
    throw new Error("player is absent from leaderboard after battle");

  console.log(
    JSON.stringify(
      {
        cluster: "devnet",
        genesisHash,
        signature,
        board: boardAddress.toBase58(),
        player: wallet.publicKey.toBase58(),
        playerStats: playerStats.toBase58(),
        battleRateLimit: battleRateLimit.toBase58(),
        games: after.games,
        wins: after.wins,
        rating: after.rating,
        boardSize: board.size,
      },
      null,
      2
    )
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
