//! Turns what a chain said about one episode into a recording this workspace can replay.
//!
//! Reads on standard input the answers a node gave — the logs of one market over a window, the
//! block times, the state at each end and the prices quoted in between — and prints the episode.
//! The reaching is the shell's; the reading is here, where a mistake would be silent.

use std::{collections::BTreeMap, io::Read};

use sting_karman_data::{Origin, PulledMarket};
use sting_karman_mech_lending::Charged;
use karman_reference::{Episode, EpisodeOrigin, Quoted};
use sting_karman_types::{AgentId, Amount, Tick};
use serde::Deserialize;

/// What a node was asked about one episode, and what it said.
#[derive(Deserialize)]
struct Answers {
    chain_id: u64,
    contract: String,
    market: String,
    from_block: u64,
    to_block: u64,
    block_times: BTreeMap<String, u64>,
    logs: Vec<RawLog>,
    opening: Held,
    closing: Held,
    market_params: Params,
    prices: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawLog {
    address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "blockNumber")]
    block_number: String,
}

#[derive(Deserialize)]
struct Held {
    market: String,
    holders: Vec<Answered>,
}

#[derive(Deserialize)]
struct Answered {
    holder: String,
    answer: String,
}

#[derive(Deserialize)]
struct Params {
    lltv: u128,
}

fn main() -> Result<(), String> {
    let mut said = String::new();
    std::io::stdin()
        .read_to_string(&mut said)
        .map_err(|e| format!("nothing readable arrived: {e}"))?;
    let answers: Answers =
        serde_json::from_str(&said).map_err(|e| format!("the answers are not readable: {e}"))?;

    // Everyone the window names, in the order the answers list them. That order is what an agent
    // identifier means here, and it is why an actor holding nothing at the opening still has one.
    let actors: Vec<String> =
        answers.opening.holders.iter().map(|held| held.holder.to_lowercase()).collect();

    let mut opening = PulledMarket::decode(
        Origin {
            chain_id: answers.chain_id,
            protocol: "morpho-blue".into(),
            contract: answers.contract.clone(),
            market: answers.market.clone(),
            block: answers.from_block,
        },
        None,
        &answers.opening.market,
    )
    .map_err(|e| format!("the opening market is not readable: {e}"))?;
    for held in &answers.opening.holders {
        opening = opening
            .with_holder(&held.holder.to_lowercase(), &held.answer)
            .map_err(|e| format!("an opening position is not readable: {e}"))?;
    }

    // Tick zero is where the market's own clock stood, not where the window opened. A market
    // accrues from its last update, and an episode that started counting at the first block of a
    // window would charge every borrower for less time than they were actually charged for.
    let opened_at = opening.last_update;
    let origin = EpisodeOrigin {
        chain_id: answers.chain_id,
        contract: answers.contract.clone(),
        market: answers.market.clone(),
        from_block: answers.from_block,
        to_block: answers.to_block,
        opening_timestamp: opened_at,
    };
    let at = |block: &str| -> Result<Tick, String> {
        let seconds = answers
            .block_times
            .get(block)
            .ok_or_else(|| format!("no time recorded for block {block}"))?;
        seconds
            .checked_sub(opened_at)
            .map(Tick)
            .ok_or_else(|| format!("block {block} is older than the market's own clock"))
    };

    let closing = karman_reference::closing(&answers.closing.market)?;

    let mut prices: Vec<Quoted> = Vec::new();
    for (block, price) in &answers.prices {
        prices.push(Quoted {
            from: at(block)?,
            price: price.parse().map_err(|_| format!("price at {block} is not a number"))?,
        });
    }
    prices.sort_by_key(|quoted| quoted.from);
    prices.dedup_by_key(|quoted| quoted.from);

    let mut rates: Vec<Charged> = Vec::new();
    let mut steps = Vec::new();
    let mut accrued: Vec<sting_karman_mech_lending::LendingEvent> = Vec::new();
    for log in &answers.logs {
        let tick = at(&u64::from_str_radix(log.block_number.trim_start_matches("0x"), 16)
            .map_err(|_| String::from("a log has no block"))?
            .to_string())?;
        if let Some(rate) = karman_reference::charged_at(&log.topics, &log.data)? {
            rates.push(Charged { from: tick, per_second: Amount::new(rate) });
        }
        let reported = karman_reference::decode(&log.address, &log.topics, &log.data, &|who| {
            actors
                .iter()
                .position(|known| known == &who)
                .map(|at| AgentId(u32::try_from(at).unwrap_or(u32::MAX)))
                .ok_or_else(|| format!("the chain named an actor the window did not: {who}"))
        })?;
        for event in reported {
            match karman_reference::acted_on(&event) {
                None => accrued.push(event),
                Some((actor, action)) => {
                    let mut events = std::mem::take(&mut accrued);
                    events.push(event);
                    steps.push(sting_karman_verify::replay::RecordedAction {
                        at: tick,
                        actor,
                        action,
                        events,
                    });
                }
            }
        }
    }
    rates.sort_by_key(|charged| charged.from);
    rates.dedup_by_key(|charged| charged.from);

    let episode = Episode {
        origin,
        lltv: Amount::new(answers.market_params.lltv),
        opening,
        prices,
        rates,
        steps,
        closing,
        actors,
    };
    let printed =
        serde_json::to_string_pretty(&episode).map_err(|e| format!("unprintable episode: {e}"))?;
    println!("{printed}");
    Ok(())
}
