use std::collections::{BTreeSet, HashMap};

use database::Database;
use integer_encoding::VarInt;

use crate::{merkle::EMPTY_HASH, Error, iavl::LeafNode};

use super::Node;

#[derive(Debug)]
pub struct NodeDB<T>
where
    T: Database,
{
    db: T,
}

const ROOTS_PREFIX: [u8; 1] = [1];
const NODES_PREFIX: [u8; 1] = [2];

impl<T> NodeDB<T>
where
    T: Database,
{
    pub fn new(db: T) -> NodeDB<T> {
        NodeDB { db }
    }

    pub fn get_versions(&self) -> BTreeSet<u64> {
        self.db
            .prefix_iterator(ROOTS_PREFIX.into())
            .map(|(k, _)| {
                u64::decode_var(&k)
                    .expect("invalid data in database - possible database corruption")
                    .0
            })
            .collect()
    }

    pub fn get_root_hash(&self, version: u64) -> Result<[u8; 32], Error> {
        self.db
            .get(&Self::get_root_key(version))
            .map(|hash| {
                hash.try_into()
                    .expect("invalid data in database - possible database corruption")
            })
            .ok_or(Error::VersionNotFound)
    }

    pub fn get_root_node(&self, version: u64) -> Result<Option<Node>, Error> {
        let root_hash = self.get_root_hash(version)?;

        if root_hash == EMPTY_HASH {
            return Ok(None);
        }

        Ok(Some(
            self.get_node(&root_hash)
                .expect("invalid data in database - possible database corruption"), // this node should be in the DB, if it isn't then better to panic
        ))
    }

    fn get_root_key(version: u64) -> Vec<u8> {
        [ROOTS_PREFIX.into(), version.encode_var_vec()].concat()
    }

    fn get_node_key(hash: &[u8; 32]) -> Vec<u8> {
        [NODES_PREFIX.to_vec(), hash.to_vec()].concat()
    }

    pub(crate) fn get_node(&self, hash: &[u8; 32]) -> Option<Node> {
        let node_bytes = self.db.get(&Self::get_node_key(hash))?;

        Some(
            Node::deserialize(node_bytes)
                .expect("invalid data in database - possible database corruption"),
        )
    }

    // pub fn get_all_nodes(&self, version: u64) -> HashMap<[u8; 32], Vec<u8>> {
    pub fn get_all_nodes(&self, version: u64) -> (String, HashMap<String, Vec<u8>>) {
        let mut nodes_map = HashMap::new();
        // TODO: 这里的错误处理需要进一步优化，处理None的情况
        let root_node = self.get_root_node(version).unwrap().unwrap();
        let root_hash = self.get_root_hash(version).unwrap();
        // 将根节点存储
        // nodes_map.insert(u8_array_to_hex_string(&root_hash), root_node.serialize());
        // 递归遍历iavl树，并将所有非根节点存储
        self.recursive_get_all_nodes(&root_node, &mut nodes_map);

        (u8_array_to_hex_string(&root_hash), nodes_map)
    }
    
    fn recursive_get_all_nodes(&self, node: &Node, nodes_map: &mut HashMap<String, Vec<u8>>) {
        println!("开始递归进行节点的遍历");
        if let Node::Inner(inner) = node {
            println!("当前节点的信息为:{:?}", node);
            println!("当前节点的key值为:{:?}", node.get_key());
            // 将节点存储在 HashMap 中，使用节点的哈希作为键
            nodes_map.insert(u8_array_to_hex_string(&node.hash()), node.clone().serialize());
            // 如果节点是内部节点，继续递归遍历左右子树
            if let Some(left_node) = self.get_node(&inner.left_hash) {
                // NOTE: 注意这里，在从根节点开始遍历的时候，根节点的左右节点是None，因为之前在存储之后就删除了左右子树的引用，所以这里通过哈希来重新索引
                // 即根节点存储了左右子树根节点的哈希的，通过哈希来获取到节点即可
                println!("开始遍历左节点");
                self.recursive_get_all_nodes(&left_node, nodes_map);
            }
            if let Some(right_node) = self.get_node(&inner.right_hash) {
                println!("开始遍历右节点");
                self.recursive_get_all_nodes(&right_node, nodes_map);
            }
        }else {
            println!("🍃当前节点的信息为:{:?}", node);
            nodes_map.insert(u8_array_to_hex_string(&node.hash()), node.clone().serialize());
        }
    }

    pub fn save_node(&mut self, node: &Node, hash: &[u8; 32]) {
        // <节点哈希，节点本身>
        self.db.put(Self::get_node_key(hash), node.serialize());
    }

    fn recursive_tree_save(&mut self, node: &Node, hash: &[u8; 32]) {
        if let Node::Inner(inner) = node {
            if let Some(left_node) = &inner.left_node {
                self.recursive_tree_save(&*left_node, &inner.left_hash);
            }
            if let Some(right_node) = &inner.right_node {
                self.recursive_tree_save(&*right_node, &inner.right_hash);
            }
        }

        self.save_node(node, hash)
    }

    /// Saves the given node and all of its descendants.
    /// Clears left_node/right_node on the root.
    pub(crate) fn save_tree(&mut self, root: &mut Node) -> [u8; 32] {
        // 哈希根节点
        let root_hash = root.hash();
        self.recursive_tree_save(root, &root_hash);

        // 注意这里在递归保存所有节点之后把根节点的左右子树的索引删除了！！
        if let Node::Inner(inner) = root {
            inner.left_node = None;
            inner.right_node = None;
        }

        return root_hash;
    }

    pub(crate) fn save_version(&mut self, version: u64, hash: &[u8; 32]) {
        let key = Self::get_root_key(version);
        self.db.put(key, hash.to_vec());
    }
}

pub fn u8_array_to_hex_string(bytes: &[u8; 32]) -> String {
    let hex_chars: Vec<String> = bytes.iter().map(|byte| format!("{:02x}", byte)).collect();
    hex_chars.concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::MemDB;

    #[test]
    fn get_root_key_works() {
        let key = NodeDB::<MemDB>::get_root_key(1u64);
        assert_eq!(key, vec![1, 1])
    }

    #[test]
    fn get_node_key_works() {
        let key = NodeDB::<MemDB>::get_node_key(&[
            13, 181, 53, 227, 140, 38, 242, 22, 94, 152, 94, 71, 0, 89, 35, 122, 129, 85, 55, 190,
            253, 226, 35, 230, 65, 214, 244, 35, 69, 39, 223, 90,
        ]);
        assert_eq!(
            key,
            vec![
                2, 13, 181, 53, 227, 140, 38, 242, 22, 94, 152, 94, 71, 0, 89, 35, 122, 129, 85,
                55, 190, 253, 226, 35, 230, 65, 214, 244, 35, 69, 39, 223, 90
            ]
        )
    }

    #[test]
    fn get_versions_works() {
        let db = MemDB::new();
        db.put(NodeDB::<MemDB>::get_root_key(1u64), vec![]);
        let node_db = NodeDB { db };

        let mut expected_versions = BTreeSet::new();
        expected_versions.insert(1);
        let versions = node_db.get_versions();

        assert_eq!(expected_versions, versions)
    }

    #[test]
    fn get_root_hash_works() {
        let root_hash = [
            13, 181, 53, 227, 140, 38, 242, 22, 94, 152, 94, 71, 0, 89, 35, 122, 129, 85, 55, 190,
            253, 226, 35, 230, 65, 214, 244, 35, 69, 39, 223, 90,
        ];
        let db = MemDB::new();
        db.put(NodeDB::<MemDB>::get_root_key(1u64), root_hash.into());
        let node_db = NodeDB { db };

        let got_root_hash = node_db.get_root_hash(1).unwrap();

        assert_eq!(root_hash, got_root_hash);
    }
}
