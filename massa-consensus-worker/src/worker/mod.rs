use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph, ConsensusChannels, ConsensusConfig,
    ConsensusController, ConsensusManager,
};
use massa_models::block::{Block, BlockHeader, BlockId, FilledBlock};
use massa_models::clique::Clique;
use massa_models::config::CHANNEL_SIZE;
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;
use tokio::sync::broadcast::{self, Sender};

use crate::commands::ConsensusCommand;
use crate::controller::ConsensusControllerImpl;
use crate::manager::ConsensusManagerImpl;
use crate::state::ConsensusState;

/// The consensus worker structure that contains all information and tools for the consensus worker thread.
pub struct ConsensusWorker {
    /// Channel to receive command from the controller
    command_receiver: mpsc::Receiver<ConsensusCommand>,
    /// Configuration of the consensus
    config: ConsensusConfig,
    /// State shared with the controller
    shared_state: Arc<RwLock<ConsensusState>>,
    /// Previous slot.
    previous_slot: Option<Slot>,
    /// Next slot
    next_slot: Slot,
    /// Next slot instant
    next_instant: Instant,
}

#[derive(Clone)]
pub struct WsConfig {
    /// Whether WebSockets are enabled
    pub enabled: bool,
    /// Broadcast sender(channel) for blocks headers
    pub block_header_sender: Sender<BlockHeader>,
    /// Broadcast sender(channel) for blocks
    pub block_sender: Sender<Block>,
    /// Broadcast sender(channel) for filled blocks
    pub filled_block_sender: Sender<FilledBlock>,
}

mod init;
mod main_loop;

/// Create a new consensus worker thread.
///
/// # Arguments:
/// * `config`: Configuration of the consensus
/// * `channels`: Channels to communicate with others modules
/// * `init_graph`: Optional initial graph to bootstrap the graph. if None, the graph will have only genesis blocks.
/// * `storage`: Storage to use for the consensus
///
/// # Returns:
/// * The consensus controller to communicate with the consensus worker thread
/// * The consensus manager to manage the consensus worker thread
pub fn start_consensus_worker(
    config: ConsensusConfig,
    channels: ConsensusChannels,
    init_graph: Option<BootstrapableGraph>,
    storage: Storage,
) -> (Box<dyn ConsensusController>, Box<dyn ConsensusManager>) {
    let ws_config = WsConfig {
        enabled: config.ws_enabled,
        block_header_sender: broadcast::channel(config.ws_blocks_headers_capacity).0,
        block_sender: broadcast::channel(config.ws_blocks_capacity).0,
        filled_block_sender: broadcast::channel(config.ws_filled_blocks_capacity).0,
    };

    let (tx, rx) = mpsc::sync_channel(CHANNEL_SIZE);
    // desync detection timespan
    let bootstrap_part_size = config.bootstrap_part_size;
    let stats_desync_detection_timespan =
        config.t0.checked_mul(config.periods_per_cycle * 2).unwrap();
    let shared_state = Arc::new(RwLock::new(ConsensusState {
        storage: storage.clone(),
        config: config.clone(),
        channels,
        max_cliques: vec![Clique {
            block_ids: PreHashSet::<BlockId>::default(),
            fitness: 0,
            is_blockclique: true,
        }],
        sequence_counter: 0,
        waiting_for_slot_index: Default::default(),
        waiting_for_dependencies_index: Default::default(),
        discarded_index: Default::default(),
        to_propagate: Default::default(),
        attack_attempts: Default::default(),
        new_final_blocks: Default::default(),
        new_stale_blocks: Default::default(),
        incoming_index: Default::default(),
        active_index: Default::default(),
        save_final_periods: Default::default(),
        latest_final_blocks_periods: Default::default(),
        best_parents: Default::default(),
        block_statuses: Default::default(),
        genesis_hashes: Default::default(),
        gi_head: Default::default(),
        final_block_stats: Default::default(),
        stale_block_stats: Default::default(),
        protocol_blocks: Default::default(),
        wishlist: Default::default(),
        launch_time: MassaTime::now(config.clock_compensation_millis).unwrap(),
        stats_desync_detection_timespan,
        stats_history_timespan: std::cmp::max(
            stats_desync_detection_timespan,
            config.stats_timespan,
        ),
        prev_blockclique: Default::default(),
    }));

    let shared_state_cloned = shared_state.clone();
    let consensus_thread = thread::Builder::new()
        .name("consensus worker".into())
        .spawn(move || {
            let mut consensus_worker =
                ConsensusWorker::new(config, rx, shared_state_cloned, init_graph, storage).unwrap();
            consensus_worker.run()
        })
        .expect("Can't spawn consensus thread.");

    let manager = ConsensusManagerImpl {
        consensus_thread: Some((tx.clone(), consensus_thread)),
    };

    let controller = ConsensusControllerImpl::new(tx, ws_config, shared_state, bootstrap_part_size);

    (Box::new(controller), Box::new(manager))
}
