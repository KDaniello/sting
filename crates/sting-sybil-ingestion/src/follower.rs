//! Following the tip, and replaying a finished range.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;

use crate::error::{IngestError, Result};
use crate::{BlockBatch, BlockSource, Store};

/// How many blocks may be in flight at once. Higher buys nothing once the endpoint is the
/// bottleneck, and costs memory proportional to the largest block.
const FETCH_WINDOW: usize = 16;

/// Roughly half a block time: long enough not to hammer the endpoint at the tip, short enough
/// that the follower is never a block behind for a whole block.
const TIP_POLL: Duration = Duration::from_millis(200);

/// Follows the chain tip, sending each block on in sequence order.
///
/// Fetches run concurrently but are handed on in order, so a checkpoint written from this stream
/// is monotonic: `buffered` is the whole reason, and `buffer_unordered` in its place would let a
/// later block commit before an earlier one and turn the checkpoint into a claim that a gap was
/// filled.
///
/// `end_seqno` is `None` to follow indefinitely, or `Some` to stop at the sequence number it
/// names.
///
/// # Errors
/// [`IngestError`] when the source refuses, or when nobody is left reading the channel.
pub async fn run_follower<S: BlockSource + 'static>(
    source: Arc<S>,
    tx: mpsc::Sender<BlockBatch>,
    start_seqno: u32,
    end_seqno: Option<u32>,
) -> Result<()> {
    let mut next = start_seqno;
    loop {
        if let Some(end) = end_seqno
            && next > end
        {
            return Ok(());
        }
        let tip = source.masterchain_info().await?.last_seqno;
        let effective_tip = end_seqno.map_or(tip, |end| end.min(tip));
        if next > effective_tip {
            tokio::time::sleep(TIP_POLL).await;
            continue;
        }
        let mut blocks = stream::iter(next..=effective_tip)
            .map(|seqno| {
                let source = Arc::clone(&source);
                async move { source.fetch_masterchain_block(seqno).await }
            })
            .buffered(FETCH_WINDOW);
        while let Some(fetched) = blocks.next().await {
            let batch = fetched?;
            let next_seqno = batch.seqno.saturating_add(1);
            // Backpressure lives here: a full channel blocks the producer rather than growing.
            tx.send(batch).await?;
            next = next_seqno;
        }
    }
}

/// Replays `[from, to]` in order, writing each block as it lands.
///
/// Deliberately sequential where the follower is concurrent: a replay is a finite range whose
/// worth is that it reproduces, and order is cheaper to guarantee than to check afterwards.
///
/// # Errors
/// [`IngestError::BackwardsRange`] when the range runs backwards, and whatever the source or the
/// store refuses with.
pub async fn ingest_range<S: BlockSource, St: Store>(
    source: &S,
    store: &St,
    from: u32,
    to: u32,
) -> Result<()> {
    if from > to {
        return Err(IngestError::BackwardsRange { from, to });
    }
    for seqno in from..=to {
        let batch = source.fetch_masterchain_block(seqno).await?;
        store.write_batch(batch).await?;
    }
    Ok(())
}
