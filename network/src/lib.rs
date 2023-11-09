mod behaviour;
pub mod reactor;
mod types;

pub use behaviour::*;
pub use reactor::*;
pub use types::*;
use tracing::info;
use serde::Deserialize;
use once_cell::sync::Lazy;
use tokio::sync::{mpsc, Mutex};
use std::{
    time::Duration, 
    collections::{hash_map::DefaultHasher, HashMap}, 
    hash::{Hasher, Hash}, sync::Arc,
};
use anyhow::Result;
use libp2p::{
    gossipsub::{IdentTopic as Topic, GossipsubConfigBuilder, ValidationMode, MessageId, GossipsubMessage}, 
    swarm::SwarmBuilder, 
    identity::Keypair, 
    tcp::{GenTcpConfig, tokio::Tcp},
    core::upgrade, 
    Swarm, PeerId, noise, yamux, Transport,
};
use libp2p::identity::ed25519::Keypair as ED25519KeyPair;
use ed25519_dalek::Keypair as ed25519_key_pair;
use tendermint_config::NodeKey;
use std::env;
use std::path::PathBuf;
pub type TokioTcpConfig = GenTcpConfig<Tcp>;

fn fix_keypair_path() -> NodeKey{
    let mut home_dir = env::home_dir().expect("Failed to get home directory");
    // TODO: 这里需要进行更换，通过home参数改变读取目录的路径！！！！
    home_dir.push(".gaia-rs/config/node_key.json");
    let node_key: NodeKey = NodeKey::load_json_file(&home_dir).unwrap();
    node_key
}

// TODO: repalce the Keypair with tendermint node key
// key pair
/* pub static ID_KEYS: Lazy<Keypair> = Lazy::new(|| {
    let node_key = fix_keypair_path();
    let private_key = *node_key.priv_key.ed25519_keypair().unwrap();
    Keypair::Ed25519(ED25519KeyPair(private_key))
});

// NOTE: 这里Node id的处理和tendermint p2p中的获取方式有点不同(后续如果要换成tendermint p2p还需要再修改！)
// node id
pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| {
    let node_key = fix_keypair_path();
    PeerId::from(node_key.public_key())
}); */

pub static ID_KEYS: Lazy<Keypair> = Lazy::new(Keypair::generate_ed25519);
pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(ID_KEYS.public()));

pub static BLOCK_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("blocks"));
pub static TRANX_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("tranxs"));

static WALLET_MAP: Lazy<Arc<Mutex<HashMap<String, String>>>> = Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

async fn create_swarm(
    topics: Vec<Topic>,
    msg_sender: mpsc::UnboundedSender<Messages>,
) -> Result<Swarm<BlockchainBehaviour>> {
    info!("Your local peer id: {:?}", *PEER_ID);

    let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&ID_KEYS)?;

    let transport = TokioTcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(yamux::YamuxConfig::default())
        .boxed();

    let message_id_fn = |message: &GossipsubMessage| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        MessageId::from(s.finish().to_string())
    };

    let gossipsub_config = GossipsubConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()
        .expect("Valid config");

    let mut behaviour = BlockchainBehaviour::new(ID_KEYS.clone(), gossipsub_config, msg_sender).await?;
    for topic in topics.iter() {
        behaviour.gossipsub.subscribe(topic).unwrap();
    }

    let swarm = SwarmBuilder::new(transport, behaviour, PEER_ID.clone())
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        })).build();
    
    info!("Exiting create_swarm...");
    Ok(swarm)
}