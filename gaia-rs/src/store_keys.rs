use gears::x::params::ParamsSubspaceKey;
use store::StoreKey;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug, PartialEq, Eq, Hash, Clone)]
pub enum GaiaStoreKey {
    Bank,
    Auth,
    Params,
}

/// WARNING: a key name must not be a prefix of another, there is currently
/// no check in the SDK to prevent this.
impl StoreKey for GaiaStoreKey {
    fn name(&self) -> &'static str {
        match self {
            GaiaStoreKey::Bank => "bank",
            GaiaStoreKey::Auth => "acc",
            GaiaStoreKey::Params => "params",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "bank" => Some(GaiaStoreKey::Bank),
            "acc" => Some(GaiaStoreKey::Auth),
            "params" => Some(GaiaStoreKey::Params),
            _ => None,
        }
    }
}

#[derive(EnumIter, Debug, PartialEq, Eq, Hash, Clone)]
pub enum GaiaParamsStoreKey {
    Bank,
    Auth,
    BaseApp,
}

/// WARNING: a key name must not be a prefix of another, there is currently
/// no check in the SDK to prevent this.
impl ParamsSubspaceKey for GaiaParamsStoreKey {
    fn name(&self) -> &'static str {
        match self {
            Self::Bank => "bank/",
            Self::Auth => "auth/",
            Self::BaseApp => "baseapp/",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "bank/" => Some(GaiaParamsStoreKey::Bank),
            "auth/" => Some(GaiaParamsStoreKey::Auth),
            "baseapp/" => Some(GaiaParamsStoreKey::BaseApp),
            _ => None,
        }
    }
}
