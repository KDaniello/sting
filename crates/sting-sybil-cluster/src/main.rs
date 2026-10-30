//! The terminal surface over the structural half of clustering.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use sting_sybil_cluster::persist::write_partition;
use sting_sybil_cluster::schema::{ClusterMembershipRow, build_record_batch};
use sting_sybil_cluster::{ClusterError, Result, materialize_hard_clusters, retro};
use sting_sybil_graph::{ExclusionCfg, load_gas_graph_from_range};
use sting_sybil_ingestion::ClickHouseStore;
use sting_sybil_types::AccountKind;

#[derive(Parser)]
#[command(name = "sting-sybil-cluster")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Finds components over a range of masterchain blocks and writes one partition.
    ///
    /// What is written is the structural half only: the soft cluster and the agreement between
    /// the halves stay empty, and the side that computes them fills them in by replacing this
    /// same partition.
    /// Measures a campaign after the fact against the threshold registered before it.
    ///
    /// Scores arrive already computed: the pipeline that computes them is the one
    /// being judged, and a measurement that produced its own inputs would be
    /// marking its own work.
    Retro {
        /// Owner to risk, as JSON: `{"0:abc…": 0.81, …}`.
        #[arg(long)]
        scores: PathBuf,
        /// The campaign's own exclusion list, as a JSON array of owners.
        #[arg(long)]
        excluded: PathBuf,
        /// The registration, committed before this run.
        #[arg(long, default_value = "corpus/sybil/campaign/threshold.toml")]
        threshold: PathBuf,
    },
    Extract {
        /// First masterchain sequence number.
        #[arg(long)]
        from: u32,
        /// Last masterchain sequence number, inclusive.
        #[arg(long)]
        to: u32,
        /// Where the raw rows are.
        ///
        /// The loopback address rather than the name: `localhost` resolves to the IPv6 loopback
        /// first on several platforms, and a container publishing its port on IPv4 alone is then
        /// unreachable under a name that looks right.
        #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://127.0.0.1:8123")]
        clickhouse_url: String,
        /// Where the partition goes.
        #[arg(long, default_value = "cluster_membership")]
        out_dir: String,
        /// Above how many outgoing edges an address is a hub and is excluded.
        #[arg(long, default_value_t = 50)]
        hub_degree_threshold: usize,
        /// Past how many owners a component is broken down rather than reported whole.
        #[arg(long, default_value_t = 10_000)]
        giant_component_threshold: usize,
        /// How many neighbours a node must keep to survive that breaking down.
        #[arg(long, default_value_t = 2)]
        k_core_k: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Retro {
            scores,
            excluded,
            threshold,
        } => {
            let registered = retro::read_threshold(&threshold)?;
            let commit = retro::registration_commit(&threshold)?;
            // Both sides through one address model before anything is compared: an exclusion list
            // and a score file come from two places, and text equality between two spellings of one
            // account is a recall of zero that looks like a detector finding nothing.
            let scored: BTreeMap<String, f64> = read_json(&scores)?;
            let (scored, unreadable_scores) = normalise_scores(scored);
            let excluded: Vec<String> = read_json(&excluded)?;
            let (excluded, unreadable_excluded) = retro::normalise(excluded);
            for (what, unreadable) in [
                ("scored", unreadable_scores),
                ("excluded", unreadable_excluded),
            ] {
                if !unreadable.is_empty() {
                    eprintln!(
                        "{} of the {what} entries are not addresses this tool can read: {}",
                        unreadable.len(),
                        unreadable
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            let result = retro::compare(&scored, &excluded, &registered, &commit);

            println!(
                "{}: {} scored owner(s), {} excluded by the campaign",
                result.campaign, result.scored_owners, result.excluded_owners
            );
            println!(
                "  precision {:.3}, recall {:.3} — {} true, {} false, {} missed",
                result.precision,
                result.recall,
                result.true_positives,
                result.false_positives,
                result.false_negatives
            );
            println!("  against the threshold registered in {commit}");
            for failure in &result.failures {
                println!("  {failure}");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&result)
                    .map_err(|source| ClusterError::Retro(source.to_string()))?
            );
            // The result is published either way, and the exit code says which way it landed.
            if !result.passed {
                return Err(ClusterError::Retro(
                    "the campaign did not meet what was registered".to_string(),
                ));
            }
        }
        Command::Extract {
            from,
            to,
            clickhouse_url,
            out_dir,
            hub_degree_threshold,
            giant_component_threshold,
            k_core_k,
        } => {
            if from > to {
                return Err(ClusterError::BackwardsRange { from, to });
            }

            let store = ClickHouseStore::new(&clickhouse_url);
            let exclusion_cfg = ExclusionCfg {
                hub_degree_threshold,
                ..ExclusionCfg::default()
            };
            let account_kind = |_owner: sting_sybil_types::OwnerId| AccountKind::Wallet;
            let (graphs, txs, _messages) =
                load_gas_graph_from_range(&store, from, to, &exclusion_cfg, account_kind).await?;
            eprintln!(
                "cluster {from}..={to}: {} owners in the gas graph after exclusion",
                graphs.gas_node_count()
            );

            let snapshot_ts = txs.iter().map(|tx| tx.now).max().unwrap_or(0);
            let hard = materialize_hard_clusters(&graphs.gas, giant_component_threshold, k_core_k);

            let rows: Vec<ClusterMembershipRow> = hard
                .into_iter()
                .map(|(owner, info)| ClusterMembershipRow {
                    owner,
                    wcc_id: info.wcc_id,
                    wcc_size: info.wcc_size,
                    snapshot_ts,
                })
                .collect();
            eprintln!("{} membership rows", rows.len());

            let record_batch = build_record_batch(&rows)?;
            let path = write_partition(Path::new(&out_dir), &record_batch)?;
            println!("{} rows written to {}", rows.len(), path.display());
        }
    }
    Ok(())
}

/// Reads one JSON file into whatever shape the caller asked for.
///
/// Named separately so that a missing file and a malformed one both say which file: a retrospective
/// measurement has three inputs, and "invalid JSON" without a path is a message that costs its
/// reader a search.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .map_err(|source| ClusterError::Retro(format!("reading {}: {source}", path.display())))?;
    serde_json::from_str(&text).map_err(|source| {
        ClusterError::Retro(format!(
            "{} is not what it should be: {source}",
            path.display()
        ))
    })
}

/// Reads a score file into the one address form, keeping what could not be read.
///
/// A score whose owner is not an address is not a score about anybody, and dropping it silently
/// would shrink the denominator of a precision without saying so.
fn normalise_scores(scores: BTreeMap<String, f64>) -> (BTreeMap<String, f64>, Vec<String>) {
    let mut read = BTreeMap::new();
    let mut unreadable = Vec::new();
    for (owner, risk) in scores {
        let (parsed, failed) = retro::normalise(vec![owner.clone()]);
        match parsed.first() {
            Some(address) => {
                read.insert(address.clone(), risk);
            }
            None => unreadable.extend(failed),
        }
    }
    (read, unreadable)
}
