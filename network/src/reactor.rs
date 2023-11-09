use super::{create_swarm, BLOCK_TOPIC, PEER_ID, TRANX_TOPIC, WALLET_MAP};
use super::{Commands, Messages};
use libp2p::{gossipsub::MessageId, swarm::SwarmEvent, PeerId, Swarm};
use std::sync::Arc;
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    spawn,
    sync::{mpsc, Mutex},
    time::{interval, sleep, Duration},
};
use store::StoreKey;
use super::BlockchainBehaviour;
use serde::Deserialize;

#[derive(Clone)]
pub struct Reactor {
    pub msg_receiver: Arc<Mutex<mpsc::UnboundedReceiver<Messages>>>,
    pub swarm: Arc<Mutex<Swarm<BlockchainBehaviour>>>,
}

impl Reactor {
    pub async fn new() -> Self {
        let (msg_sender, msg_receiver) = mpsc::unbounded_channel();
        Self {
            msg_receiver: Arc::new(Mutex::new(msg_receiver)),
            swarm: Arc::new(Mutex::new(
                create_swarm(vec![BLOCK_TOPIC.clone(), TRANX_TOPIC.clone()], msg_sender)
                    .await
                    .unwrap(),
            )),
        }
    }
}
