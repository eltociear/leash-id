use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub min_kyc_level: u8,
    pub denom: String,
}

#[cw_serde]
pub struct Policy {
    pub label: String,
    pub per_tx_limit: Uint128,
    pub daily_limit: Uint128,
    pub approve_above: Option<Uint128>,
    pub vendors: Vec<Addr>,
    pub any_verified_vendor: bool,
    pub expires_at: u64,
}

#[cw_serde]
pub struct AgentRec {
    pub owner: Addr,
    pub policy: Policy,
    /// UTC day number (`block_time / 86400`) that `spent` belongs to.
    pub day: u64,
    pub spent: Uint128,
    pub revoked: bool,
}

#[cw_serde]
#[derive(Default)]
pub struct OwnerAcct {
    pub balance: Uint128,
    /// Held for payments that are waiting for approval.
    pub reserved: Uint128,
}

#[cw_serde]
pub enum PayStatus {
    Paid,
    Pending,
    Rejected,
}

#[cw_serde]
pub struct Payment {
    pub id: u64,
    pub agent: Addr,
    pub owner: Addr,
    /// The owner's ME ID number at the moment the agent paid.
    pub owner_did: String,
    pub vendor: Addr,
    /// The vendor's ME ID, when it has one.
    pub vendor_did: Option<String>,
    pub amount: Uint128,
    pub invoice: String,
    pub memo: Option<String>,
    pub status: PayStatus,
    pub created_at: u64,
    pub settled_at: Option<u64>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const OWNERS: Map<&Addr, OwnerAcct> = Map::new("owners");
pub const AGENTS: Map<&Addr, AgentRec> = Map::new("agents");
pub const OWNER_AGENTS: Map<(&Addr, &Addr), ()> = Map::new("owner_agents");
pub const PAYMENTS: Map<u64, Payment> = Map::new("payments");
pub const NEXT_ID: Item<u64> = Item::new("next_id");
/// (owner, vendor, invoice) -> payment id: one invoice is paid at most once.
pub const INVOICES: Map<(&Addr, &Addr, &str), u64> = Map::new("invoices");
