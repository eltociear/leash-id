use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, CustomMsg, Uint128};

use crate::did::MeId;
use crate::state::{Payment, Policy};

#[cw_serde]
pub struct InstantiateMsg {
    /// Minimum `KYC_LEVEL_*` an owner (and an "any verified vendor") must hold.
    pub min_kyc_level: u8,
    /// Payment denom, `umec` on ME Hub.
    pub denom: String,
}

#[cw_serde]
pub struct PolicyMsg {
    pub label: String,
    pub per_tx_limit: Uint128,
    pub daily_limit: Uint128,
    /// Payments above this amount wait for the owner's approval.
    pub approve_above: Option<Uint128>,
    /// Explicit vendor allowlist.
    pub vendors: Vec<String>,
    /// Also allow any vendor that holds a live ME ID at `min_kyc_level`.
    pub any_verified_vendor: bool,
    /// Unix seconds; the leash stops working after this.
    pub expires_at: u64,
}

#[cw_serde]
pub struct PayItem {
    pub vendor: String,
    pub amount: Uint128,
    pub invoice: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Owner funds the agent budget (escrowed in the contract).
    Deposit {},
    Withdraw { amount: Uint128 },
    RegisterAgent { agent: String, policy: PolicyMsg },
    UpdatePolicy { agent: String, policy: PolicyMsg },
    RevokeAgent { agent: String },
    /// Called by the agent key.
    Pay {
        vendor: String,
        amount: Uint128,
        invoice: String,
        memo: Option<String>,
    },
    /// Called by the agent key; settles through ME Hub's `multi_send` custom message.
    PayBatch { payments: Vec<PayItem> },
    Approve { id: u64 },
    Reject { id: u64 },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::state::Config)]
    Config {},
    /// Who stands behind this agent, re-checked live against the DID module.
    #[returns(WhoisResponse)]
    Whois { agent: String },
    #[returns(OwnerResponse)]
    Owner { owner: String },
    /// Dry-run of `Pay` with the exact same rules.
    #[returns(CheckPayResponse)]
    CheckPay {
        agent: String,
        vendor: String,
        amount: Uint128,
        invoice: String,
    },
    #[returns(Payment)]
    Payment { id: u64 },
    #[returns(PaymentsResponse)]
    Payments {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    /// Pass-through ME ID lookup (frontends cannot call the REST API cross-origin).
    #[returns(Option<MeId>)]
    MeId { address: String },
}

#[cw_serde]
pub struct WhoisResponse {
    pub agent: String,
    pub owner: String,
    pub owner_me_id: Option<MeId>,
    /// Owner holds a live, active ME ID at the required KYC level right now.
    pub verified: bool,
    pub policy: Policy,
    pub spent_today: Uint128,
    pub remaining_today: Uint128,
    pub revoked: bool,
    pub expired: bool,
    pub owner_balance: Uint128,
}

#[cw_serde]
pub struct OwnerResponse {
    pub owner: String,
    pub me_id: Option<MeId>,
    pub balance: Uint128,
    pub reserved: Uint128,
    pub agents: Vec<String>,
}

#[cw_serde]
pub struct CheckPayResponse {
    pub ok: bool,
    pub needs_approval: bool,
    pub reason: Option<String>,
}

#[cw_serde]
pub struct PaymentsResponse {
    pub payments: Vec<Payment>,
}

// ---------- ME Hub custom message (app/keepers/wasm_me_msg.go) ----------

#[cw_serde]
pub enum MeMsg {
    MultiSend(MultiSend),
}

#[cw_serde]
pub struct MultiSend {
    pub amount: Vec<Coin>,
    pub output: Vec<Output>,
}

#[cw_serde]
pub struct Output {
    pub address: String,
    pub amount: Vec<Coin>,
}

impl CustomMsg for MeMsg {}
