use std::collections::HashMap;

use serde::{Serialize, Deserialize};

use store::{MultiStore, StoreKey};

#[derive(Debug, Serialize, Deserialize)]
pub enum Commands {
    Genesis(String),
    Blocks(String),
    Sync(String),
    GetAddress(String),
    Trans {
        from: String,
        to: String,
        amount: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Messages {
    Version {
        best_height: u64,
        from_addr: String,
    },
    State {
        // 这里存储了每一个模块的状态数据节点(key: SK)
        // HashMap<String, HashMap<[u8; 32], Vec<u8>>>  ->  HashMap<String, String>
        // <module_name -> (module avl tree root hash, <node hash -> node data>)>
        states: HashMap<String, (String, HashMap<String, Vec<u8>>)>,
        height: u64,
        to_addr: String,
    }
}