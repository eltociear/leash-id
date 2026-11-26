use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub min_kyc_level: u8,
    pub denom: String,
}

/// Same storage key and shape as cw2, so standard tooling can read the contract version.
#[cw_serde]
pub struct ContractVersion {
    pub contract: String,
    pub version: String,
}

#[cw_serde]
pub struct Policy {
    pub label: String,
    pub per_tx_limit: Uint128,
    /// Limit over any rolling 24 hours (not a calendar day, so midnight can't double it).
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
    /// Spend per hour, `(unix_hour, amount)`, only the last 24 hours are kept.
    pub spend: Vec<(u64, Uint128)>,
    pub revoked: bool,
    /// A leash accepted while its owner needs co-signatures starts working only after the review delay, so a
    /// co-approver can revoke a leash the owner set up alone before it pays anything. 0 = from the start.
    #[serde(default)]
    pub active_from: u64,
}

/// A leash the owner offered; it only takes effect once the agent key accepts it.
#[cw_serde]
pub struct Offer {
    pub owner: Addr,
    pub policy: Policy,
}

#[cw_serde]
#[derive(Default)]
pub struct OwnerAcct {
    pub balance: Uint128,
    /// Held for payments that are waiting for approval.
    pub reserved: Uint128,
    /// Number of payments waiting for approval (capped, so the queue stays bounded).
    pub pending: u32,
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
    /// As the agent sent it (trimmed). Duplicates are matched on `invoice_key`.
    pub invoice: String,
    pub invoice_key: String,
    /// The address the invoice number is filed under: the vendor's main account when the vendor is a registered
    /// EVM sub-account, else the vendor. Freeing the number (reject, release) removes exactly this entry.
    #[serde(default)]
    pub invoice_party: Option<Addr>,
    pub memo: Option<String>,
    pub status: PayStatus,
    /// Who has approved a held payment so far (owner and/or co-approvers).
    #[serde(default)]
    pub approved_by: Vec<Addr>,
    pub created_at: u64,
    pub settled_at: Option<u64>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const VERSION: Item<ContractVersion> = Item::new("contract_info");
pub const OWNERS: Map<&Addr, OwnerAcct> = Map::new("owners");
pub const AGENTS: Map<&Addr, AgentRec> = Map::new("agents");
/// (agent, owner) -> offered leash, waiting for the agent's AcceptLeash.
pub const OFFERS: Map<(&Addr, &Addr), Offer> = Map::new("offers");
pub const OWNER_AGENTS: Map<(&Addr, &Addr), ()> = Map::new("owner_agents");
pub const PAYMENTS: Map<u64, Payment> = Map::new("payments");
pub const NEXT_ID: Item<u64> = Item::new("next_id");
/// (owner, vendor, invoice_key) -> payment id: one invoice is paid at most once.
pub const INVOICES: Map<(&Addr, &Addr, &str), u64> = Map::new("invoices");
/// Indexes for newest-first history and the approval queue.
pub const AGENT_PAYMENTS: Map<(&Addr, u64), ()> = Map::new("agent_payments");
pub const OWNER_PAYMENTS: Map<(&Addr, u64), ()> = Map::new("owner_payments");
pub const OWNER_PENDING: Map<(&Addr, u64), ()> = Map::new("owner_pending");
/// EVM-form sub-account -> the main account whose ME ID lists it (re-checked against x/did on every use).
pub const SUB_ACCOUNTS: Map<&Addr, Addr> = Map::new("sub_accounts");
/// Owner -> ME Hub fixed deposit ids they put up as a bond (read live on every whois).
pub const BONDS: Map<&Addr, Vec<u64>> = Map::new("bonds");
/// Owner -> co-approvers for held payments, and how many signatures (owner included) release one.
#[cw_serde]
pub struct Approvers {
    pub list: Vec<Addr>,
    pub threshold: u8,
}
pub const APPROVERS: Map<&Addr, Approvers> = Map::new("approvers");
/// (approver, owner): lets an approver find the owners whose payments wait on them.
pub const APPROVER_OF: Map<(&Addr, &Addr), ()> = Map::new("approver_of");

/// A change that loosens control while an owner needs two or more signatures. It waits `CHANGE_DELAY`, during
/// which any co-approver (or the owner) can cancel it; after that anyone can apply it. Tightening is never delayed.
#[cw_serde]
pub enum ChangeKind {
    /// The agent's new policy (key `policy:<agent>`).
    Policy(Policy),
    /// Lift the agent's revoke (key `resume:<agent>`).
    Resume {},
    /// New co-approvers and threshold (key `approvers`).
    Approvers(Approvers),
    /// Take budget back to the owner's wallet (key `withdraw`): with the owner key alone, that is where a
    /// thief would send it.
    Withdraw { amount: Uint128 },
}

#[cw_serde]
pub struct Change {
    pub kind: ChangeKind,
    pub proposed_at: u64,
    pub effective_at: u64,
    /// Co-signers who agreed to it (`approve_change`); with enough of them it applies before the 24 h are up.
    #[serde(default)]
    pub approved_by: Vec<Addr>,
}

/// (owner, key) -> a loosening change waiting out the review delay.
pub const CHANGES: Map<(&Addr, &str), Change> = Map::new("changes");
