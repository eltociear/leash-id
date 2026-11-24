use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("no ME ID: {addr} has no DID on ME Hub")]
    NoMeId { addr: String },

    #[error("ME ID {did} is not active")]
    MeIdInactive { did: String },

    #[error("ME ID {did} has KYC level {have}, need {need}")]
    KycTooLow { did: String, have: u8, need: u8 },

    #[error("not registered: {agent} is not a leashed agent")]
    UnknownAgent { agent: String },

    #[error("agent {agent} already has a leash; it must leave_leash first")]
    AgentTaken { agent: String },

    #[error("no leash offered to this agent by {owner}")]
    NoOffer { owner: String },

    #[error("unauthorized: only the owner can do this")]
    Unauthorized {},

    #[error("agent revoked")]
    Revoked {},

    #[error("leash expired")]
    Expired {},

    #[error("vendor {vendor} is not allowed by this leash")]
    VendorNotAllowed { vendor: String },

    #[error("self-dealing: {why}")]
    SelfDealing { why: String },

    #[error("over per-payment limit: {amount} > {limit}")]
    OverPerTx { amount: Uint128, limit: Uint128 },

    #[error("over daily limit: {spent} spent in the last 24h + {amount} > {limit}")]
    OverDaily {
        spent: Uint128,
        amount: Uint128,
        limit: Uint128,
    },

    #[error("insufficient owner budget: {available} available, {amount} needed")]
    InsufficientBudget { available: Uint128, amount: Uint128 },

    #[error("invoice {invoice} to this vendor was already handled (payment #{id})")]
    DuplicateInvoice { invoice: String, id: u64 },

    #[error("invoice {invoice} appears twice in this batch")]
    DuplicateInBatch { invoice: String },

    #[error("payment #{id} is not pending")]
    NotPending { id: u64 },

    #[error("payment #{id} waited more than 7 days; reject it and have the agent pay again")]
    PendingExpired { id: u64 },

    #[error("too many payments waiting for approval ({count}); approve or reject some first")]
    TooManyPending { count: u32 },

    #[error("this message takes no funds")]
    NonPayable {},

    #[error("invalid input: {msg}")]
    Invalid { msg: String },
}
