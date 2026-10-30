//! The terminal surface over ingestion: replay a range, or put the schema in place.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use sting_sybil_ingestion::{
    BlockBatch, BlockSource, ClickHouseStore, FakeBlockSource, InMemoryStore, IngestError, RawTx,
    Result, Snapshot, Store, StoreSummary, TonCenterV3Source, ingest_range,
};
use sting_sybil_types::{Finality, TonAddress};

#[derive(Parser)]
#[command(name = "sting-sybil-ingestion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Replays a range of masterchain blocks. With no flags the blocks are synthetic and the
    /// rows stay in memory, which is the form that reproduces exactly.
    Replay {
        /// First masterchain sequence number.
        #[arg(long)]
        from: u32,
        /// Last masterchain sequence number, inclusive.
        #[arg(long)]
        to: u32,
        /// Read real mainnet blocks instead of synthetic ones.
        #[arg(long)]
        live: bool,
        /// Write to the column store instead of memory.
        #[arg(long)]
        to_clickhouse: bool,
    },
    /// Replays a fixed range and writes down every row it produced, or holds a new
    /// replay against a snapshot already written.
    ///
    /// A range of finalized blocks is a fact that does not move, so a second replay
    /// of it has to produce the same file. When it does not, either the chain
    /// answered differently or this ingestion is not deterministic, and both are
    /// things whoever depends on it needs told.
    Snapshot {
        /// First masterchain sequence number.
        #[arg(long)]
        from: u32,
        /// Last masterchain sequence number, inclusive.
        #[arg(long)]
        to: u32,
        /// Read real mainnet blocks instead of synthetic ones.
        #[arg(long)]
        live: bool,
        /// Where to write the snapshot.
        #[arg(long)]
        out: Option<PathBuf>,
        /// A snapshot to hold this replay against, byte for byte.
        #[arg(long)]
        verify: Option<PathBuf>,
    },
    /// Creates the raw-store schema, and does nothing when it is already there.
    Migrate {
        /// Where the column store is.
        ///
        /// The loopback address rather than the name: `localhost` resolves to the IPv6 loopback
        /// first on several platforms, and a container publishing its port on IPv4 alone is then
        /// unreachable under a name that looks right.
        #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://127.0.0.1:8123")]
        clickhouse_url: String,
    },
}

/// A block that depends on nothing but its sequence number, so the same range replays to the same
/// rows on any machine and at any time.
fn synthetic_block(seqno: u32) -> BlockBatch {
    let n_txs = 1 + (seqno % 3);
    let txs = (0..n_txs)
        .map(|i| RawTx {
            account: TonAddress::new(0, [u8::try_from(seqno % 256).unwrap_or(0); 32]),
            lt: u64::from(seqno) * 1000 + u64::from(i),
            tx_hash: [u8::try_from(seqno.wrapping_add(i) % 256).unwrap_or(0); 32],
            mc_seqno: seqno,
            now: seqno,
            finality: Finality::Finalized,
        })
        .collect();
    BlockBatch {
        seqno,
        finality: Finality::Finalized,
        txs,
        messages: vec![],
    }
}

/// Replays a fixed range and either writes what it produced, holds it against a file, or prints it.
///
/// # Errors
/// When the range runs backwards, when the replay fails, or when the file it is held against does
/// not match byte for byte.
async fn run_snapshot(
    from: u32,
    to: u32,
    live: bool,
    out: Option<&std::path::Path>,
    verify: Option<&std::path::Path>,
) -> Result<()> {
    if from > to {
        return Err(IngestError::BackwardsRange { from, to });
    }
    let snapshot = replay_to_snapshot(from, to, live).await?;
    let bytes = snapshot
        .to_bytes()
        .map_err(|source| IngestError::Snapshot(source.to_string()))?;
    eprintln!(
        "{} transaction(s) and {} message(s) over {from}..={to}",
        snapshot.txs.len(),
        snapshot.messages.len()
    );

    if let Some(path) = out {
        std::fs::write(path, &bytes).map_err(|source| {
            IngestError::Snapshot(format!("writing {}: {source}", path.display()))
        })?;
        println!("wrote {}", path.display());
    }
    if let Some(path) = verify {
        let committed = std::fs::read(path).map_err(|source| {
            IngestError::Snapshot(format!("reading {}: {source}", path.display()))
        })?;
        if bytes != committed {
            let first = bytes
                .iter()
                .zip(committed.iter())
                .position(|(left, right)| left != right);
            return Err(IngestError::Snapshot(format!(
                "the replay does not reproduce {}: {} bytes against {}, first difference at {}",
                path.display(),
                bytes.len(),
                committed.len(),
                first.map_or_else(
                    || "the end of the shorter file".to_string(),
                    |at| at.to_string()
                )
            )));
        }
        println!(
            "the replay reproduces {} byte for byte ({} bytes)",
            path.display(),
            committed.len()
        );
    }
    if out.is_none() && verify.is_none() {
        println!("{}", String::from_utf8_lossy(&bytes));
    }
    Ok(())
}

/// Replays `from..=to` into memory and reads the whole of it back as a snapshot.
///
/// Into memory rather than into the column store, deliberately: what is being checked is that the
/// ingestion produces the same rows twice, and a store that returned rows in its own order would
/// answer a different question.
async fn replay_to_snapshot(from: u32, to: u32, live: bool) -> Result<Snapshot> {
    let store = InMemoryStore::default();
    let source_name = if live {
        "mainnet over v3 REST"
    } else {
        "synthetic blocks"
    };
    if live {
        ingest_range(&TonCenterV3Source::new(), &store, from, to).await?;
    } else {
        let source = FakeBlockSource::new((from..=to).map(synthetic_block).collect());
        ingest_range(&source, &store, from, to).await?;
    }
    let summary = store.summary().await?;
    Ok(Snapshot::of(&store, source_name, from, to, summary))
}

async fn run_and_summarize<S: BlockSource, St: Store>(
    source: S,
    store: St,
    from: u32,
    to: u32,
) -> Result<StoreSummary> {
    ingest_range(&source, &store, from, to).await?;
    store.summary().await
}

fn clickhouse_url_from_env() -> String {
    std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Replay {
            from,
            to,
            live,
            to_clickhouse,
        } => {
            if from > to {
                return Err(IngestError::BackwardsRange { from, to });
            }
            eprintln!(
                "replay {from}..={to}: {} into {}",
                if live {
                    "mainnet over v3 REST"
                } else {
                    "synthetic blocks"
                },
                if to_clickhouse {
                    "the column store"
                } else {
                    "memory"
                },
            );

            let summary = match (live, to_clickhouse) {
                (false, false) => {
                    let source = FakeBlockSource::new((from..=to).map(synthetic_block).collect());
                    run_and_summarize(source, InMemoryStore::default(), from, to).await?
                }
                (false, true) => {
                    let source = FakeBlockSource::new((from..=to).map(synthetic_block).collect());
                    let store = ClickHouseStore::new(&clickhouse_url_from_env());
                    store.migrate().await?;
                    run_and_summarize(source, store, from, to).await?
                }
                (true, false) => {
                    let source = TonCenterV3Source::new();
                    run_and_summarize(source, InMemoryStore::default(), from, to).await?
                }
                (true, true) => {
                    let source = TonCenterV3Source::new();
                    let store = ClickHouseStore::new(&clickhouse_url_from_env());
                    store.migrate().await?;
                    run_and_summarize(source, store, from, to).await?
                }
            };
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Command::Snapshot {
            from,
            to,
            live,
            out,
            verify,
        } => run_snapshot(from, to, live, out.as_deref(), verify.as_deref()).await?,
        Command::Migrate { clickhouse_url } => {
            let store = ClickHouseStore::new(&clickhouse_url);
            store.migrate().await?;
            println!("raw-store schema is in place at {clickhouse_url}");
        }
    }
    Ok(())
}
