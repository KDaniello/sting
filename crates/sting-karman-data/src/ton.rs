//! Pulling a market of assets that are held rather than traded into a census.
//!
//! What crosses this boundary is an indexer's answer to four questions — which collections a
//! platform issues, what each one holds, what changed hands inside a window, and what was paid
//! when something did — and what leaves it is one plain value with its origin attached.
//!
//! The decoding is here for the same reason the lending adapter's is: a miscount is silent. A
//! page counted twice, an item whose owner is a marketplace escrow rather than a person, a
//! transfer that is the second leg of one sale — each produces a turnover figure that is wrong
//! and entirely readable, and a mechanism calibrated against it would be calibrated against
//! nothing.
//!
//! The transport is not here, for the reason given on [the crate](crate): reaching an indexer is
//! paging, pacing and retrying, and none of that belongs in the dependency path of a simulator.
//! The reaching is [`crate::census_sweep`], which writes one JSON object per line, and this
//! module folds those lines.
//!
//! The calls, and what each answer contributes:
//!
//! ```text
//! toncenter/api/v3/masterchainInfo                                 where the chain stood
//! toncenter/api/v3/nft/collections?limit&offset                    which collections exist,
//!                                                                  and which of them are copies
//! tonapi/v2/nfts/collections/<id>/items?limit&offset               what one holds, who holds it,
//!                                                                  and what it is listed at
//! toncenter/api/v3/nft/transfers?collection_address&start_utime     what changed hands
//!                               &sort=desc&end_lt                   ... paged by cursor
//! toncenter/api/v3/actions?action_type=nft_purchase&sort=desc&end_lt  what was paid
//!
//! sweep-ton-census --into out/census --days 90 --served-by <host> \
//!   | cargo run -p sting-karman-data --bin pull-ton-census -- corpus/karman/fixtures/<name>.json
//! ```
//!
//! Two indexers rather than one, and the split is forced rather than chosen: the first will not
//! page a collection of this market past about ninety thousand items — it answers a server error
//! and keeps answering it — and the second will not list transfers or purchases at all. Each is
//! asked only what it can answer, and what each answered travels with the census.
//!
//! Two counts are deliberately not the same number. **Transfers** are messages: one sale through
//! an escrow marketplace produces two of them, and a platform moving its own stock produces one
//! that nobody traded. **Moved items** are distinct items that appeared in at least one transfer,
//! which is free of that inflation and is therefore what turnover is measured on.

use std::fmt::Write as _;

use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sting_karman_types::u128_str;
use thiserror::Error;

/// An answer could not be read as the value it was asked for, or the pages do not reconcile.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CensusError {
    /// A field that names an account did not hold one.
    #[error("{field} is not an account address: {found}")]
    Address {
        /// Which field.
        field: &'static str,
        /// What was there instead.
        found: String,
    },
    /// A page carried rows for a collection the census does not cover.
    #[error("a page names collection {collection}, which this census did not discover")]
    Unnamed {
        /// The collection the rows claim.
        collection: String,
    },
    /// A collection answered something that is not an index for the item it would mint next.
    #[error("a collection counts its next item at {found}, which is not an index")]
    Sequence {
        /// What stood where the index does.
        found: String,
    },
    /// A price did not fit the width money is carried in, or was not a number at all.
    #[error("a purchase was settled at something that is not a quantity: {found}")]
    Price {
        /// What stood where the price does.
        found: String,
    },
    /// An action arrived that is not the one the sweep asked for.
    #[error("expected purchases, found an action of type {kind}")]
    Unexpected {
        /// The type the row declared.
        kind: String,
    },
    /// A purchase of a collection this census covers named no item or no price.
    #[error("a purchase of a covered collection names no {missing}")]
    Anonymous {
        /// Which was absent.
        missing: &'static str,
    },
    /// A page asked about one collection answered about another.
    #[error("a page about {expected} carries a row of {found}")]
    Misfiled {
        /// The collection the page was asked about.
        expected: String,
        /// The collection a row claims.
        found: String,
    },
    /// The sweep declares a window that runs backwards.
    #[error("the window {from}..={to} runs backwards")]
    Backwards {
        /// The declared start.
        from: u64,
        /// The declared end.
        to: u64,
    },
    /// Nothing was discovered, so there is nothing to reconcile and nothing to report.
    #[error("no collection matched what this census was asked to cover")]
    Empty,
}

/// An account on TON: the workchain it lives in, and the hash that names it there.
///
/// Held as the thirty-two bytes it is rather than as the text it arrives in. A census of this
/// market holds several hundred thousand of these at once — every item, every holder, everything
/// that moved — and at that count the difference between a fixed-size value and a heap string
/// decides whether the tool runs on an ordinary machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId {
    /// Which workchain. Zero is the basechain, where everything this census measures lives.
    workchain: i32,
    /// The account's own 256-bit identity within it.
    hash: [u8; 32],
}

impl AccountId {
    /// The workchain this account lives in.
    #[must_use]
    pub const fn workchain(&self) -> i32 {
        self.workchain
    }
}

/// An account address was not `<workchain>:<64 hex digits>`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("not a raw account address: {found}")]
pub struct NotAnAccount {
    /// What was there instead.
    pub found: String,
}

impl FromStr for AccountId {
    type Err = NotAnAccount;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (workchain, body) = text.split_once(':').ok_or_else(|| NotAnAccount {
            found: text.to_owned(),
        })?;
        let workchain: i32 = workchain.parse().map_err(|_| NotAnAccount {
            found: text.to_owned(),
        })?;
        if body.len() != 64 {
            return Err(NotAnAccount {
                found: text.to_owned(),
            });
        }
        let mut hash = [0u8; 32];
        for (byte, at) in hash.iter_mut().zip((0..64).step_by(2)) {
            *byte = u8::from_str_radix(&body[at..at + 2], 16).map_err(|_| NotAnAccount {
                found: text.to_owned(),
            })?;
        }
        Ok(AccountId { workchain, hash })
    }
}

impl Display for AccountId {
    fn fmt(&self, into: &mut Formatter<'_>) -> fmt::Result {
        write!(into, "{}:", self.workchain)?;
        for byte in self.hash {
            write!(into, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for AccountId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

/// The sweep a census came from, in enough detail to ask the same questions again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sweep {
    /// The chain that answered.
    pub network: String,
    /// The indexer that answered for it.
    pub source: String,
    /// Where the chain stood when the sweep began.
    ///
    /// A sweep of this size takes longer than a block, so the chain moves under it. Both ends are
    /// recorded rather than one: the distance between them is how much reality the census could
    /// not hold still, and a reader is entitled to it.
    pub seqno_before: u32,
    /// Where it stood when the sweep ended.
    pub seqno_after: u32,
    /// The first second of the window turnover is measured over.
    pub window_start: u64,
    /// The last.
    pub window_end: u64,
}

/// One holder, and how much of a collection it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolderCount {
    /// Who holds.
    pub holder: AccountId,
    /// How many items.
    pub items: u32,
}

/// What was actually paid for a collection's items inside the window.
///
/// Every figure is in nanotons, the base unit the chain counts its own asset in.
///
/// Percentiles by nearest rank over the integer prices themselves, so the figures are exact and
/// identical on every machine — the discipline every distribution in this workspace is held to
/// (karman/docs/REPORTING.md §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prices {
    /// How many sales the prices are drawn from.
    pub sales: u32,
    /// What they add up to.
    #[serde(with = "u128_str")]
    pub total: u128,
    /// The cheapest.
    #[serde(with = "u128_str")]
    pub min: u128,
    /// The lower quartile.
    #[serde(with = "u128_str")]
    pub p25: u128,
    /// The middle.
    #[serde(with = "u128_str")]
    pub median: u128,
    /// The upper quartile.
    #[serde(with = "u128_str")]
    pub p75: u128,
    /// The dearest.
    #[serde(with = "u128_str")]
    pub max: u128,
}

impl Prices {
    /// Summarises what a collection's sales paid. Sorts `paid` in place.
    ///
    /// The total saturates rather than wraps. It cannot be reached — every price on this chain is
    /// bounded by a supply of about `5e18` nanotons and there are fewer than `2^32` of them — and
    /// a total that wrapped would put a small number where a huge one belongs, which is the one
    /// failure a reader cannot see.
    fn of(paid: &mut [u128]) -> Option<Prices> {
        let last = paid.len().checked_sub(1)?;
        paid.sort_unstable();
        Some(Prices {
            sales: u32::try_from(paid.len()).unwrap_or(u32::MAX),
            total: paid
                .iter()
                .fold(0u128, |sum, price| sum.saturating_add(*price)),
            min: paid[0],
            p25: nearest_rank(paid, 25),
            median: nearest_rank(paid, 50),
            p75: nearest_rank(paid, 75),
            max: paid[last],
        })
    }
}

/// The value at a percentile by nearest rank: the smallest observation at or above which that
/// share of the sorted sample lies. Integer throughout, so no two machines can disagree about it.
fn nearest_rank(sorted: &[u128], percentile: usize) -> u128 {
    let rank = (sorted.len() * percentile)
        .div_ceil(100)
        .clamp(1, sorted.len());
    sorted[rank - 1]
}

/// One collection, as the chain answered for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionCensus {
    /// The collection contract.
    pub collection: AccountId,
    /// What it calls itself.
    pub name: String,
    /// Where its own description is served from — the mark a census selects collections by.
    pub content: String,
    /// Who can still act on the collection, absent where it renounced that.
    ///
    /// This field and the one below it are here because serving a description from a platform's
    /// host is not proof of being that platform's collection: anyone can point a collection at
    /// somebody else's description, and on this market many have. What an issuer deploys and then
    /// walks away from has no owner and mints no sequence; a copy of it is owned by whoever made
    /// it and counts its items from zero. Neither field decides anything here — a census records
    /// what the chain says and leaves the judgement to whoever reads it — but without them the
    /// count of a market includes its imitations and nothing says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<AccountId>,
    /// The index the collection would mint next, absent where it does not mint in sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// How many items of it exist on chain.
    pub items: u32,
    /// How many were read more than once because the chain moved under the sweep.
    ///
    /// Paging a live collection by offset can show one item twice and another not at all. The
    /// duplicates are removed; this is how many there were, and it is the honest measure of how
    /// far the count above may be from the one a frozen chain would have given.
    pub restated: u32,
    /// How many sit in a marketplace's escrow with a price attached.
    pub on_sale: u32,
    /// How many distinct addresses hold one.
    pub holders: u32,
    /// How much of it the census's named issuers hold between them.
    pub issuer_held: u32,
    /// The largest holders, longest first.
    pub largest_holders: Vec<HolderCount>,
    /// How many ownership messages the window carried.
    pub transfers: u32,
    /// How many distinct items appeared in at least one of them.
    pub moved: u32,
    /// How many distinct items were bought for a price the chain records.
    pub sold: u32,
    /// What those purchases paid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
    /// What the items in escrow are asking, for those asking in the chain's own asset.
    ///
    /// Not every listing does: a marketplace will take a price in some other token, and adding
    /// those to these would be adding two units. `sales` here against [`on_sale`](Self::on_sale)
    /// is how many of the listings this summarises.
    ///
    /// These are the only prices this market states of its own accord, and every one of them is an
    /// offer nobody has to accept — which is the whole of what
    /// [`DEAD-CAPITAL.md`](https://github.com/karman-dev/karman/blob/main/karman/docs/DEAD-CAPITAL.md)
    /// §2.1 says about a price without a trade behind it. They are recorded because a mechanism
    /// that turns declarations into obligations has to be calibrated against the declarations that
    /// exist before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking: Option<Prices>,
}

/// What the chain says about a collection itself, as opposed to about what it holds.
///
/// A named struct rather than four arguments, because two of the four are addresses and two are
/// text, and a boundary that let them be swapped by position would swap them eventually.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    /// What it calls itself.
    pub name: String,
    /// Where its own description is served from.
    pub content: String,
    /// Who can still act on it, absent where it renounced that.
    #[serde(default)]
    pub owner: Option<AccountId>,
    /// What it answered for the index it would mint next, exactly as it answered it.
    ///
    /// Text rather than a number, because "does not mint in sequence" and "answered something
    /// nobody can read" are two different things and only one of them is a fact about the market.
    /// Telling them apart is reading, and reading happens here.
    #[serde(default)]
    pub next_index: Option<String>,
}

/// A market of held assets, as one sweep found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Census {
    /// Where this came from.
    pub sweep: Sweep,
    /// The addresses whose holdings are stock rather than ownership.
    ///
    /// A list rather than one address, because a census of several classes of asset covers
    /// several issuers, and a class whose issuer went unnamed would report its undistributed
    /// stock as a float that never moves — which is the exact shape of the finding this census
    /// exists to produce, arrived at by not looking.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issuers: Vec<AccountId>,
    /// How many rows the sweep read and did not count, because they happened outside the window.
    ///
    /// The chain does not stop for a sweep. Reading the recent end of a listing first means the
    /// first page of every walk is newer than the window, and the last page reaches past its
    /// start; both are dropped here. A number far above the pages that bound the walks means the
    /// window and the sweep disagree about what was being measured.
    pub outside_window: u32,
    /// The collections it covers, in the order they were discovered.
    pub collections: Vec<CollectionCensus>,
}

impl CollectionCensus {
    /// The items somebody other than an issuer holds: the denominator of turnover.
    ///
    /// Stock still sitting at an issuer has not entered the market, and counting it as unsold
    /// would blame the market for a decision the issuer made.
    #[must_use]
    pub const fn float(&self) -> u32 {
        self.items.saturating_sub(self.issuer_held)
    }

    /// Whether the collection has no owner and mints no sequence.
    ///
    /// What an issuer deploys and then walks away from looks like this, and a copy of it — same
    /// description, same name, different contract — does not: somebody still owns the copy, and it
    /// counts its items from zero. Neither is proof, which is why this is a question a reader asks
    /// of the census rather than a filter the census applies.
    #[must_use]
    pub const fn renounced(&self) -> bool {
        self.owner.is_none() && self.sequence.is_none()
    }

    /// How much of the float changed hands, in basis points.
    ///
    /// Integer, and against the float rather than against everything on chain, for the reason
    /// [`float`](Self::float) gives. A collection whose float is nothing has no turnover to state,
    /// which is not the same as a turnover of zero.
    #[must_use]
    pub fn turnover_bps(&self) -> Option<u32> {
        turnover_bps(u64::from(self.moved), u64::from(self.float()))
    }
}

impl Census {
    /// Renders the census as the tables a reader would ask for.
    ///
    /// A value to a string and nothing else: writing it is the caller's business, as everywhere
    /// in this workspace. It lives here rather than in `sting-karman-report` because that crate reports
    /// on runs, and this is a measurement of a chain — the two share a discipline and no types.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(1 << 14);
        self.render_identity(&mut out);
        self.render_totals(&mut out);
        self.render_classes(&mut out);
        self.render_collections(&mut out);
        out
    }

    fn render_identity(&self, out: &mut String) {
        let days = (self.sweep.window_end - self.sweep.window_start) / 86_400;
        out.push_str("# Census of a market that is held rather than traded\n\n## Identity\n\n");
        out.push_str("| | |\n|---|---|\n");
        row(out, "Chain", &self.sweep.network);
        row(out, "Read from", &self.sweep.source);
        row(
            out,
            "Window",
            &format!(
                "{}..{} ({days} days)",
                self.sweep.window_start, self.sweep.window_end
            ),
        );
        row(
            out,
            "Chain moved during the sweep",
            &format!(
                "seqno {} to {} ({} blocks)",
                self.sweep.seqno_before,
                self.sweep.seqno_after,
                self.sweep
                    .seqno_after
                    .saturating_sub(self.sweep.seqno_before)
            ),
        );
        row(
            out,
            "Issuers named",
            &if self.issuers.is_empty() {
                "none".to_owned()
            } else {
                self.issuers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        );
        row(
            out,
            "Rows read outside the window and dropped",
            &self.outside_window.to_string(),
        );
        row(
            out,
            "Collections covered",
            &self.collections.len().to_string(),
        );
        let renounced = self.collections.iter().filter(|of| of.renounced()).count();
        row(
            out,
            "— with no owner and no sequence",
            &renounced.to_string(),
        );
        row(
            out,
            "— owned or numbered",
            &(self.collections.len() - renounced).to_string(),
        );
        // Renouncing narrows the field and does not close it: several collections here share a
        // description and have all been walked away from. What separates them further is whether
        // an issuer still holds stock of the thing — a fact, and still not a proof.
        row(
            out,
            "— of the first, with stock still at a named issuer",
            &self
                .collections
                .iter()
                .filter(|of| of.renounced() && of.issuer_held > 0)
                .count()
                .to_string(),
        );
        out.push('\n');
        out.push_str(
            "Serving a description from a platform's host is not proof of being that \
             platform's collection: several collections here share one, and the counts above \
             are what the chain says about each rather than a verdict on any. Every total \
             below is given both ways, because netting the two together is what produced the \
             numbers this census was built to replace.\n\n",
        );
    }

    fn render_totals(&self, out: &mut String) {
        out.push_str("## Totals\n\n");
        out.push_str(
            "| | No owner, no sequence | Owned or numbered | All |\n|---|---:|---:|---:|\n",
        );
        let of = |take: fn(&CollectionCensus) -> u64| {
            let sum = |answer: bool| {
                self.collections
                    .iter()
                    .filter(|it| it.renounced() == answer)
                    .map(take)
                    .sum::<u64>()
            };
            let (kept, copied) = (sum(true), sum(false));
            (kept, copied, kept + copied)
        };
        let mut line = |what: &str, take: fn(&CollectionCensus) -> u64| {
            let (kept, copied, all) = of(take);
            let _ = writeln!(out, "| {what} | {kept} | {copied} | {all} |");
        };
        line("Items on chain", |it| u64::from(it.items));
        line("Held by a named issuer", |it| u64::from(it.issuer_held));
        line("Float", |it| u64::from(it.float()));
        line("Priced on chain, in escrow", |it| u64::from(it.on_sale));
        line("Distinct holders, counted per collection", |it| {
            u64::from(it.holders)
        });
        line("Ownership messages in the window", |it| {
            u64::from(it.transfers)
        });
        line("Distinct items that moved", |it| u64::from(it.moved));
        line("Distinct items sold for a recorded price", |it| {
            u64::from(it.sold)
        });
        line("Listings quoting the chain's own asset", |it| {
            it.asking.as_ref().map_or(0, |ask| u64::from(ask.sales))
        });
        line("Read twice and counted once", |it| u64::from(it.restated));
        out.push('\n');
    }

    /// One line per class of asset, where a class is the path a platform serves its descriptions
    /// under.
    ///
    /// Grouping by that path rather than by anything a reader decides: gifts, usernames, numbers
    /// and domains are told apart by where their own descriptions live, which is a fact of the
    /// chain, and every collection claiming a path is counted under it — copies included, and
    /// named as such.
    fn render_classes(&self, out: &mut String) {
        out.push_str(
            "## By class\n\nA class is the path a platform serves its descriptions under.\n\n",
        );
        out.push_str(
            "| Class | Collections | Own | On chain | At issuer | Float | Moved | Turnover | \
             Priced | Sold |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        let mut classes: BTreeMap<&str, Vec<&CollectionCensus>> = BTreeMap::new();
        for of in &self.collections {
            classes.entry(class_of(&of.content)).or_default().push(of);
        }
        for (class, held) in classes {
            let sum =
                |take: fn(&CollectionCensus) -> u64| held.iter().copied().map(take).sum::<u64>();
            let (items, issuer_held) = (
                sum(|it| u64::from(it.items)),
                sum(|it| u64::from(it.issuer_held)),
            );
            let (float, moved) = (items - issuer_held, sum(|it| u64::from(it.moved)));
            let turnover = as_percent(turnover_bps(moved, float));
            let _ = writeln!(
                out,
                "| {class} | {} | {} | {items} | {issuer_held} | {float} | {moved} | {turnover} | {} | {} |",
                held.len(),
                held.iter().filter(|it| it.renounced()).count(),
                sum(|it| u64::from(it.on_sale)),
                sum(|it| u64::from(it.sold)),
            );
        }
        out.push('\n');
    }

    fn render_collections(&self, out: &mut String) {
        out.push_str("## By collection\n\nLongest first. `turnover` is distinct items moved against the float.\n\n");
        out.push_str(
            "| Collection | Own | On chain | At issuer | Float | Moved | Turnover | Msgs/moved | \
             Priced | Median asked | Sold | Median paid |\n\
             |---|:-:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        let mut order: Vec<&CollectionCensus> = self.collections.iter().collect();
        order.sort_by(|left, right| {
            right
                .items
                .cmp(&left.items)
                .then_with(|| left.collection.cmp(&right.collection))
        });
        for of in order {
            let name = format!(
                "{} `{}`",
                if of.name.is_empty() {
                    "(unnamed)"
                } else {
                    of.name.as_str()
                },
                &of.collection.to_string()[..10]
            );
            let turnover = as_percent(of.turnover_bps());
            let per_moved = if of.moved == 0 {
                "—".to_owned()
            } else {
                let hundredths = u64::from(of.transfers) * 100 / u64::from(of.moved);
                format!("{}.{:02}", hundredths / 100, hundredths % 100)
            };
            let middle = |what: Option<&Prices>| {
                what.map_or_else(|| "—".to_owned(), |seen| whole_units(seen.median))
            };
            let _ = writeln!(
                out,
                "| {name} | {} | {} | {} | {} | {} | {turnover} | {per_moved} | {} | {} | {} | {} |",
                if of.renounced() { "yes" } else { "no" },
                of.items,
                of.issuer_held,
                of.float(),
                of.moved,
                of.on_sale,
                middle(of.asking.as_ref()),
                of.sold,
                middle(of.prices.as_ref()),
            );
        }
        out.push('\n');
    }
}

/// What share of a float changed hands, in basis points, or nothing where there is no float.
///
/// A collection nobody holds any of has no turnover to state, which is not the same as a turnover
/// of zero — one is a market that did not move and the other is no market at all.
fn turnover_bps(moved: u64, float: u64) -> Option<u32> {
    u32::try_from(moved.checked_mul(10_000)?.checked_div(float)?).ok()
}

/// Turnover as a reader reads it, or a dash where there is none.
fn as_percent(bps: Option<u32>) -> String {
    bps.map_or_else(
        || "—".to_owned(),
        |bps| format!("{}.{:02}%", bps / 100, bps % 100),
    )
}

/// The class a collection belongs to: everything of its description's address up to the last
/// separator, which is where a platform puts one kind of thing and not another.
fn class_of(content: &str) -> &str {
    let body = content
        .split_once("://")
        .map_or(content, |(_, after)| after);
    body.rsplit_once('/').map_or(body, |(path, _)| path)
}

/// One row of a two-column table.
fn row(out: &mut String, what: &str, value: &str) {
    let _ = writeln!(out, "| {what} | {value} |");
}

/// A quantity in the chain's whole unit, from the nanotons it is counted in. Three decimals, and
/// the rest truncated: the digits shown are digits that were measured.
fn whole_units(nanotons: u128) -> String {
    format!(
        "{}.{:03}",
        nanotons / 1_000_000_000,
        (nanotons % 1_000_000_000) / 1_000_000
    )
}

/// What one page of the item listing answered.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemsPage {
    nft_items: Vec<ItemRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ItemRow {
    address: String,
    /// Whoever holds the item's contract. For a listed item that is a marketplace's escrow, and
    /// the person it is really for is inside `sale`.
    owner: Option<Party>,
    /// Present exactly when the item is listed.
    sale: Option<SaleRow>,
    collection: Option<Party>,
}

/// An account, as the item listing names one: an address and things this census does not read.
#[derive(Debug, Clone, Deserialize)]
struct Party {
    address: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SaleRow {
    owner: Option<Party>,
    price: Option<PriceRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceRow {
    /// `native` where the asking price is in the chain's own asset. A listing priced in some other
    /// token is a price in another unit, and this census does not add units together.
    currency_type: String,
    value: String,
}

/// What one page of the transfer listing answered.
#[derive(Debug, Clone, Deserialize)]
pub struct TransfersPage {
    nft_transfers: Vec<TransferRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct TransferRow {
    nft_address: String,
    nft_collection: Option<String>,
    transaction_now: u64,
}

/// What one page of the purchase listing answered.
#[derive(Debug, Clone, Deserialize)]
pub struct PurchasesPage {
    actions: Vec<ActionRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionRow {
    #[serde(rename = "type")]
    kind: String,
    success: bool,
    start_utime: u64,
    details: PurchaseDetails,
}

#[derive(Debug, Clone, Deserialize)]
struct PurchaseDetails {
    nft_collection: Option<String>,
    nft_item: Option<String>,
    price: Option<String>,
}

/// One asking price as it was read, still carrying the item it is asked for.
///
/// Kept against the item rather than as a bare number, because the item listing is paged by offset
/// and a page can be read twice: prices summarised from a flat list would count a repeat as a
/// second listing, and the distribution the mechanism is calibrated against would be of a market
/// with more listings in it than exist.
#[derive(Debug, Clone, Copy)]
struct Asked {
    item: AccountId,
    price: u128,
}

/// One item as it was read, before duplicates across pages are removed.
#[derive(Debug, Clone, Copy)]
struct Owned {
    item: AccountId,
    /// Whoever the item is really for: the seller rather than the escrow holding it while it is
    /// listed. Absent where the listing named nobody at all.
    holder: Option<AccountId>,
    on_sale: bool,
}

/// What is known about one collection while the pages are still arriving.
#[derive(Debug)]
struct Tally {
    collection: AccountId,
    name: String,
    content: String,
    owner: Option<AccountId>,
    sequence: Option<u64>,
    owned: Vec<Owned>,
    asked: Vec<Asked>,
    transfers: u32,
    moved: Vec<AccountId>,
    sold: Vec<AccountId>,
    paid: Vec<u128>,
}

/// Folds the pages of one sweep into a [`Census`].
///
/// Every page lands in a flat vector and is reconciled once at the end, rather than into sets as
/// it arrives. The sweep reads several hundred thousand rows: appending to a vector is a move
/// where inserting into a tree is a comparison chain and an allocation, and the sort that
/// replaces them runs once over memory that is already contiguous.
#[derive(Debug)]
pub struct CensusBuilder {
    sweep: Sweep,
    issuers: Vec<AccountId>,
    outside_window: u32,
    order: Vec<AccountId>,
    covered: BTreeMap<AccountId, Tally>,
}

impl CensusBuilder {
    /// Opens a census over one window.
    ///
    /// # Errors
    ///
    /// Fails when the window runs backwards, which is a sweep parameterised wrongly and would
    /// otherwise produce a census of nothing that reads exactly like a census of a dead market.
    pub fn new(sweep: Sweep, mut issuers: Vec<AccountId>) -> Result<Self, CensusError> {
        if sweep.window_start > sweep.window_end {
            return Err(CensusError::Backwards {
                from: sweep.window_start,
                to: sweep.window_end,
            });
        }
        issuers.sort_unstable();
        issuers.dedup();
        Ok(CensusBuilder {
            sweep,
            issuers,
            outside_window: 0,
            order: Vec::new(),
            covered: BTreeMap::new(),
        })
    }

    /// Brings one collection into the census, with what the chain says about the collection
    /// itself. Covering the same one twice keeps the first.
    ///
    /// # Errors
    ///
    /// Fails when the collection answered something that is not an index for the item it would
    /// mint next.
    pub fn cover(&mut self, collection: AccountId, listed: Listed) -> Result<(), CensusError> {
        if self.covered.contains_key(&collection) {
            return Ok(());
        }
        let sequence = listed
            .next_index
            .as_deref()
            .map(sequence)
            .transpose()?
            .flatten();
        self.order.push(collection);
        self.covered.insert(
            collection,
            Tally {
                collection,
                name: listed.name,
                content: listed.content,
                owner: listed.owner,
                sequence,
                owned: Vec::new(),
                asked: Vec::new(),
                transfers: 0,
                moved: Vec::new(),
                sold: Vec::new(),
                paid: Vec::new(),
            },
        );
        Ok(())
    }

    /// Records where the chain stood when the sweep finished.
    pub const fn ended_at(&mut self, seqno: u32) {
        self.sweep.seqno_after = seqno;
    }

    /// Folds one page of a collection's items.
    ///
    /// # Errors
    ///
    /// Fails when the page is about a collection the census does not cover, when one of its rows
    /// belongs to a different collection than the page was asked about, or when an address is not
    /// one.
    pub fn count_items(
        &mut self,
        collection: AccountId,
        page: &ItemsPage,
    ) -> Result<(), CensusError> {
        let tally = Self::tally(&mut self.covered, collection)?;
        tally.owned.reserve(page.nft_items.len());
        for row in &page.nft_items {
            belongs_to(
                collection,
                row.collection.as_ref().map(|of| of.address.as_str()),
            )?;
            // Whoever the item is really for. A listed item's contract is held by a marketplace's
            // escrow, and counting that as its holder would turn every marketplace into a whale
            // and take the item from the population that actually holds it.
            let held = row
                .sale
                .as_ref()
                .and_then(|listed| listed.owner.as_ref())
                .or(row.owner.as_ref());
            let item = account("nft item", &row.address)?;
            if let Some(asked) = row.sale.as_ref().and_then(|listed| listed.price.as_ref())
                && asked.currency_type == "native"
            {
                let price = asked.value.parse().map_err(|_| CensusError::Price {
                    found: asked.value.clone(),
                })?;
                tally.asked.push(Asked { item, price });
            }
            tally.owned.push(Owned {
                item,
                holder: held.map(|by| account("owner", &by.address)).transpose()?,
                on_sale: row.sale.is_some(),
            });
        }
        Ok(())
    }

    /// Folds one page of a collection's transfers.
    ///
    /// # Errors
    ///
    /// Fails for the reasons [`count_items`](Self::count_items) does. A transfer outside the
    /// window is not one of them: it is dropped and counted, because a sweep of this length runs
    /// past the window it declares and that is a fact about the sweep rather than about the page.
    pub fn count_transfers(
        &mut self,
        collection: AccountId,
        page: &TransfersPage,
    ) -> Result<(), CensusError> {
        let (from, to) = (self.sweep.window_start, self.sweep.window_end);
        let outside = &mut self.outside_window;
        let tally = Self::tally(&mut self.covered, collection)?;
        tally.moved.reserve(page.nft_transfers.len());
        for row in &page.nft_transfers {
            belongs_to(collection, row.nft_collection.as_deref())?;
            if !within(row.transaction_now, from, to) {
                *outside = outside.saturating_add(1);
                continue;
            }
            tally.transfers = tally.transfers.saturating_add(1);
            tally.moved.push(account("nft item", &row.nft_address)?);
        }
        Ok(())
    }

    /// Folds one page of the chain's purchases.
    ///
    /// The sweep behind this page is not per collection — the indexer will not filter purchases
    /// by one — so it carries every purchase the chain settled inside the window. Rows naming a
    /// collection this census does not cover are passed over, which is the whole reason a global
    /// listing is read at all; rows naming one it does cover are counted exactly once.
    ///
    /// An action the chain recorded as failed is passed over too. It moved nothing and paid
    /// nothing, and counting it would put a price on a sale that did not happen.
    ///
    /// # Errors
    ///
    /// Fails when a row is not a purchase, when a purchase of a covered collection names no item
    /// or no price, or when a price is wider than money is carried in. A purchase outside the
    /// window is dropped and counted rather than refused, for the reason a transfer is.
    pub fn count_purchases(&mut self, page: &PurchasesPage) -> Result<(), CensusError> {
        let (from, to) = (self.sweep.window_start, self.sweep.window_end);
        for row in &page.actions {
            if row.kind != "nft_purchase" {
                return Err(CensusError::Unexpected {
                    kind: row.kind.clone(),
                });
            }
            if !within(row.start_utime, from, to) {
                self.outside_window = self.outside_window.saturating_add(1);
                continue;
            }
            if !row.success {
                continue;
            }
            // An item on this chain need not belong to a collection at all, and one that does not
            // belongs to none this census covers. Passed over for the same reason a purchase of
            // another collection is, and not refused: it is a fact of the chain rather than an
            // answer nobody can read.
            let Some(of) = row.details.nft_collection.as_deref() else {
                continue;
            };
            let Some(tally) = self.covered.get_mut(&account("purchased collection", of)?) else {
                continue;
            };
            let item = row
                .details
                .nft_item
                .as_deref()
                .ok_or(CensusError::Anonymous { missing: "item" })?;
            let paid = row
                .details
                .price
                .as_deref()
                .ok_or(CensusError::Anonymous { missing: "price" })?;
            tally.sold.push(account("purchased item", item)?);
            tally
                .paid
                .push(paid.parse().map_err(|_| CensusError::Price {
                    found: paid.to_owned(),
                })?);
        }
        Ok(())
    }

    /// Reconciles every collection and closes the census.
    ///
    /// # Errors
    ///
    /// Fails when no collection was covered: a census of nothing is not a measurement of a market
    /// with nothing in it, and the two must never render the same.
    pub fn finish(mut self) -> Result<Census, CensusError> {
        if self.order.is_empty() {
            return Err(CensusError::Empty);
        }
        let issuers = self.issuers;
        let collections = self
            .order
            .iter()
            .filter_map(|of| self.covered.remove(of))
            .map(|tally| tally.close(&issuers))
            .collect();
        Ok(Census {
            sweep: self.sweep,
            issuers,
            outside_window: self.outside_window,
            collections,
        })
    }

    /// The tally of one covered collection, or the error saying it was never discovered.
    fn tally(
        covered: &mut BTreeMap<AccountId, Tally>,
        collection: AccountId,
    ) -> Result<&mut Tally, CensusError> {
        covered
            .get_mut(&collection)
            .ok_or_else(|| CensusError::Unnamed {
                collection: collection.to_string(),
            })
    }
}

impl Tally {
    /// Turns everything read about one collection into what is known about it.
    fn close(mut self, issuers: &[AccountId]) -> CollectionCensus {
        // Stable, so that among rows naming the same item the one read first survives: a sweep
        // that raced the chain should report the state it saw first and say how often that
        // happened, rather than mix two moments and report neither.
        self.owned.sort_by_key(|held| held.item);
        let read = self.owned.len();
        self.owned.dedup_by_key(|held| held.item);

        let mut by: Vec<AccountId> = self.owned.iter().filter_map(|held| held.holder).collect();
        by.sort_unstable();
        let mut named: Vec<HolderCount> = Vec::new();
        for holder in &by {
            match named.last_mut() {
                Some(last) if last.holder == *holder => last.items = last.items.saturating_add(1),
                _ => named.push(HolderCount {
                    holder: *holder,
                    items: 1,
                }),
            }
        }
        let distinct = count(named.len());
        let issuer_held = named
            .iter()
            .filter(|held| issuers.contains(&held.holder))
            .fold(0u32, |sum, held| sum.saturating_add(held.items));
        // Longest first, and by address where two hold the same number, so the list a report
        // quotes is the same list on every machine.
        named.sort_by(|left, right| {
            right
                .items
                .cmp(&left.items)
                .then_with(|| left.holder.cmp(&right.holder))
        });
        named.truncate(NAMED_HOLDERS);

        // Asking prices go through the same dedup the items did, and for the same reason.
        self.asked.sort_by_key(|listed| listed.item);
        self.asked.dedup_by_key(|listed| listed.item);
        let mut asked: Vec<u128> = self.asked.iter().map(|listed| listed.price).collect();

        self.moved.sort_unstable();
        self.moved.dedup();
        self.sold.sort_unstable();
        self.sold.dedup();

        CollectionCensus {
            collection: self.collection,
            name: self.name,
            content: self.content,
            owner: self.owner,
            sequence: self.sequence,
            items: count(self.owned.len()),
            restated: count(read - self.owned.len()),
            on_sale: count(self.owned.iter().filter(|held| held.on_sale).count()),
            holders: distinct,
            issuer_held,
            largest_holders: named,
            transfers: self.transfers,
            moved: count(self.moved.len()),
            sold: count(self.sold.len()),
            prices: Prices::of(&mut self.paid),
            asking: Prices::of(&mut asked),
        }
    }
}

/// How many holders a census names before it stops naming them.
///
/// Enough to show whether a collection is one address and a crowd, which is the shape this census
/// exists to detect. Short of the tail, which is thousands of addresses holding one item each and
/// says nothing the count of holders does not already say.
const NAMED_HOLDERS: usize = 10;

/// A count of rows at the width a census carries counts in.
///
/// Saturating rather than fallible: no collection on this chain is within four billion items of
/// the ceiling, and a census that refused to close over an arithmetic impossibility would be
/// harder to trust rather than easier.
fn count(rows: usize) -> u32 {
    u32::try_from(rows).unwrap_or(u32::MAX)
}

/// Reads what a collection answered for the index it would mint next.
///
/// A collection that does not mint in sequence answers a negative number, which is how it says
/// so. Anything that is not a number at all is refused rather than read as the same thing: the
/// two differ, and this field is one of the two that separate an issuer's collection from a copy.
fn sequence(answered: &str) -> Result<Option<u64>, CensusError> {
    let index: i64 = answered.parse().map_err(|_| CensusError::Sequence {
        found: answered.to_owned(),
    })?;
    Ok(u64::try_from(index).ok())
}

/// Reads one field that names an account.
fn account(field: &'static str, text: &str) -> Result<AccountId, CensusError> {
    text.parse().map_err(|_| CensusError::Address {
        field,
        found: text.to_owned(),
    })
}

/// Refuses a row belonging to a collection other than the one its page was asked about.
fn belongs_to(expected: AccountId, found: Option<&str>) -> Result<(), CensusError> {
    let Some(found) = found else {
        return Ok(());
    };
    let of = account("collection", found)?;
    if of == expected {
        return Ok(());
    }
    Err(CensusError::Misfiled {
        expected: expected.to_string(),
        found: of.to_string(),
    })
}

/// Whether a row happened inside the window the census declares.
///
/// A sweep of this size runs for longer than the window's last second, and it walks the recent
/// end of a listing first, so rows on both sides of the window are expected rather than wrong.
/// They are dropped and counted — see [`Census::outside_window`] — because a rate is only a rate
/// over the window it names, and a row silently absorbed into the wrong one is a rate nobody can
/// reproduce.
fn within(at: u64, from: u64, to: u64) -> bool {
    (from..=to).contains(&at)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Two collections of this market, by the addresses the chain knows them at.
    const GIFT: &str = "0:46fa0e9a864014196a5e7d66f1f83ffdb10f2859bbf2ea9baeabbf14d9ce0d50";
    const OTHER: &str = "0:147df416974f7fca15c34e9f28375dcfc7cca03e6d317323468473a238d57ec7";
    /// The address a platform mints its stock to, and holds it at until somebody claims it.
    const MINTER: &str = "0:158136239adb15dd59df90c641f9efd312cfeb8664f218f4c3e5fce9d95e6c07";

    fn account(text: &str) -> AccountId {
        text.parse().unwrap()
    }

    fn sweep() -> Sweep {
        Sweep {
            network: "mainnet".into(),
            source: "toncenter v3 + tonapi v2".into(),
            seqno_before: 87_201_515,
            seqno_after: 87_201_565,
            window_start: 1_000,
            window_end: 2_000,
        }
    }

    /// A collection as an issuer leaves it: nobody owns it, and it mints no sequence.
    fn renounced() -> Listed {
        Listed {
            name: "Plush Pepes".into(),
            content: "https://nft.fragment.com/collection/plushpepe.json".into(),
            owner: None,
            next_index: Some("-1".into()),
        }
    }

    /// A census of one collection, with an issuer named.
    fn opened() -> CensusBuilder {
        let mut census = CensusBuilder::new(sweep(), vec![account(MINTER)]).unwrap();
        census.cover(account(GIFT), renounced()).unwrap();
        census
    }

    fn items(rows: &str) -> ItemsPage {
        serde_json::from_str(&format!("{{\"nft_items\":[{rows}]}}")).unwrap()
    }

    /// One item as the item listing returns it: held outright, by one account.
    fn item(address: &str, owner: &str) -> String {
        format!(
            "{{\"address\":\"{address}\",\"collection\":{{\"address\":\"{GIFT}\"}},\
              \"owner\":{{\"address\":\"{owner}\"}}}}"
        )
    }

    /// One item in a marketplace's escrow: the contract is the marketplace's, the item is the
    /// seller's, and the asking price is quoted in something.
    fn listed(address: &str, escrow: &str, seller: &str, price: &str, currency: &str) -> String {
        format!(
            "{{\"address\":\"{address}\",\"collection\":{{\"address\":\"{GIFT}\"}},\
              \"owner\":{{\"address\":\"{escrow}\"}},\
              \"sale\":{{\"owner\":{{\"address\":\"{seller}\"}},\
              \"price\":{{\"currency_type\":\"{currency}\",\"value\":\"{price}\"}}}}}}"
        )
    }

    /// A distinct item address, derived from a small number so a test can name several.
    fn nth(of: u8) -> String {
        format!("0:{:064x}", u32::from(of) + 0x0100)
    }

    fn transfers(rows: &str) -> TransfersPage {
        serde_json::from_str(&format!("{{\"nft_transfers\":[{rows}]}}")).unwrap()
    }

    fn transfer(item: &str, at: u64) -> String {
        format!(
            "{{\"nft_address\":\"{item}\",\"nft_collection\":\"{GIFT}\",\
              \"transaction_now\":{at}}}"
        )
    }

    fn purchases(rows: &str) -> PurchasesPage {
        serde_json::from_str(&format!("{{\"actions\":[{rows}]}}")).unwrap()
    }

    fn purchase(of: &str, item: &str, price: &str, at: u64, success: bool) -> String {
        format!(
            "{{\"type\":\"nft_purchase\",\"success\":{success},\"start_utime\":{at},\
              \"details\":{{\"nft_collection\":\"{of}\",\"nft_item\":\"{item}\",\
              \"price\":\"{price}\"}}}}"
        )
    }

    fn only(census: CensusBuilder) -> CollectionCensus {
        let mut closed = census.finish().unwrap();
        closed.collections.remove(0)
    }

    /// The address a census carries is the address it was given, and it survives the round trip
    /// through the text a fixture stores it as.
    #[test]
    fn an_account_is_the_account_it_was_read_from() {
        let read = account(GIFT);
        assert_eq!(read.to_string(), GIFT);
        assert_eq!(read.workchain(), 0);
        assert_eq!(serde_json::to_string(&read).unwrap(), format!("\"{GIFT}\""));
        assert_eq!(
            serde_json::from_str::<AccountId>(&format!("\"{GIFT}\"")).unwrap(),
            read
        );
    }

    /// A hash of the wrong length is refused rather than padded, because a padded address is a
    /// different account and every count keyed by it would be keyed by nobody.
    #[test]
    fn a_truncated_address_is_not_an_account() {
        for wrong in ["0:abc", "abc", "0:", &GIFT[..GIFT.len() - 1], "x:0000"] {
            assert!(
                wrong.parse::<AccountId>().is_err(),
                "{wrong} was read as an account"
            );
        }
    }

    /// Paging a live collection by offset can show the same item twice. It is one item, and the
    /// census says how many times that happened rather than letting a repeat inflate the stock.
    #[test]
    fn an_item_read_twice_is_one_item_and_says_so() {
        let mut census = opened();
        let row = item(&nth(1), MINTER);
        census
            .count_items(account(GIFT), &items(&format!("{row},{row}")))
            .unwrap();
        let closed = only(census);
        assert_eq!((closed.items, closed.restated), (1, 1));
    }

    /// An item listed for sale is held by a marketplace's escrow and owned by the person who put
    /// it there. Counting the escrow as its holder would make every marketplace look like a whale
    /// and would take the item away from the population that actually holds it.
    #[test]
    fn a_listed_item_counts_to_its_seller_rather_than_to_the_escrow() {
        let seller = nth(9);
        let escrow = nth(8);
        let mut census = opened();
        census
            .count_items(
                account(GIFT),
                &items(&listed(&nth(1), &escrow, &seller, "1", "native")),
            )
            .unwrap();
        let closed = only(census);
        assert_eq!(closed.on_sale, 1);
        assert_eq!(closed.holders, 1);
        assert_eq!(
            closed.largest_holders,
            vec![HolderCount {
                holder: account(&seller),
                items: 1
            }]
        );
    }

    /// A marketplace will take a price in some other token, and adding those to the ones quoted in
    /// the chain's own asset would be adding two units. The listing still counts; its price does
    /// not, and the count of prices says how many were left out.
    #[test]
    fn a_price_in_another_token_is_not_added_to_prices_in_this_one() {
        let mut census = opened();
        let rows = format!(
            "{},{}",
            listed(&nth(1), &nth(8), &nth(9), "5000000000", "native"),
            listed(&nth(2), &nth(8), &nth(9), "7000000000", "jetton")
        );
        census.count_items(account(GIFT), &items(&rows)).unwrap();
        let closed = only(census);
        let asked = closed.asking.unwrap();
        assert_eq!(closed.on_sale, 2);
        assert_eq!((asked.sales, asked.total), (1, 5_000_000_000));
    }

    /// A page read twice offers the same item at the same price twice. That is one listing, and
    /// a distribution that counted it as two would describe a market with more offers in it than
    /// exist — which is the number a mechanism turning offers into obligations is calibrated on.
    #[test]
    fn an_asking_price_read_twice_is_one_offer() {
        let mut census = opened();
        let row = listed(&nth(1), &nth(8), &nth(9), "5000000000", "native");
        census
            .count_items(account(GIFT), &items(&format!("{row},{row}")))
            .unwrap();
        let closed = only(census);
        assert_eq!((closed.items, closed.restated, closed.on_sale), (1, 1, 1));
        let asked = closed.asking.unwrap();
        assert_eq!((asked.sales, asked.total), (1, 5_000_000_000));
    }

    /// The float is what somebody other than an issuer holds. Stock an issuer has not handed out
    /// has not entered the market, and turnover measured against it would blame the market for a
    /// decision the issuer made.
    #[test]
    fn the_float_excludes_what_the_issuer_still_holds() {
        let held = nth(9);
        let mut census = opened();
        let rows = format!(
            "{},{},{}",
            item(&nth(1), MINTER),
            item(&nth(2), MINTER),
            item(&nth(3), &held)
        );
        census.count_items(account(GIFT), &items(&rows)).unwrap();
        let closed = only(census);
        assert_eq!(
            (closed.items, closed.issuer_held, closed.float()),
            (3, 2, 1)
        );
    }

    /// One sale through an escrow marketplace is two transfers — in and out — and a platform
    /// handing out its own stock is a third that nobody traded. Turnover is therefore counted on
    /// distinct items, and both numbers are reported so the difference is visible.
    #[test]
    fn turnover_counts_distinct_items_rather_than_transfer_messages() {
        let mut census = opened();
        let rows = format!(
            "{},{},{}",
            transfer(&nth(1), 1_500),
            transfer(&nth(1), 1_600),
            transfer(&nth(2), 1_700)
        );
        census
            .count_transfers(account(GIFT), &transfers(&rows))
            .unwrap();
        let closed = only(census);
        assert_eq!((closed.transfers, closed.moved), (3, 2));
    }

    /// The purchase listing cannot be filtered by collection, so it arrives carrying the whole
    /// chain's sales. What the census is not about is passed over; what it is about is counted
    /// once, at what was paid.
    #[test]
    fn a_purchase_of_another_collection_is_passed_over() {
        let mut census = opened();
        let rows = format!(
            "{},{}",
            purchase(GIFT, &nth(1), "5900000000", 1_500, true),
            purchase(OTHER, &nth(2), "33000000000", 1_500, true)
        );
        census.count_purchases(&purchases(&rows)).unwrap();
        let closed = only(census);
        assert_eq!(closed.sold, 1);
        assert_eq!(closed.prices.unwrap().total, 5_900_000_000);
    }

    /// An item on this chain need not belong to a collection, and one that does not belongs to
    /// none this census covers. It is passed over like any other purchase the census is not about,
    /// and the ones it is about are still counted.
    #[test]
    fn a_purchase_of_an_item_with_no_collection_is_passed_over() {
        let mut census = opened();
        let orphan = format!(
            "{{\"type\":\"nft_purchase\",\"success\":true,\"start_utime\":1500,\
              \"details\":{{\"nft_item\":\"{}\",\"price\":\"1\"}}}}",
            nth(7)
        );
        let rows = format!(
            "{orphan},{}",
            purchase(GIFT, &nth(1), "5900000000", 1_500, true)
        );
        census.count_purchases(&purchases(&rows)).unwrap();
        let closed = only(census);
        assert_eq!(closed.sold, 1);
        assert_eq!(closed.prices.unwrap().total, 5_900_000_000);
    }

    /// An action the chain recorded as failed moved nothing and paid nothing. Counting it would
    /// put a price on a sale that never happened, which is exactly the kind of number this census
    /// exists to stop being quoted.
    #[test]
    fn a_failed_purchase_pays_nothing() {
        let mut census = opened();
        census
            .count_purchases(&purchases(&purchase(
                GIFT,
                &nth(1),
                "5900000000",
                1_500,
                false,
            )))
            .unwrap();
        let closed = only(census);
        assert_eq!((closed.sold, closed.prices), (0, None));
    }

    /// A sweep this long runs past the window it declares and walks back beyond its start. Rows on
    /// either side are dropped and counted: a rate is only a rate over the window it names.
    #[test]
    fn a_row_outside_the_window_is_counted_and_not_folded() {
        let mut census = opened();
        let rows = format!("{},{}", transfer(&nth(1), 999), transfer(&nth(2), 2_001));
        census
            .count_transfers(account(GIFT), &transfers(&rows))
            .unwrap();
        census
            .count_purchases(&purchases(&purchase(GIFT, &nth(3), "1", 2_001, true)))
            .unwrap();
        let closed = census.finish().unwrap();
        assert_eq!(closed.outside_window, 3);
        assert_eq!(
            (closed.collections[0].transfers, closed.collections[0].moved),
            (0, 0)
        );
        assert_eq!(closed.collections[0].sold, 0);
    }

    /// A page asked about one collection and answering about another is an indexer disagreeing
    /// with itself, and every number folded from it would belong to the wrong market.
    #[test]
    fn a_page_answering_about_another_collection_is_refused() {
        let mut census = opened();
        census.cover(account(OTHER), renounced()).unwrap();
        let refused = census.count_items(account(OTHER), &items(&item(&nth(1), MINTER)));
        assert!(
            matches!(refused, Err(CensusError::Misfiled { .. })),
            "{refused:?}"
        );
    }

    /// A page about a collection nobody discovered would be folded into a market the census does
    /// not describe.
    #[test]
    fn a_page_about_an_undiscovered_collection_is_refused() {
        let mut census = opened();
        let refused = census.count_transfers(account(OTHER), &transfers(&transfer(&nth(1), 1_500)));
        assert!(
            matches!(refused, Err(CensusError::Unnamed { .. })),
            "{refused:?}"
        );
    }

    /// An action of some other kind means the sweep asked for something else, and a census folded
    /// from it would be measuring a different verb.
    #[test]
    fn an_action_that_is_not_a_purchase_is_refused() {
        let mut census = opened();
        let row =
            "{\"type\":\"nft_transfer\",\"success\":true,\"start_utime\":1500,\"details\":{}}";
        let refused = census.count_purchases(&purchases(row));
        assert!(
            matches!(refused, Err(CensusError::Unexpected { .. })),
            "{refused:?}"
        );
    }

    /// Percentiles by nearest rank over the integers themselves: the median of four prices is the
    /// second, exactly, on every machine — no interpolation, no float, nothing to disagree about.
    #[test]
    fn percentiles_land_on_observations() {
        let mut census = opened();
        let rows: Vec<String> = ["4000000000", "1000000000", "3000000000", "2000000000"]
            .iter()
            .enumerate()
            .map(|(at, price)| {
                purchase(
                    GIFT,
                    &nth(u8::try_from(at).unwrap() + 1),
                    price,
                    1_500,
                    true,
                )
            })
            .collect();
        census.count_purchases(&purchases(&rows.join(","))).unwrap();
        let paid = only(census).prices.unwrap();
        assert_eq!(paid.sales, 4);
        assert_eq!(paid.total, 10_000_000_000);
        assert_eq!((paid.min, paid.max), (1_000_000_000, 4_000_000_000));
        assert_eq!(
            (paid.p25, paid.median, paid.p75),
            (1_000_000_000, 2_000_000_000, 3_000_000_000)
        );
    }

    /// Serving the same description as an issuer's collection is not being it. What an issuer
    /// walks away from has no owner and no sequence; a copy of it has both, and a census that
    /// could not tell them apart would count a market and its imitations as one.
    #[test]
    fn a_copy_of_a_collection_is_not_the_collection() {
        let copy = Listed {
            name: "Plush Pepes".into(),
            content: "https://nft.fragment.com/collection/plushpepe.json".into(),
            owner: Some(account(OTHER)),
            next_index: Some("1164".into()),
        };
        let mut census = CensusBuilder::new(sweep(), Vec::new()).unwrap();
        census.cover(account(GIFT), renounced()).unwrap();
        census.cover(account(OTHER), copy).unwrap();
        let closed = census.finish().unwrap();
        assert!(closed.collections[0].renounced());
        assert!(!closed.collections[1].renounced());
    }

    /// A window that runs backwards would measure nothing while looking exactly like a market
    /// where nothing happens, so it is refused where it is declared rather than where it shows.
    #[test]
    fn a_backwards_window_is_refused() {
        let mut backwards = sweep();
        backwards.window_start = backwards.window_end + 1;
        assert!(matches!(
            CensusBuilder::new(backwards, Vec::new()),
            Err(CensusError::Backwards { .. })
        ));
    }

    /// A collection that does not mint in sequence says so with a negative index. One that
    /// answers something else has said nothing, and reading that as the first would make a copy
    /// indistinguishable from what it copies.
    #[test]
    fn an_unreadable_next_index_is_refused_rather_than_read_as_no_sequence() {
        let mut census = CensusBuilder::new(sweep(), Vec::new()).unwrap();
        let mut unreadable = renounced();
        unreadable.next_index = Some("later".into());
        let refused = census.cover(account(GIFT), unreadable);
        assert!(
            matches!(refused, Err(CensusError::Sequence { .. })),
            "{refused:?}"
        );
    }

    /// A census that discovered nothing is not a measurement of a market with nothing in it.
    #[test]
    fn a_census_of_nothing_does_not_close() {
        let empty = CensusBuilder::new(sweep(), Vec::new()).unwrap();
        assert_eq!(empty.finish().unwrap_err(), CensusError::Empty);
    }

    /// A census round-trips through the fixture it is stored as, addresses and prices included.
    #[test]
    fn a_census_round_trips() {
        let mut census = opened();
        census
            .count_items(
                account(GIFT),
                &items(&listed(&nth(1), &nth(8), MINTER, "77", "native")),
            )
            .unwrap();
        census
            .count_purchases(&purchases(&purchase(
                GIFT,
                &nth(1),
                "5900000000",
                1_500,
                true,
            )))
            .unwrap();
        let closed = census.finish().unwrap();
        let stored = serde_json::to_string(&closed).unwrap();
        assert_eq!(serde_json::from_str::<Census>(&stored).unwrap(), closed);
    }
}
