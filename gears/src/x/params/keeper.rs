use database::{Database, PrefixDB};
use store_crate::StoreKey;

use std::{hash::Hash, marker::PhantomData};
use strum::IntoEnumIterator;

use crate::types::context::Context;

pub trait ParamsSubspaceKey: Hash + Eq + IntoEnumIterator {
    fn name(&self) -> &'static str;
    fn from_name(name: &str) -> Option<Self>;
}

#[derive(Debug, Clone)]
pub struct Keeper<SK: StoreKey, PSK: ParamsSubspaceKey> {
    store_key: SK,
    p: PhantomData<PSK>,
}

impl<SK: StoreKey, PSK: ParamsSubspaceKey> Keeper<SK, PSK> {
    pub fn new(store_key: SK) -> Self {
        Keeper {
            store_key,
            p: PhantomData,
        }
    }

    pub fn get_raw_subspace<'a, DB: Database>(
        &self,
        ctx: &'a Context<DB, SK>,
        params_subspace_key: &PSK,
    ) -> store_crate::ImmutablePrefixStore<'a, PrefixDB<DB>> {
        let params_store = ctx.get_kv_store(&self.store_key);

        params_store.get_immutable_prefix_store(params_subspace_key.name().as_bytes().to_vec())
    }

    pub fn get_mutable_raw_subspace<'a, DB: Database>(
        &self,
        ctx: &'a mut Context<DB, SK>,
        params_subspace_key: &PSK,
    ) -> store_crate::MutablePrefixStore<'a, PrefixDB<DB>> {
        let params_store = ctx.get_mutable_kv_store(&self.store_key);

        params_store.get_mutable_prefix_store(params_subspace_key.name().as_bytes().to_vec())
    }
}
