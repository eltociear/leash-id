use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, CustomMsg, Uint128};

use crate::did::MeId;
use crate::state::{ContractVersion, Payment, Policy};

#[cw_serde]
pub struct InstantiateMsg {
    /// Minimum `KYC_LEVEL_*` an owner (and an "any verified vendor") must hold.
    pub min_kyc_level: u8,
    /// Payment denom, `umec` on ME Hub.
    pub denom: String,
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub struct PolicyMsg {
    pub label: String,
    pub per_tx_limit: Uint128,
    /// Limit over any rolling 24 hours.
    pub daily_limit: Uint128,
    /// Payments above this amount wait for the owner's approval. Must be below `per_tx_limit`.
    pub approve_above: Option<Uint128>,
    /// Explicit vendor allowlist.
    pub vendors: Vec<String>,
    /// Also allow any vendor that holds a live ME ID at `min_kyc_level`.
    pub any_verified_vendor: bool,
    /// Unix seconds, at most 366 days ahead; the leash stops working after this.
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
    /// Owner funds the agent budget (escrowed in the contract). The only message that takes funds.
    Deposit {},
    Withdraw { amount: Uint128 },
    /// Owner offers a leash to an agent key. Nothing is in force until the agent accepts it.
    RegisterAgent { agent: String, policy: PolicyMsg },
    /// Owner withdraws an offer the agent hasn't accepted.
    CancelOffer { agent: String },
    /// Called by the agent key: take the leash `owner` offered.
    AcceptLeash { owner: String },
    /// Called by the agent key: drop its leash (pending payments are rejected).
    LeaveLeash {},
    /// Owner changes the policy. Never lifts a revoke.
    UpdatePolicy { agent: String, policy: PolicyMsg },
    /// Owner, or any one of the owner's co-approvers, stops the agent; its pending payments are rejected.
    RevokeAgent { agent: String },
    /// Owner lifts a revoke.
    ResumeAgent { agent: String },
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
    /// Owner lets an already-handled invoice number be paid again (e.g. a vendor re-issued it).
    ReleaseInvoice { vendor: String, invoice: String },
    /// Anyone: record that `sub` is the EVM-form sub-account of `main`. Accepted only if `main`'s ME ID
    /// lists `sub` in x/did, and re-checked on every use, so it can never claim more than x/did says.
    RegisterSubAccount { sub: String, main: String },
    /// Owner: ME Hub fixed deposits (ids) to show as their bond. Up to 5; each must belong to the owner's ME ID.
    SetBond { deposit_ids: Vec<u64> },
    /// Owner: co-approvers (each with a live ME ID, up to 5) and how many signatures, the owner's included,
    /// release a held payment. Any one of them can reject. `threshold: 1` with no approvers = owner alone.
    SetApprovers { approvers: Vec<String>, threshold: u8 },
    /// While two or more signatures are needed, changes that loosen control (higher limits, a higher or no
    /// approval line, new vendors, a later expiry, a resume, fewer or other co-approvers, a withdrawal) wait 24 hours
    /// under a key (`policy:<agent>`, `resume:<agent>`, `approvers`, `withdraw`). Anyone applies one after that.
    ApplyChange { owner: String, key: String },
    /// The owner, or any one co-approver, drops a waiting change.
    CancelChange { owner: String, key: String },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::state::Config)]
    Config {},
    #[returns(ContractVersion)]
    Version {},
    /// Who stands behind this agent, re-checked live against the DID module.
    #[returns(WhoisResponse)]
    Whois { agent: String },
    #[returns(OwnerResponse)]
    Owner { owner: String },
    /// Leashes offered to this agent and not yet accepted.
    #[returns(OffersResponse)]
    Offers { agent: String },
    /// Dry-run of `Pay` with the exact same rules.
    #[returns(CheckPayResponse)]
    CheckPay {
        agent: String,
        vendor: String,
        amount: Uint128,
        invoice: String,
        memo: Option<String>,
    },
    #[returns(Payment)]
    Payment { id: u64 },
    /// All payments, oldest first.
    #[returns(PaymentsResponse)]
    Payments {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    /// One agent's payments, newest first.
    #[returns(PaymentsResponse)]
    PaymentsByAgent {
        agent: String,
        start_before: Option<u64>,
        limit: Option<u32>,
    },
    /// One owner's payments (all agents), newest first.
    #[returns(PaymentsResponse)]
    PaymentsByOwner {
        owner: String,
        start_before: Option<u64>,
        limit: Option<u32>,
    },
    /// The owner's approval queue, oldest first.
    #[returns(PaymentsResponse)]
    Pending {
        owner: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    /// Was this invoice already handled? (Use after a timeout before paying again.)
    #[returns(Option<Payment>)]
    Invoice {
        owner: String,
        vendor: String,
        invoice: String,
    },
    /// Pass-through ME ID lookup, following registered sub-accounts.
    #[returns(Option<MeId>)]
    MeId { address: String },
    /// Everything the contract can vouch for about any address: ME ID (direct or via its main account) and bond.
    #[returns(IdentityResponse)]
    Identity { address: String },
    /// An owner's co-approvers and threshold.
    #[returns(crate::state::Approvers)]
    Approvers { owner: String },
    /// The owners whose held payments this address can co-approve.
    #[returns(Vec<String>)]
    ApproverOf { approver: String },
    /// Changes waiting out the review delay: `(key, change)`.
    #[returns(Vec<(String, crate::state::Change)>)]
    Changes { owner: String },
}

#[cw_serde]
pub struct Bond {
    /// Sum of the owner's registered fixed deposits that are still locked, in the contract denom.
    pub amount: Uint128,
    /// The earliest of their unlock times (unix seconds).
    pub earliest_unlock: u64,
    pub deposits: Vec<crate::me::Deposit>,
}

#[cw_serde]
pub struct IdentityResponse {
    pub address: String,
    pub me_id: Option<MeId>,
    /// Set when the ME ID was found through a registered sub-account: the main account that holds it.
    pub via_main: Option<String>,
    pub bond: Option<Bond>,
}

#[cw_serde]
pub struct WhoisResponse {
    pub agent: String,
    pub owner: String,
    pub owner_me_id: Option<MeId>,
    /// The owner's locked ME Hub fixed deposits, if they registered any.
    pub owner_bond: Option<Bond>,
    /// Owner holds a live, active ME ID at the required KYC level right now.
    pub verified: bool,
    pub min_kyc_level: u8,
    pub policy: Policy,
    pub spent_24h: Uint128,
    pub remaining_24h: Uint128,
    pub revoked: bool,
    pub expired: bool,
    /// Owner's budget not reserved for pending payments.
    pub owner_balance: Uint128,
    pub pending_count: u32,
    /// Signatures (owner included) that release a held payment.
    pub approvals_needed: u8,
    /// Could this agent pay anything at all right now? If not, `blocked_by` says why.
    pub can_pay_now: bool,
    pub blocked_by: Option<String>,
    /// Set while a leash accepted under co-signing waits out its review delay (unix seconds it starts).
    #[serde(default)]
    pub active_from: Option<u64>,
    /// A loosening of this leash waiting out the review delay, if any.
    #[serde(default)]
    pub pending_change: Option<crate::state::Change>,
}

#[cw_serde]
pub struct OwnerResponse {
    pub owner: String,
    pub me_id: Option<MeId>,
    pub balance: Uint128,
    pub reserved: Uint128,
    pub pending_count: u32,
    pub agents: Vec<String>,
}

#[cw_serde]
pub struct OffersResponse {
    pub offers: Vec<crate::state::Offer>,
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
