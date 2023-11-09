use bytes::Bytes;
use database::{Database, RocksDB};
use proto_messages::cosmos::tx::v1beta1::{Message, TxWithRaw};
use serde::de::DeserializeOwned;
use std::{
    marker::PhantomData,
    sync::{Arc, RwLock}, ops::DerefMut, collections::HashMap,
};
use store_crate::{MultiStore, StoreKey, hash::{StoreInfo, hash_store_infos}};
use tendermint_abci::Application;
use tendermint_informal::block::Header;
use tendermint_proto::abci::{
    RequestApplySnapshotChunk, RequestBeginBlock, RequestCheckTx, RequestDeliverTx, RequestEcho,
    RequestEndBlock, RequestInfo, RequestInitChain, RequestLoadSnapshotChunk, RequestOfferSnapshot,
    RequestQuery, ResponseApplySnapshotChunk, ResponseBeginBlock, ResponseCheckTx, ResponseCommit,
    ResponseDeliverTx, ResponseEcho, ResponseEndBlock, ResponseFlush, ResponseInfo,
    ResponseInitChain, ResponseListSnapshots, ResponseLoadSnapshotChunk, ResponseOfferSnapshot,
    ResponseQuery,
};
use tracing::{error, info};
// p2p reactor
use network::reactor::Reactor;
use network::{Commands, Messages, BLOCK_TOPIC, PEER_ID};
use trees::iavl::{Node, Tree};
use libp2p::{gossipsub::MessageId, swarm::SwarmEvent, PeerId, Swarm};
use libp2p::futures::StreamExt;
use tokio::time::{sleep, Duration};
use super::BaseAppError;

use tendermint_abci::ServerBuilder;
use crate::client::rest::run_rest_server;
use axum::Router;
use axum::body::Body;

use crate::{
    error::AppError,
    types::context::{Context, InitContext, QueryContext, TxContext},
    x::params::{Keeper, ParamsSubspaceKey},
};

use super::{
    ante::{AnteHandler, AuthKeeper, BankKeeper},
    params::BaseAppParamsKeeper,
};

pub trait Handler<M: Message, SK: StoreKey, G>: Clone + Send + Sync {
    fn handle_tx<DB: Database>(&self, ctx: &mut Context<DB, SK>, msg: &M) -> Result<(), AppError>;

    fn handle_init_genesis<DB: Database>(&self, ctx: &mut Context<DB, SK>, genesis: G);

    fn handle_query<DB: Database>(
        &self,
        ctx: &QueryContext<DB, SK>,
        query: RequestQuery,
    ) -> Result<Bytes, AppError>;
}

#[derive(Clone)]
pub struct BaseApp<
    SK: StoreKey,
    PSK: ParamsSubspaceKey,
    M: Message,
    BK: BankKeeper<SK> + Clone + Send + Sync + 'static,
    AK: AuthKeeper<SK> + Clone + Send + Sync + 'static,
    H: Handler<M, SK, G>,
    G: DeserializeOwned + Clone + Send + Sync + 'static,
> {
    multi_store: Arc<RwLock<MultiStore<RocksDB, SK>>>,
    height: Arc<RwLock<u64>>,
    p2p_reactor: Arc<RwLock<Reactor>>,
    base_ante_handler: AnteHandler<BK, AK, SK>,
    handler: H,
    block_header: Arc<RwLock<Option<Header>>>, // passed by Tendermint in call to begin_block
    baseapp_params_keeper: BaseAppParamsKeeper<SK, PSK>,
    app_name: &'static str,
    app_version: &'static str,
    pub m: PhantomData<M>,
    pub g: PhantomData<G>,
}

impl<
        M: Message + 'static,
        SK: StoreKey + Clone + Send + Sync + 'static,
        PSK: ParamsSubspaceKey + Clone + Send + Sync + 'static,
        BK: BankKeeper<SK> + Clone + Send + Sync + 'static,
        AK: AuthKeeper<SK> + Clone + Send + Sync + 'static,
        H: Handler<M, SK, G> + 'static,
        G: DeserializeOwned + Clone + Send + Sync + 'static,
    > Application for BaseApp<SK, PSK, M, BK, AK, H, G>
{
    fn init_chain(&self, request: RequestInitChain) -> ResponseInitChain {
        info!("Got init chain request");
        let mut multi_store = self
            .multi_store
            .write()
            .expect("RwLock will not be poisoned");

        //TODO: handle request height > 1 as is done in SDK

        let mut ctx = InitContext::new(&mut multi_store, self.get_block_height(), request.chain_id);

        if let Some(params) = request.consensus_params.clone() {
            self.baseapp_params_keeper
                .set_consensus_params(&mut ctx.as_any(), params);
        }

        let genesis: G = String::from_utf8(request.app_state_bytes.into())
            .map_err(|e| AppError::Genesis(e.to_string()))
            .and_then(|s| serde_json::from_str(&s).map_err(|e| AppError::Genesis(e.to_string())))
            .unwrap_or_else(|e| {
                error!(
                    "Invalid genesis provided by Tendermint.\n{}\nTerminating process",
                    e.to_string()
                );
                std::process::exit(1)
            });

        self.handler.handle_init_genesis(&mut ctx.as_any(), genesis);

        multi_store.write_then_clear_tx_caches();

        ResponseInitChain {
            consensus_params: request.consensus_params,
            validators: request.validators,
            app_hash: "hash_goes_here".into(),
        }
    }

    fn info(&self, request: RequestInfo) -> ResponseInfo {
        info!(
            "Got info request. Tendermint version: {}; Block version: {}; P2P version: {}",
            request.version, request.block_version, request.p2p_version
        );

        ResponseInfo {
            data: self.app_name.to_string(),
            version: self.app_version.to_string(),
            app_version: 1,
            last_block_height: self
                .get_block_height()
                .try_into()
                .expect("can't believe we made it this far"),
            last_block_app_hash: self.get_last_commit_hash().to_vec().into(),
        }
    }

    fn query(&self, request: RequestQuery) -> ResponseQuery {
        info!("Got query request to: {}", request.path);

        let multi_store = self
            .multi_store
            .read()
            .expect("RwLock will not be poisoned");

        let ctx = QueryContext::new(&multi_store, self.get_block_height());
        let res = self.handler.handle_query(&ctx, request.clone());

        match res {
            Ok(res) => ResponseQuery {
                code: 0,
                log: "exists".to_string(),
                info: "".to_string(),
                index: 0,
                key: request.data,
                value: res.into(),
                proof_ops: None,
                height: self
                    .get_block_height()
                    .try_into()
                    .expect("can't believe we made it this far"),
                codespace: "".to_string(),
            },
            Err(e) => ResponseQuery {
                code: 1,
                log: e.to_string(),
                info: "".to_string(),
                index: 0,
                key: request.data,
                value: Default::default(),
                proof_ops: None,
                height: 0,
                codespace: "".to_string(),
            },
        }
    }

    fn check_tx(&self, _request: RequestCheckTx) -> ResponseCheckTx {
        info!("Got check tx request");
        ResponseCheckTx {
            code: 0,
            data: Default::default(),
            log: "".to_string(),
            info: "".to_string(),
            gas_wanted: 1,
            gas_used: 0,
            events: vec![],
            codespace: "".to_string(),
            mempool_error: "".to_string(),
            priority: 0,
            sender: "".to_string(),
        }
    }

    fn deliver_tx(&self, request: RequestDeliverTx) -> ResponseDeliverTx {
        info!("Got deliver tx request");
        match self.run_tx(request.tx) {
            Ok(events) => ResponseDeliverTx {
                code: 0,
                data: Default::default(),
                log: "".to_string(),
                info: "".to_string(),
                gas_wanted: 0,
                gas_used: 0,
                events: events.into_iter().map(|e| e.into()).collect(),
                codespace: "".to_string(),
            },
            Err(e) => {
                info!("Failed to process tx: {}", e);
                ResponseDeliverTx {
                    code: e.code(),
                    data: Bytes::new(),
                    log: e.to_string(),
                    info: "".to_string(),
                    gas_wanted: 0,
                    gas_used: 0,
                    events: vec![],
                    codespace: "".to_string(),
                }
            }
        }
    }

    fn commit(&self) -> ResponseCommit {
        info!("Got commit request");
        let new_height = self.increment_block_height();
        let mut multi_store = self
            .multi_store
            .write()
            .expect("RwLock will not be poisoned");

        // NOTE: 区块提交到本地存储
        let hash = multi_store.commit();
        info!(
            "Committed state, block height: {} app hash: {}",
            new_height,
            hex::encode(hash)
        );
        println!("本地提交的multiStore hash为:{:?}", hash);

        ResponseCommit {
            data: hash.to_vec().into(),
            retain_height: (new_height - 1)
                .try_into()
                .expect("can't believe we made it this far"),
        }
    }

    fn echo(&self, request: RequestEcho) -> ResponseEcho {
        info!("Got echo request");
        ResponseEcho {
            message: request.message,
        }
    }

    fn begin_block(&self, request: RequestBeginBlock) -> ResponseBeginBlock {
        info!("Got begin block request");

        self.set_block_header(
            request
                .header
                .expect("tendermint will never send nothing to the app")
                .try_into()
                .expect("tendermint will send a valid Header struct"),
        );

        Default::default()
    }

    fn end_block(&self, _request: RequestEndBlock) -> ResponseEndBlock {
        info!("Got end block request");
        Default::default()
    }

    /// Signals that messages queued on the client should be flushed to the server.
    fn flush(&self) -> ResponseFlush {
        info!("Got flush request");
        ResponseFlush {}
    }

    /// Used during state sync to discover available snapshots on peers.
    fn list_snapshots(&self) -> ResponseListSnapshots {
        info!("Got list snapshots request");
        Default::default()
    }

    /// Called when bootstrapping the node using state sync.
    fn offer_snapshot(&self, _request: RequestOfferSnapshot) -> ResponseOfferSnapshot {
        info!("Got offer snapshot request");
        Default::default()
    }

    /// Used during state sync to retrieve chunks of snapshots from peers.
    fn load_snapshot_chunk(&self, _request: RequestLoadSnapshotChunk) -> ResponseLoadSnapshotChunk {
        info!("Got load snapshot chunk request");
        Default::default()
    }

    /// Apply the given snapshot chunk to the application's state.
    fn apply_snapshot_chunk(
        &self,
        _request: RequestApplySnapshotChunk,
    ) -> ResponseApplySnapshotChunk {
        info!("Got apply snapshot chunk request");
        Default::default()
    }
}

impl<
        M: Message,
        SK: StoreKey,
        PSK: ParamsSubspaceKey + Clone + Send + Sync + 'static,
        BK: BankKeeper<SK> + Clone + Send + Sync + 'static,
        AK: AuthKeeper<SK> + Clone + Send + Sync + 'static,
        H: Handler<M, SK, G>,
        G: DeserializeOwned + Clone + Send + Sync + 'static,
    > BaseApp<SK, PSK, M, BK, AK, H, G>
{
    pub fn new(
        db: RocksDB,
        app_name: &'static str,
        version: &'static str,
        p2p_reactor: Reactor,
        bank_keeper: BK,
        auth_keeper: AK,
        params_keeper: Keeper<SK, PSK>,
        params_subspace_key: PSK,
        handler: H,
    ) -> Self {
        let multi_store = MultiStore::new(db);
        let baseapp_params_keeper = BaseAppParamsKeeper {
            params_keeper,
            params_subspace_key,
        };
        let height = multi_store.get_head_version().into();

        Self {
            multi_store: Arc::new(RwLock::new(multi_store)),
            base_ante_handler: AnteHandler::new(bank_keeper, auth_keeper),
            p2p_reactor: Arc::new(RwLock::new(p2p_reactor)),
            handler,
            block_header: Arc::new(RwLock::new(None)),
            baseapp_params_keeper,
            height: Arc::new(RwLock::new(height)),
            app_name,
            app_version: version,
            m: PhantomData,
            g: PhantomData,
        }
    }

    pub fn get_block_height(&self) -> u64 {
        *self.height.read().expect("RwLock will not be poisoned")
    }

    fn get_block_header(&self) -> Option<Header> {
        self.block_header
            .read()
            .expect("RwLock will not be poisoned")
            .clone()
    }

    fn set_block_header(&self, header: Header) {
        let mut current_header = self
            .block_header
            .write()
            .expect("RwLock will not be poisoned");
        *current_header = Some(header);
    }

    fn get_last_commit_hash(&self) -> [u8; 32] {
        self.multi_store
            .read()
            .expect("RwLock will not be poisoned")
            .get_head_commit_hash()
    }

    fn increment_block_height(&self) -> u64 {
        let mut height = self.height.write().expect("RwLock will not be poisoned");
        *height += 1;
        return *height;
    }

    fn update_block_height(&self, new_height: u64) {
        let mut height = self.height.write().expect("RwLock will not be poisoned");
        *height = new_height;
    }

    fn run_tx(&self, raw: Bytes) -> Result<Vec<tendermint_informal::abci::Event>, AppError> {
        println!("gears收到的交易为:{:?}", raw);
        let tx_with_raw: TxWithRaw<M> = TxWithRaw::from_bytes(raw.clone())
            .map_err(|e| AppError::TxParseError(e.to_string()))?;

        // println!("解码后的交易为:{:?}", tx_with_raw.tx.get_msgs());

        Self::validate_basic_tx_msgs(tx_with_raw.tx.get_msgs())?;

        // 获取Store的写锁
        let mut multi_store = self
            .multi_store
            .write()
            .expect("RwLock will not be poisoned");

        // 创建交易上下文
        let mut ctx = TxContext::new(
            &mut multi_store,
            self.get_block_height(),
            self.get_block_header()
                .expect("block header is set in begin block"),
            raw.clone().into(),
        );

        // 做执行的预检查（可以理解为模拟执行）
        match self.base_ante_handler.run(&mut ctx.as_any(), &tx_with_raw) {
            Ok(_) => multi_store.write_then_clear_tx_caches(),
            Err(e) => {
                multi_store.clear_tx_caches();
                return Err(e);
            }
        };

        // 创建交易上下文
        let mut ctx = TxContext::new(
            &mut multi_store,
            self.get_block_height(),
            self.get_block_header()
                .expect("block header is set in begin block"),
            raw.into(),
        );

        // 正式执行交易
        match self.run_msgs(&mut ctx.as_any(), tx_with_raw.tx.get_msgs()) {
            Ok(_) => {
                let events = ctx.events;
                multi_store.write_then_clear_tx_caches();
                Ok(events)
            }
            Err(e) => {
                multi_store.clear_tx_caches();
                Err(e)
            }
        }
    }

    fn run_msgs<T: Database>(
        &self,
        ctx: &mut Context<T, SK>,
        msgs: &Vec<M>,
    ) -> Result<(), AppError> {
        for msg in msgs {
            self.handler.handle_tx(ctx, msg)?
        }

        return Ok(());
    }

    fn validate_basic_tx_msgs(msgs: &Vec<M>) -> Result<(), AppError> {
        if msgs.is_empty() {
            return Err(AppError::InvalidRequest(
                "must contain at least one message".into(),
            ));
        }

        for msg in msgs {
            msg.validate_basic()
                .map_err(|e| AppError::TxValidation(e.to_string()))?
        }

        return Ok(());
    }


    // 每个节点都发布一个本地的version消息，主要是向别人说一下自己的区块高度，以及PEER ID
    async fn sync(&mut self) -> Result<bool, ()> {
        let version = Messages::Version {
            best_height: self.get_block_height(),
            from_addr: PEER_ID.to_string(),
        };

        println!("当前本地节点信息为:{:?}", version);

        let line = serde_json::to_vec(&version).unwrap();

        let p2p_reactor = &mut *self.p2p_reactor.write().unwrap();

        let swarm_arc = p2p_reactor.swarm.clone();
        let mut swarm_lock = swarm_arc.lock().await;
        let mut swarm = swarm_lock.deref_mut();

        let publish_result = swarm
            .behaviour_mut()
            .gossipsub
            .publish(BLOCK_TOPIC.clone(), line);

        match publish_result {
            Ok(message_id) => {
                info!("sync successfully");
                return Ok(true);
            }
            Err(e) => {
                error!("Failed to sync block with error: {}", e);
                return Ok(false);
            }
        }
    }

    // 处理version消息(其他节点会收到发送者的version)
    async fn process_version_msg(&mut self, best_height: u64, from_addr: String) -> Result<(), ()> {
        // 如果收到的version消息中高度是小于当前的高度，那就广播自己的状态数据，让别的节点可以同步
        println!("当前的区块高度为:{:?}, 目前已经的最新区块为{:?}", self.get_block_height(), best_height);
        if self.get_block_height() > best_height {
            let multistore = &mut *self.multi_store.write().expect("RwLock will not be poisoned");
            
            let mut states = HashMap::new();
            // 从本地module的KVStore中取出所有的Node信息
            for store in SK::iter() {
                // 获取到每个SK的kv store
                let mut kv_store = multistore.get_mutable_kv_store(&store);
                // 获取该modle的所有nodes
                let (root_hash, nodes) = kv_store.load_all_nodes(self.get_block_height().try_into().unwrap());
                // 收集将所有module得nodes信息
                states.insert(store.name().to_string(), (root_hash, nodes));
            }

            let state_msg = Messages::State {
                states,
                height: self.get_block_height(),
                to_addr: from_addr,
            };

            println!("state message 为 {:?}!!!!!", state_msg);
            let msg = serde_json::to_vec(&state_msg).unwrap();

            let p2p_reactor = &mut *self.p2p_reactor.write().unwrap();

            let swarm_arc = p2p_reactor.swarm.clone();
            let mut swarm_lock = swarm_arc.lock().await;
            let mut swarm = swarm_lock.deref_mut();

            swarm
                .behaviour_mut()
                .gossipsub
                .publish(BLOCK_TOPIC.clone(), msg)
                .unwrap();
        }
        Ok(())
    }


    // 处理state store的消息，接受者可能会收到其他发送者发送的blocks消息，并且它本身的区块高度是小于发送者的，此时就考虑接收其区块，并放入本地的状态数据库
    async fn process_state_msg(
        &mut self,
        states: HashMap<String, (String, HashMap<String, Vec<u8>>)>,
        to_addr: String,
        height: u64,
    ) -> Result<(), ()> {
        println!("接收到其他节点发送的app state, 其报告的区块高度为{:?}", height);
        println!("当前本节点的区块高度为{:?}", self.get_block_height());
        println!("接收到的states为{:?}", states);
        // 更新state store
        if PEER_ID.to_string() == to_addr && self.get_block_height() < height {
            // 更新本地的multi store为最新的multistore
            // let mut multi_store = self.multi_store.write().unwrap();
            let multi_store = &mut *self.multi_store.write().expect("RwLock will not be poisoned");

            let mut stores_info = vec![];
            for (store, modules_state_nodes) in &states {
                let store_key = SK::from_name(store.clone().as_str()).unwrap();
                // 首先通过最新的SK获取到本地kv store
                let kvstore = multi_store.get_mutable_kv_store(&store_key);
                // 更新本地的kv store(其中包含了Tree的更新)
                let root_hash = kvstore.process_all_state_info(modules_state_nodes.clone(), height);
                println!("module name is {:?}, root hash is {:?}", store, root_hash);
                // 存储每个store信息
                let store_info = StoreInfo {
                    name: store_key.name().into(),
                    hash: root_hash,
                };
                stores_info.push(store_info)
            }

            println!("实际接收到的stores info为:{:?}", stores_info);

            let multistore_hash = hash_store_infos(stores_info);

            // 和收到的哈希进行校验
            println!("multistore_hash is :{:?}", multistore_hash);

            multi_store.set_head_commit_hash(multistore_hash);
            multi_store.set_head_version(height);

            // update app height
            self.update_block_height(height);
        }
        Ok(())
    }


    pub async fn run_p2p_reactor(&mut self) -> Result<(), ()> {

        info!("start p2p module");
        // swarm info
        let p2p_reactor = self.p2p_reactor.write().unwrap();

        let swarm_arc = p2p_reactor.swarm.clone();
        let mut swarm_lock = swarm_arc.lock().await;
        let mut swarm = swarm_lock.deref_mut();

        // 这边如果是0.0.0.0/tpc/0的话，会监听两个tcp地址：一个是本地127.0.0.1/tpc/xxx，另一个是局域网192.168.xx.xx/tcp/xxx
        swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
        drop(swarm_lock);
        drop(p2p_reactor);
        loop {
            // sync peer data
            match self.sync().await {
                Ok(is_synced) => {
                    if is_synced {
                        info!("prepare syncing next block....");
                    } else {
                        info!("no block can be synced, continue to listening....");
                    }
                }
                Err(err) => {
                    error!("Error during sync");
                }
            }

            let p2p_reactor = self.p2p_reactor.write().unwrap();
            let swarm_arc = p2p_reactor.swarm.clone();

            let msg_receiver_arc = p2p_reactor.msg_receiver.clone();
            drop(p2p_reactor);

            tokio::select! {
                messages = async move{
                    let mut msg_receiver_lock = msg_receiver_arc.lock().await;
                    let msg_receiver = &mut *msg_receiver_lock;
                    msg_receiver.recv().await
                } => {
                    if let Some(msg) = messages {
                        match msg {
                            Messages::Version{best_height, from_addr} => {
                                self.process_version_msg(best_height, from_addr).await;
                            },
                            Messages::State{states, to_addr, height} => {
                                self.process_state_msg(states, to_addr, height).await;
                            },
                        }
                    }
                },
                event = async move{
                    let mut swarm_lock = swarm_arc.lock().await;
                    let swarm = &mut *swarm_lock;
                    swarm_lock.select_next_some().await
                } => {
                    match event {
                        SwarmEvent::NewListenAddr{address, ..} => {
                            info!("Listening on {:?}", address);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            info!("Connected to {:?}", peer_id);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            info!("Disconnected from {:?}", peer_id);
                        }
                        SwarmEvent::ListenerClosed { addresses, .. } => {
                            info!("Listener closed for addresses: {:?}", addresses);
                        }
                        SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error } => {
                            info!("Incoming connection error (local: {:?}, send back: {:?}): {:?}", local_addr, send_back_addr, error);
                        }
                        _ => {
                            // 其他事件类型，根据需要添加
                            info!("Unhandled event: {:?}", event);
                        }
                    }
                },
                _ = sleep(Duration::from_secs(7)) => {
                    continue;
                }
            }
        }
    }

}
