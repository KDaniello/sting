//! The terminal surface over feature extraction: read a range of blocks, write a partition.

use std::path::Path;

use clap::{Parser, Subcommand};
use sting_sybil_features::persist::write_partition;
use sting_sybil_features::{
    FeatureExtractionConfig, Result, TopologyThresholds, build_record_batch,
    extract_features_for_range,
};
use sting_sybil_graph::{ExclusionCfg, load_gas_graph_from_range};
use sting_sybil_ingestion::ClickHouseStore;
use sting_sybil_types::AccountKind;

#[derive(Parser)]
#[command(name = "sting-sybil-features")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extracts features for every owner active in a range of masterchain blocks and writes one
    /// partition.
    ///
    /// What is real on live data today and what is not: the gas graph, the shapes read off it and
    /// the temporal features of an owner's own transactions are complete. The value graph is not
    /// built at all, because resolving a jetton wallet needs a chain client that exists nowhere
    /// but in test doubles — so the jetton figures come out zero. Every address is treated as a
    /// plain wallet, since there is no catalogue of code hashes to say otherwise, and the
    /// exclusion lists start empty.
    Extract {
        /// First masterchain sequence number.
        #[arg(long)]
        from: u32,
        /// Last masterchain sequence number, inclusive.
        #[arg(long)]
        to: u32,
        /// Which version of the vocabulary this run produces.
        #[arg(long, default_value = "v1")]
        feature_version: String,
        /// Where the raw rows are.
        ///
        /// The loopback address rather than the name: `localhost` resolves to the IPv6 loopback
        /// first on several platforms, and a container publishing its port on IPv4 alone is then
        /// unreachable under a name that looks right.
        #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://127.0.0.1:8123")]
        clickhouse_url: String,
        /// Where the partition goes.
        #[arg(long, default_value = "features")]
        out_dir: String,
        /// How much fan-out makes an address a star.
        #[arg(long, default_value_t = 20)]
        star_threshold: usize,
        /// How much makes it a tree root.
        #[arg(long, default_value_t = 20)]
        tree_threshold: usize,
        /// How far below the mean gap counts as a burst.
        #[arg(long, default_value_t = 0.5)]
        burst_factor: f64,
        /// Above how many outgoing edges an address is a hub and is excluded.
        #[arg(long, default_value_t = 50)]
        hub_degree_threshold: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Extract {
            from,
            to,
            feature_version,
            clickhouse_url,
            out_dir,
            star_threshold,
            tree_threshold,
            burst_factor,
            hub_degree_threshold,
        } => {
            if from > to {
                return Err(sting_sybil_features::FeatureError::BackwardsRange { from, to });
            }

            let store = ClickHouseStore::new(&clickhouse_url);
            let exclusion_cfg = ExclusionCfg {
                hub_degree_threshold,
                ..ExclusionCfg::default()
            };
            let account_kind = |_owner: sting_sybil_types::OwnerId| AccountKind::Wallet;
            let (graphs, txs, messages) =
                load_gas_graph_from_range(&store, from, to, &exclusion_cfg, account_kind).await?;
            eprintln!(
                "extract {from}..={to}: {} transactions and {} messages read",
                txs.len(),
                messages.len()
            );

            // The snapshot is the last transaction seen in the range rather than the clock at
            // launch, so the same range extracted twice produces the same rows.
            let snapshot_ts = txs.iter().map(|tx| tx.now).max().unwrap_or(0);

            let cfg = FeatureExtractionConfig {
                feature_version,
                snapshot_ts,
                burst_factor,
                topology: TopologyThresholds {
                    star_fan_threshold: star_threshold,
                    tree_fan_threshold: tree_threshold,
                },
            };
            let rows = extract_features_for_range(&txs, &messages, &[], &graphs, &cfg);
            eprintln!("{} feature rows", rows.len());

            let record_batch = build_record_batch(&rows)?;
            let path = write_partition(Path::new(&out_dir), &record_batch)?;
            println!("{} rows written to {}", rows.len(), path.display());
        }
    }
    Ok(())
}
