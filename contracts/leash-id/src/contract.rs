use cosmwasm_std::{
    coins, to_json_binary, Addr, BankMsg, Binary, CosmosMsg, Deps, DepsMut, Env, Event,
    MessageInfo, Order, Response, StdError, StdResult, Storage, Uint128,
};
use cw_storage_plus::Bound;

use crate::did::{self, MeId};
use crate::error::ContractError;
use crate::msg::{
    CheckPayResponse, ExecuteMsg, InstantiateMsg, MeMsg, MigrateMsg, MultiSend, OffersResponse,
    Output, OwnerResponse, PayItem, PaymentsResponse, PolicyMsg, QueryMsg, WhoisResponse,
};
use crate::state::{
    AgentRec, Config, ContractVersion, Offer, OwnerAcct, PayStatus, Payment, Policy, AGENTS,
    AGENT_PAYMENTS, CONFIG, INVOICES, NEXT_ID, OFFERS, OWNERS, OWNER_AGENTS, OWNER_PAYMENTS,
    OWNER_PENDING, PAYMENTS, VERSION,
};

pub type Res = Result<Response<MeMsg>, ContractError>;

pub const CONTRACT_NAME: &str = "crates.io:leash-id";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_VENDORS: usize = 32;
const MAX_BATCH: usize = 20;
const MAX_AGENTS_PER_OWNER: usize = 50;
/// Bounds the approval queue, so revoke/leave (which reject an agent's pending payments) stay cheap.
pub const MAX_PENDING: u32 = 50;
pub const PENDING_TTL: u64 = 7 * 86400;
const MAX_LEASH: u64 = 366 * 86400;
const HOUR: u64 = 3600;
const WINDOW_HOURS: u64 = 24;
const MAX_INVOICE: usize = 64;
const MAX_MEMO: usize = 256;

pub fn instantiate(deps: DepsMut, _env: Env, info: MessageInfo, msg: InstantiateMsg) -> Res {
    nonpayable(&info)?;
    if msg.min_kyc_level > 5 || msg.denom.is_empty() {
        return Err(invalid("min_kyc_level must be 0..=5 and denom non-empty"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            min_kyc_level: msg.min_kyc_level,
            denom: msg.denom,
        },
    )?;
    VERSION.save(
        deps.storage,
        &ContractVersion {
            contract: CONTRACT_NAME.into(),
            version: CONTRACT_VERSION.into(),
        },
    )?;
    NEXT_ID.save(deps.storage, &1)?;
    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("version", CONTRACT_VERSION))
}

pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Res {
    let old = VERSION.may_load(deps.storage)?;
    if let Some(old) = &old {
        if old.contract != CONTRACT_NAME {
            return Err(invalid("migrating from a different contract"));
        }
        if semver(&old.version) >= semver(CONTRACT_VERSION) {
            return Err(invalid("can only migrate to a newer version"));
        }
    }
    VERSION.save(
        deps.storage,
        &ContractVersion {
            contract: CONTRACT_NAME.into(),
            version: CONTRACT_VERSION.into(),
        },
    )?;
    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("from", old.map_or("none".into(), |o| o.version))
        .add_attribute("to", CONTRACT_VERSION))
}

fn semver(v: &str) -> (u64, u64, u64) {
    let mut it = v.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> Res {
    // Only Deposit takes coins. Anything attached to another message would sit in the contract,
    // credited to nobody.
    if !matches!(msg, ExecuteMsg::Deposit {}) {
        nonpayable(&info)?;
    }
    match msg {
        ExecuteMsg::Deposit {} => deposit(deps, info),
        ExecuteMsg::Withdraw { amount } => withdraw(deps, info, amount),
        ExecuteMsg::RegisterAgent { agent, policy } => offer(deps, env, info, agent, policy),
        ExecuteMsg::CancelOffer { agent } => cancel_offer(deps, info, agent),
        ExecuteMsg::AcceptLeash { owner } => accept(deps, env, info, owner),
        ExecuteMsg::LeaveLeash {} => leave(deps, env, info),
        ExecuteMsg::UpdatePolicy { agent, policy } => update_policy(deps, env, info, agent, policy),
        ExecuteMsg::RevokeAgent { agent } => revoke(deps, env, info, agent),
        ExecuteMsg::ResumeAgent { agent } => resume(deps, info, agent),
        ExecuteMsg::Pay {
            vendor,
            amount,
            invoice,
            memo,
        } => pay(deps, env, info, vendor, amount, invoice, memo),
        ExecuteMsg::PayBatch { payments } => pay_batch(deps, env, info, payments),
        ExecuteMsg::Approve { id } => settle(deps, env, info, id, true),
        ExecuteMsg::Reject { id } => settle(deps, env, info, id, false),
        ExecuteMsg::ReleaseInvoice { vendor, invoice } => release_invoice(deps, info, vendor, invoice),
    }
}

// ---------------- helpers ----------------

fn invalid(msg: &str) -> ContractError {
    ContractError::Invalid {
        msg: msg.to_string(),
    }
}

fn nonpayable(info: &MessageInfo) -> Result<(), ContractError> {
    if info.funds.is_empty() {
        Ok(())
    } else {
        Err(ContractError::NonPayable {})
    }
}

/// The address must hold a live ME ID that is active and at the required KYC level.
fn verified(deps: Deps, cfg: &Config, addr: &Addr) -> Result<MeId, ContractError> {
    let id = did::lookup(deps, addr.as_str()).ok_or_else(|| ContractError::NoMeId {
        addr: addr.to_string(),
    })?;
    if !id.active {
        return Err(ContractError::MeIdInactive { did: id.did });
    }
    if id.kyc_level < cfg.min_kyc_level {
        return Err(ContractError::KycTooLow {
            did: id.did,
            have: id.kyc_level,
            need: cfg.min_kyc_level,
        });
    }
    Ok(id)
}

/// Invoice numbers are matched on a key that ignores case and separators, so "INV-1001",
/// "inv 1001" and "INV_1001" are one invoice. Only printable ASCII is accepted, which rules out
/// look-alike Unicode (fullwidth digits, non-breaking hyphens) that would otherwise dodge the match.
pub fn invoice_key(invoice: &str) -> Result<(String, String), ContractError> {
    let shown = invoice.trim();
    if shown.is_empty() || shown.chars().count() > MAX_INVOICE {
        return Err(invalid("invoice must be 1..64 characters"));
    }
    if !shown.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        return Err(invalid("invoice must be printable ASCII"));
    }
    let key: String = shown
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if key.is_empty() {
        return Err(invalid("invoice needs at least one letter or digit"));
    }
    Ok((shown.to_string(), key))
}

fn hour(env: &Env) -> u64 {
    env.block.time.seconds() / HOUR
}

/// Spend in the rolling 24 hours that end now.
fn spent_24h(rec: &AgentRec, env: &Env) -> Uint128 {
    let now = hour(env);
    rec.spend
        .iter()
        .filter(|(h, _)| *h + WINDOW_HOURS > now)
        .map(|(_, a)| *a)
        .fold(Uint128::zero(), |a, b| a.saturating_add(b))
}

fn add_spend(rec: &mut AgentRec, env: &Env, amount: Uint128) {
    let now = hour(env);
    rec.spend.retain(|(h, _)| *h + WINDOW_HOURS > now);
    match rec.spend.iter_mut().find(|(h, _)| *h == now) {
        Some((_, a)) => *a += amount,
        None => rec.spend.push((now, amount)),
    }
}

fn payment_event(kind: &str, p: &Payment) -> Event {
    let mut ev = Event::new("leash_payment")
        .add_attribute("kind", kind)
        .add_attribute("payment_id", p.id.to_string())
        .add_attribute("status", status_str(&p.status))
        .add_attribute("agent", &p.agent)
        .add_attribute("owner", &p.owner)
        .add_attribute("owner_did", &p.owner_did)
        .add_attribute("vendor", &p.vendor)
        .add_attribute("amount", p.amount)
        .add_attribute("invoice", &p.invoice)
        .add_attribute("invoice_key", &p.invoice_key);
    // wasmd rejects empty attribute values ("Empty attribute value"), so only emit it when present.
    if let Some(vd) = &p.vendor_did {
        ev = ev.add_attribute("vendor_did", vd);
    }
    ev
}

fn status_str(s: &PayStatus) -> &'static str {
    match s {
        PayStatus::Paid => "paid",
        PayStatus::Pending => "pending",
        PayStatus::Rejected => "rejected",
    }
}

// ---------------- owner funds ----------------

fn deposit(deps: DepsMut, info: MessageInfo) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let id = verified(deps.as_ref(), &cfg, &info.sender)?;
    if AGENTS.has(deps.storage, &info.sender) {
        return Err(invalid("an agent key cannot also be an owner"));
    }
    if info.funds.len() != 1 || info.funds[0].denom != cfg.denom || info.funds[0].amount.is_zero()
    {
        return Err(invalid("send exactly one non-zero coin of the leash denom"));
    }
    let amt = info.funds[0].amount;
    let mut acct = OWNERS
        .may_load(deps.storage, &info.sender)?
        .unwrap_or_default();
    acct.balance += amt;
    OWNERS.save(deps.storage, &info.sender, &acct)?;
    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("owner", info.sender)
        .add_attribute("owner_did", id.did)
        .add_attribute("amount", amt)
        .add_attribute("balance", acct.balance))
}

fn withdraw(deps: DepsMut, info: MessageInfo, amount: Uint128) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    // Only an owner who deposited can withdraw. An agent (or anyone else) gets a plain refusal.
    let mut acct = OWNERS
        .may_load(deps.storage, &info.sender)?
        .ok_or(ContractError::Unauthorized {})?;
    let available = acct.balance - acct.reserved;
    if amount.is_zero() || amount > available {
        return Err(ContractError::InsufficientBudget { available, amount });
    }
    acct.balance -= amount;
    OWNERS.save(deps.storage, &info.sender, &acct)?;
    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: coins(amount.u128(), cfg.denom),
        })
        .add_attribute("action", "withdraw")
        .add_attribute("owner", info.sender)
        .add_attribute("amount", amount)
        .add_attribute("balance", acct.balance))
}

// ---------------- leash management ----------------

fn build_policy(deps: Deps, env: &Env, agent: &Addr, p: PolicyMsg) -> Result<Policy, ContractError> {
    if p.label.trim().is_empty() || p.label.chars().count() > 64 {
        return Err(invalid("label must be 1..64 characters"));
    }
    if p.daily_limit.is_zero() || p.per_tx_limit.is_zero() || p.per_tx_limit > p.daily_limit {
        return Err(invalid("need 0 < per_tx_limit <= daily_limit"));
    }
    if let Some(t) = p.approve_above {
        if t.is_zero() || t >= p.per_tx_limit {
            return Err(invalid(
                "approve_above must be > 0 and below per_tx_limit (omit it to never hold payments)",
            ));
        }
    }
    if p.vendors.len() > MAX_VENDORS {
        return Err(invalid("too many vendors (max 32)"));
    }
    if p.vendors.is_empty() && !p.any_verified_vendor {
        return Err(invalid("allow at least one vendor or any_verified_vendor"));
    }
    let now = env.block.time.seconds();
    if p.expires_at <= now || p.expires_at > now + MAX_LEASH {
        return Err(invalid("expires_at must be in the future and at most 366 days away"));
    }
    let mut vendors = Vec::with_capacity(p.vendors.len());
    for v in p.vendors {
        let a = deps.api.addr_validate(&v)?;
        vendor_sane(deps, env, agent, &a)?;
        if !vendors.contains(&a) {
            vendors.push(a);
        }
    }
    Ok(Policy {
        label: p.label.trim().to_string(),
        per_tx_limit: p.per_tx_limit,
        daily_limit: p.daily_limit,
        approve_above: p.approve_above,
        vendors,
        any_verified_vendor: p.any_verified_vendor,
        expires_at: p.expires_at,
    })
}

/// Money must leave the leash: not to the agent itself, the contract, or another leashed agent.
fn vendor_sane(deps: Deps, env: &Env, agent: &Addr, vendor: &Addr) -> Result<(), ContractError> {
    let why = if vendor == agent {
        "an agent cannot pay itself"
    } else if vendor == env.contract.address {
        "the contract cannot be a vendor"
    } else if AGENTS.has(deps.storage, vendor) {
        "a leashed agent cannot be a vendor"
    } else {
        return Ok(());
    };
    Err(ContractError::SelfDealing { why: why.into() })
}

fn offer(deps: DepsMut, env: Env, info: MessageInfo, agent: String, p: PolicyMsg) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let id = verified(deps.as_ref(), &cfg, &info.sender)?;
    let agent = deps.api.addr_validate(&agent)?;
    if agent == info.sender {
        return Err(invalid("the agent key must differ from the owner key"));
    }
    if OWNERS.has(deps.storage, &agent) {
        return Err(invalid("an owner account cannot be leashed as an agent"));
    }
    if let Some(rec) = AGENTS.may_load(deps.storage, &agent)? {
        if rec.owner == info.sender {
            return Err(invalid("already your agent; use update_policy"));
        }
    }
    let policy = build_policy(deps.as_ref(), &env, &agent, p)?;
    OFFERS.save(
        deps.storage,
        (&agent, &info.sender),
        &Offer {
            owner: info.sender.clone(),
            policy,
        },
    )?;
    Ok(Response::new()
        .add_attribute("action", "offer_leash")
        .add_attribute("owner", info.sender)
        .add_attribute("owner_did", id.did)
        .add_attribute("agent", agent))
}

fn cancel_offer(deps: DepsMut, info: MessageInfo, agent: String) -> Res {
    let agent = deps.api.addr_validate(&agent)?;
    if !OFFERS.has(deps.storage, (&agent, &info.sender)) {
        return Err(ContractError::NoOffer {
            owner: info.sender.to_string(),
        });
    }
    OFFERS.remove(deps.storage, (&agent, &info.sender));
    Ok(Response::new()
        .add_attribute("action", "cancel_offer")
        .add_attribute("owner", info.sender)
        .add_attribute("agent", agent))
}

fn accept(deps: DepsMut, env: Env, info: MessageInfo, owner: String) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let agent = info.sender;
    let owner = deps.api.addr_validate(&owner)?;
    if AGENTS.has(deps.storage, &agent) {
        return Err(ContractError::AgentTaken {
            agent: agent.to_string(),
        });
    }
    if OWNERS.has(deps.storage, &agent) {
        return Err(invalid("an owner account cannot be leashed as an agent"));
    }
    let off = OFFERS
        .may_load(deps.storage, (&agent, &owner))?
        .ok_or_else(|| ContractError::NoOffer {
            owner: owner.to_string(),
        })?;
    let id = verified(deps.as_ref(), &cfg, &owner)?;
    if env.block.time.seconds() >= off.policy.expires_at {
        return Err(ContractError::Expired {});
    }
    let count = OWNER_AGENTS
        .prefix(&owner)
        .keys(deps.storage, None, None, Order::Ascending)
        .take(MAX_AGENTS_PER_OWNER)
        .count();
    if count >= MAX_AGENTS_PER_OWNER {
        return Err(invalid("this owner already has 50 agents"));
    }
    AGENTS.save(
        deps.storage,
        &agent,
        &AgentRec {
            owner: owner.clone(),
            policy: off.policy,
            spend: vec![],
            revoked: false,
        },
    )?;
    OFFERS.remove(deps.storage, (&agent, &owner));
    OWNER_AGENTS.save(deps.storage, (&owner, &agent), &())?;
    OWNERS.update(deps.storage, &owner, |a| -> StdResult<_> {
        Ok(a.unwrap_or_default())
    })?;
    Ok(Response::new()
        .add_attribute("action", "accept_leash")
        .add_attribute("owner", owner)
        .add_attribute("owner_did", id.did)
        .add_attribute("agent", agent))
}

fn owned(deps: Deps, info: &MessageInfo, agent: &str) -> Result<(Addr, AgentRec), ContractError> {
    let agent = deps.api.addr_validate(agent)?;
    let rec = AGENTS
        .may_load(deps.storage, &agent)?
        .ok_or_else(|| ContractError::UnknownAgent {
            agent: agent.to_string(),
        })?;
    if rec.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok((agent, rec))
}

fn update_policy(deps: DepsMut, env: Env, info: MessageInfo, agent: String, p: PolicyMsg) -> Res {
    let (agent, mut rec) = owned(deps.as_ref(), &info, &agent)?;
    // Only the policy changes: a revoked agent stays revoked, and the 24h spend window is kept.
    rec.policy = build_policy(deps.as_ref(), &env, &agent, p)?;
    AGENTS.save(deps.storage, &agent, &rec)?;
    Ok(Response::new()
        .add_attribute("action", "update_policy")
        .add_attribute("owner", info.sender)
        .add_attribute("agent", agent)
        .add_attribute("per_tx_limit", rec.policy.per_tx_limit)
        .add_attribute("daily_limit", rec.policy.daily_limit)
        .add_attribute("expires_at", rec.policy.expires_at.to_string()))
}

/// Rejects every pending payment of `agent` (bounded by MAX_PENDING). Returns the events.
fn reject_pending_of(storage: &mut dyn Storage, env: &Env, owner: &Addr, agent: &Addr) -> StdResult<Vec<Event>> {
    let ids: Vec<u64> = OWNER_PENDING
        .prefix(owner)
        .keys(storage, None, None, Order::Ascending)
        .take(MAX_PENDING as usize)
        .collect::<StdResult<_>>()?;
    let mut acct = OWNERS.load(storage, owner)?;
    let mut events = vec![];
    for id in ids {
        let mut p = PAYMENTS.load(storage, id)?;
        if p.agent != *agent {
            continue;
        }
        close_pending(storage, &mut acct, &mut p, env, false)?;
        events.push(payment_event("rejected", &p));
    }
    OWNERS.save(storage, owner, &acct)?;
    Ok(events)
}

/// Takes a pending payment out of the queue. Rejecting also frees its invoice number.
fn close_pending(storage: &mut dyn Storage, acct: &mut OwnerAcct, p: &mut Payment, env: &Env, paid: bool) -> StdResult<()> {
    acct.reserved -= p.amount;
    acct.pending = acct.pending.saturating_sub(1);
    OWNER_PENDING.remove(storage, (&p.owner, p.id));
    if paid {
        acct.balance -= p.amount;
        p.status = PayStatus::Paid;
    } else {
        INVOICES.remove(storage, (&p.owner, &p.vendor, p.invoice_key.as_str()));
        p.status = PayStatus::Rejected;
    }
    p.settled_at = Some(env.block.time.seconds());
    PAYMENTS.save(storage, p.id, p)
}

fn revoke(deps: DepsMut, env: Env, info: MessageInfo, agent: String) -> Res {
    let (agent, mut rec) = owned(deps.as_ref(), &info, &agent)?;
    rec.revoked = true;
    AGENTS.save(deps.storage, &agent, &rec)?;
    let events = reject_pending_of(deps.storage, &env, &rec.owner, &agent)?;
    Ok(Response::new()
        .add_attribute("action", "revoke_agent")
        .add_attribute("owner", info.sender)
        .add_attribute("agent", agent)
        .add_attribute("rejected_pending", events.len().to_string())
        .add_events(events))
}

fn resume(deps: DepsMut, info: MessageInfo, agent: String) -> Res {
    let (agent, mut rec) = owned(deps.as_ref(), &info, &agent)?;
    if !rec.revoked {
        return Err(invalid("agent is not revoked"));
    }
    rec.revoked = false;
    AGENTS.save(deps.storage, &agent, &rec)?;
    Ok(Response::new()
        .add_attribute("action", "resume_agent")
        .add_attribute("owner", info.sender)
        .add_attribute("agent", agent))
}

fn leave(deps: DepsMut, env: Env, info: MessageInfo) -> Res {
    let agent = info.sender;
    let rec = load_agent(deps.as_ref(), &agent)?;
    let events = reject_pending_of(deps.storage, &env, &rec.owner, &agent)?;
    AGENTS.remove(deps.storage, &agent);
    OWNER_AGENTS.remove(deps.storage, (&rec.owner, &agent));
    Ok(Response::new()
        .add_attribute("action", "leave_leash")
        .add_attribute("owner", rec.owner)
        .add_attribute("agent", agent)
        .add_attribute("rejected_pending", events.len().to_string())
        .add_events(events))
}

fn release_invoice(deps: DepsMut, info: MessageInfo, vendor: String, invoice: String) -> Res {
    let vendor = deps.api.addr_validate(&vendor)?;
    let (_, key) = invoice_key(&invoice)?;
    let id = INVOICES
        .may_load(deps.storage, (&info.sender, &vendor, key.as_str()))?
        .ok_or_else(|| invalid("no such invoice for this owner and vendor"))?;
    let p = PAYMENTS.load(deps.storage, id)?;
    if p.status == PayStatus::Pending {
        return Err(invalid("that invoice is pending; reject it instead"));
    }
    INVOICES.remove(deps.storage, (&info.sender, &vendor, key.as_str()));
    Ok(Response::new()
        .add_attribute("action", "release_invoice")
        .add_attribute("owner", info.sender)
        .add_attribute("vendor", vendor)
        .add_attribute("invoice_key", key)
        .add_attribute("payment_id", id.to_string()))
}

// ---------------- the leash itself ----------------

struct Decision {
    needs_approval: bool,
    owner_did: String,
    vendor_did: Option<String>,
}

/// Every rule a payment must pass. `Pay`, `PayBatch` and the `CheckPay` dry-run all go
/// through here, so the dry-run can never disagree with the real thing.
#[allow(clippy::too_many_arguments)]
fn evaluate(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    agent: &Addr,
    rec: &AgentRec,
    acct: &OwnerAcct,
    vendor: &Addr,
    amount: Uint128,
    invoice_key: &str,
    shown: &str,
    memo: Option<&str>,
    in_flight: &[(Addr, String)],
) -> Result<Decision, ContractError> {
    if rec.revoked {
        return Err(ContractError::Revoked {});
    }
    if env.block.time.seconds() >= rec.policy.expires_at {
        return Err(ContractError::Expired {});
    }
    if amount.is_zero() {
        return Err(invalid("amount must be > 0"));
    }
    if memo.map_or(false, |m| m.chars().count() > MAX_MEMO) {
        return Err(invalid("memo must be at most 256 characters"));
    }
    // The human behind the agent must still hold a live ME ID. Suspend the ME ID and
    // every agent under it stops paying on the next block.
    let owner_id = verified(deps, cfg, &rec.owner)?;
    // Duplicates first: "already paid as #N" is the useful answer, even when a limit would also refuse.
    if in_flight.iter().any(|(v, k)| v == vendor && k == invoice_key) {
        return Err(ContractError::DuplicateInBatch {
            invoice: shown.to_string(),
        });
    }
    if let Some(id) = INVOICES.may_load(deps.storage, (&rec.owner, vendor, invoice_key))? {
        return Err(ContractError::DuplicateInvoice {
            invoice: shown.to_string(),
            id,
        });
    }
    vendor_sane(deps, env, agent, vendor)?;
    let vendor_id = did::lookup(deps, vendor.as_str());
    if !vendor_allowed(cfg, &rec.policy, vendor, vendor_id.as_ref()) {
        return Err(ContractError::VendorNotAllowed {
            vendor: vendor.to_string(),
        });
    }
    if amount > rec.policy.per_tx_limit {
        return Err(ContractError::OverPerTx {
            amount,
            limit: rec.policy.per_tx_limit,
        });
    }
    let spent = spent_24h(rec, env);
    if spent.checked_add(amount).map_or(true, |t| t > rec.policy.daily_limit) {
        return Err(ContractError::OverDaily {
            spent,
            amount,
            limit: rec.policy.daily_limit,
        });
    }
    let available = acct.balance - acct.reserved;
    if amount > available {
        return Err(ContractError::InsufficientBudget { available, amount });
    }
    let needs_approval = rec.policy.approve_above.map_or(false, |t| amount > t);
    if needs_approval && acct.pending >= MAX_PENDING {
        return Err(ContractError::TooManyPending {
            count: acct.pending,
        });
    }
    Ok(Decision {
        needs_approval,
        owner_did: owner_id.did,
        vendor_did: vendor_id.map(|v| v.did),
    })
}

fn vendor_allowed(cfg: &Config, policy: &Policy, vendor: &Addr, vendor_id: Option<&MeId>) -> bool {
    let verified_vendor = vendor_id
        .map(|v| v.active && v.kyc_level >= cfg.min_kyc_level)
        .unwrap_or(false);
    policy.vendors.contains(vendor) || (policy.any_verified_vendor && verified_vendor)
}

fn load_agent(deps: Deps, agent: &Addr) -> Result<AgentRec, ContractError> {
    AGENTS
        .may_load(deps.storage, agent)?
        .ok_or_else(|| ContractError::UnknownAgent {
            agent: agent.to_string(),
        })
}

/// Records one accepted payment. Updates `rec`/`acct` in memory; the caller saves them.
#[allow(clippy::too_many_arguments)]
fn record(
    storage: &mut dyn Storage,
    env: &Env,
    agent: &Addr,
    rec: &mut AgentRec,
    acct: &mut OwnerAcct,
    vendor: &Addr,
    amount: Uint128,
    shown: &str,
    key: &str,
    memo: Option<String>,
    d: &Decision,
) -> StdResult<Payment> {
    // Pending payments count against the 24h limit too, so an agent cannot flood
    // the owner with approval requests.
    add_spend(rec, env, amount);
    if d.needs_approval {
        acct.reserved += amount;
        acct.pending += 1;
    } else {
        acct.balance -= amount;
    }
    let id = NEXT_ID.load(storage)?;
    NEXT_ID.save(storage, &(id + 1))?;
    let now = env.block.time.seconds();
    let p = Payment {
        id,
        agent: agent.clone(),
        owner: rec.owner.clone(),
        owner_did: d.owner_did.clone(),
        vendor: vendor.clone(),
        vendor_did: d.vendor_did.clone(),
        amount,
        invoice: shown.to_string(),
        invoice_key: key.to_string(),
        memo,
        status: if d.needs_approval {
            PayStatus::Pending
        } else {
            PayStatus::Paid
        },
        created_at: now,
        settled_at: if d.needs_approval { None } else { Some(now) },
    };
    INVOICES.save(storage, (&rec.owner, vendor, key), &id)?;
    PAYMENTS.save(storage, id, &p)?;
    AGENT_PAYMENTS.save(storage, (agent, id), &())?;
    OWNER_PAYMENTS.save(storage, (&rec.owner, id), &())?;
    if d.needs_approval {
        OWNER_PENDING.save(storage, (&rec.owner, id), &())?;
    }
    Ok(p)
}

#[allow(clippy::too_many_arguments)]
fn pay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vendor: String,
    amount: Uint128,
    invoice: String,
    memo: Option<String>,
) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let agent = info.sender;
    let vendor = deps.api.addr_validate(&vendor)?;
    let (shown, key) = invoice_key(&invoice)?;
    let memo = memo.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    let mut rec = load_agent(deps.as_ref(), &agent)?;
    let mut acct = OWNERS.load(deps.storage, &rec.owner)?;
    let d = evaluate(
        deps.as_ref(),
        &env,
        &cfg,
        &agent,
        &rec,
        &acct,
        &vendor,
        amount,
        &key,
        &shown,
        memo.as_deref(),
        &[],
    )?;
    let p = record(
        deps.storage, &env, &agent, &mut rec, &mut acct, &vendor, amount, &shown, &key, memo, &d,
    )?;
    AGENTS.save(deps.storage, &agent, &rec)?;
    OWNERS.save(deps.storage, &rec.owner, &acct)?;

    let mut res = Response::new()
        .add_attribute("action", "pay")
        .add_attribute("payment_id", p.id.to_string())
        .add_attribute("agent", &agent)
        .add_attribute("owner", &rec.owner)
        .add_attribute("owner_did", &d.owner_did)
        .add_attribute("vendor", &vendor)
        .add_attribute("amount", amount)
        .add_attribute("invoice", &shown)
        .add_attribute("status", status_str(&p.status))
        .add_event(payment_event("created", &p));
    if let Some(vd) = &d.vendor_did {
        res = res.add_attribute("vendor_did", vd);
    }
    if !d.needs_approval {
        res = res.add_message(BankMsg::Send {
            to_address: vendor.to_string(),
            amount: coins(amount.u128(), cfg.denom),
        });
    }
    Ok(res)
}

fn pay_batch(deps: DepsMut, env: Env, info: MessageInfo, items: Vec<PayItem>) -> Res {
    if items.is_empty() || items.len() > MAX_BATCH {
        return Err(invalid("batch must hold 1..20 payments"));
    }
    let cfg = CONFIG.load(deps.storage)?;
    let agent = info.sender;
    let mut rec = load_agent(deps.as_ref(), &agent)?;
    let mut acct = OWNERS.load(deps.storage, &rec.owner)?;
    let mut in_flight: Vec<(Addr, String)> = vec![];
    let mut outputs: Vec<Output> = vec![];
    let mut total = Uint128::zero();
    let mut ids = vec![];
    let mut events = vec![];
    for it in items {
        let vendor = deps.api.addr_validate(&it.vendor)?;
        let (shown, key) = invoice_key(&it.invoice)?;
        let d = evaluate(
            deps.as_ref(),
            &env,
            &cfg,
            &agent,
            &rec,
            &acct,
            &vendor,
            it.amount,
            &key,
            &shown,
            None,
            &in_flight,
        )?;
        if d.needs_approval {
            return Err(invalid(
                "batch payments must be under approve_above; send that one with Pay",
            ));
        }
        let p = record(
            deps.storage, &env, &agent, &mut rec, &mut acct, &vendor, it.amount, &shown, &key,
            None, &d,
        )?;
        ids.push(p.id.to_string());
        events.push(payment_event("created", &p));
        in_flight.push((vendor.clone(), key));
        total += it.amount;
        outputs.push(Output {
            address: vendor.to_string(),
            amount: coins(it.amount.u128(), &cfg.denom),
        });
    }
    AGENTS.save(deps.storage, &agent, &rec)?;
    OWNERS.save(deps.storage, &rec.owner, &acct)?;
    Ok(Response::new()
        .add_message(CosmosMsg::Custom(MeMsg::MultiSend(MultiSend {
            amount: coins(total.u128(), &cfg.denom),
            output: outputs,
        })))
        .add_attribute("action", "pay_batch")
        .add_attribute("agent", &agent)
        .add_attribute("owner", &rec.owner)
        .add_attribute("payment_ids", ids.join(","))
        .add_attribute("total", total)
        .add_events(events))
}

fn settle(deps: DepsMut, env: Env, info: MessageInfo, id: u64, approve: bool) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let mut p = PAYMENTS
        .may_load(deps.storage, id)?
        .ok_or_else(|| invalid("no such payment"))?;
    if p.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    if p.status != PayStatus::Pending {
        return Err(ContractError::NotPending { id });
    }
    let mut acct = OWNERS.load(deps.storage, &p.owner)?;
    let mut res = Response::new()
        .add_attribute("action", if approve { "approve" } else { "reject" })
        .add_attribute("payment_id", id.to_string())
        .add_attribute("owner", &p.owner)
        .add_attribute("vendor", &p.vendor)
        .add_attribute("amount", p.amount);
    if approve {
        if env.block.time.seconds() > p.created_at + PENDING_TTL {
            return Err(ContractError::PendingExpired { id });
        }
        // Approving moves money, so the approver must still hold a live ME ID. Rejecting stays open:
        // a suspended owner can always stop a payment, never push one out.
        verified(deps.as_ref(), &cfg, &p.owner)?;
        // The vendor must still be one this leash may pay (a "verified vendor" may have lost its ME ID).
        let rec = load_agent(deps.as_ref(), &p.agent)?;
        let vid = did::lookup(deps.as_ref(), p.vendor.as_str());
        if !vendor_allowed(&cfg, &rec.policy, &p.vendor, vid.as_ref()) {
            return Err(ContractError::VendorNotAllowed {
                vendor: p.vendor.to_string(),
            });
        }
        close_pending(deps.storage, &mut acct, &mut p, &env, true)?;
        res = res.add_message(BankMsg::Send {
            to_address: p.vendor.to_string(),
            amount: coins(p.amount.u128(), cfg.denom),
        });
    } else {
        close_pending(deps.storage, &mut acct, &mut p, &env, false)?;
    }
    OWNERS.save(deps.storage, &p.owner, &acct)?;
    Ok(res.add_event(payment_event(if approve { "approved" } else { "rejected" }, &p)))
}

// ---------------- queries ----------------

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

fn lim(l: Option<u32>) -> usize {
    l.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize
}

pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Version {} => to_json_binary(&VERSION.load(deps.storage)?),
        QueryMsg::Whois { agent } => to_json_binary(&whois(deps, &env, agent)?),
        QueryMsg::Owner { owner } => {
            let owner = deps.api.addr_validate(&owner)?;
            let acct = OWNERS.may_load(deps.storage, &owner)?.unwrap_or_default();
            let agents = OWNER_AGENTS
                .prefix(&owner)
                .keys(deps.storage, None, None, Order::Ascending)
                .take(MAX_AGENTS_PER_OWNER)
                .map(|k| k.map(|a| a.to_string()))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&OwnerResponse {
                me_id: did::lookup(deps, owner.as_str()),
                owner: owner.to_string(),
                balance: acct.balance,
                reserved: acct.reserved,
                pending_count: acct.pending,
                agents,
            })
        }
        QueryMsg::Offers { agent } => {
            let agent = deps.api.addr_validate(&agent)?;
            let offers = OFFERS
                .prefix(&agent)
                .range(deps.storage, None, None, Order::Ascending)
                .take(MAX_LIMIT as usize)
                .map(|r| r.map(|(_, o)| o))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&OffersResponse { offers })
        }
        QueryMsg::CheckPay {
            agent,
            vendor,
            amount,
            invoice,
            memo,
        } => to_json_binary(&check_pay(deps, &env, agent, vendor, amount, invoice, memo)),
        QueryMsg::Payment { id } => to_json_binary(
            &PAYMENTS
                .may_load(deps.storage, id)?
                .ok_or_else(|| StdError::generic_err(format!("no payment #{id}")))?,
        ),
        QueryMsg::Payments { start_after, limit } => {
            let payments = PAYMENTS
                .range(deps.storage, start_after.map(Bound::exclusive), None, Order::Ascending)
                .take(lim(limit))
                .map(|r| r.map(|(_, p)| p))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&PaymentsResponse { payments })
        }
        QueryMsg::PaymentsByAgent {
            agent,
            start_before,
            limit,
        } => {
            let agent = deps.api.addr_validate(&agent)?;
            let ids = AGENT_PAYMENTS
                .prefix(&agent)
                .keys(deps.storage, None, start_before.map(Bound::exclusive), Order::Descending)
                .take(lim(limit))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&load_payments(deps, ids)?)
        }
        QueryMsg::PaymentsByOwner {
            owner,
            start_before,
            limit,
        } => {
            let owner = deps.api.addr_validate(&owner)?;
            let ids = OWNER_PAYMENTS
                .prefix(&owner)
                .keys(deps.storage, None, start_before.map(Bound::exclusive), Order::Descending)
                .take(lim(limit))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&load_payments(deps, ids)?)
        }
        QueryMsg::Pending {
            owner,
            start_after,
            limit,
        } => {
            let owner = deps.api.addr_validate(&owner)?;
            let ids = OWNER_PENDING
                .prefix(&owner)
                .keys(deps.storage, start_after.map(Bound::exclusive), None, Order::Ascending)
                .take(lim(limit))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&load_payments(deps, ids)?)
        }
        QueryMsg::Invoice {
            owner,
            vendor,
            invoice,
        } => {
            let owner = deps.api.addr_validate(&owner)?;
            let vendor = deps.api.addr_validate(&vendor)?;
            let found = match invoice_key(&invoice) {
                Ok((_, key)) => match INVOICES.may_load(deps.storage, (&owner, &vendor, key.as_str()))? {
                    Some(id) => PAYMENTS.may_load(deps.storage, id)?,
                    None => None,
                },
                Err(_) => None,
            };
            to_json_binary(&found)
        }
        QueryMsg::MeId { address } => to_json_binary(&did::lookup(deps, &address)),
    }
}

fn load_payments(deps: Deps, ids: Vec<u64>) -> StdResult<PaymentsResponse> {
    let payments = ids
        .into_iter()
        .map(|id| PAYMENTS.load(deps.storage, id))
        .collect::<StdResult<Vec<_>>>()?;
    Ok(PaymentsResponse { payments })
}

fn whois(deps: Deps, env: &Env, agent: String) -> StdResult<WhoisResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let agent_addr = deps.api.addr_validate(&agent)?;
    let rec = AGENTS
        .may_load(deps.storage, &agent_addr)?
        .ok_or_else(|| StdError::generic_err(format!("not registered: {agent} is not a leashed agent")))?;
    let me = did::lookup(deps, rec.owner.as_str());
    let id_check = verified(deps, &cfg, &rec.owner);
    let spent = spent_24h(&rec, env);
    let remaining = rec.policy.daily_limit.saturating_sub(spent);
    let acct = OWNERS.may_load(deps.storage, &rec.owner)?.unwrap_or_default();
    let available = acct.balance - acct.reserved;
    let expired = env.block.time.seconds() >= rec.policy.expires_at;
    let blocked_by = if rec.revoked {
        Some("revoked by the owner".to_string())
    } else if expired {
        Some("leash expired".to_string())
    } else if let Err(e) = &id_check {
        Some(e.to_string())
    } else if available.is_zero() {
        Some("owner budget is empty".to_string())
    } else if remaining.is_zero() {
        Some("24h limit used up".to_string())
    } else {
        None
    };
    Ok(WhoisResponse {
        agent,
        owner: rec.owner.to_string(),
        owner_me_id: me,
        verified: id_check.is_ok(),
        min_kyc_level: cfg.min_kyc_level,
        spent_24h: spent,
        remaining_24h: remaining,
        revoked: rec.revoked,
        expired,
        owner_balance: available,
        pending_count: acct.pending,
        can_pay_now: blocked_by.is_none(),
        blocked_by,
        policy: rec.policy,
    })
}

fn check_pay(
    deps: Deps,
    env: &Env,
    agent: String,
    vendor: String,
    amount: Uint128,
    invoice: String,
    memo: Option<String>,
) -> CheckPayResponse {
    let run = || -> Result<Decision, ContractError> {
        let cfg = CONFIG.load(deps.storage)?;
        let agent = deps.api.addr_validate(&agent)?;
        let vendor = deps.api.addr_validate(&vendor)?;
        let (shown, key) = invoice_key(&invoice)?;
        let memo = memo.as_deref().map(str::trim).filter(|m| !m.is_empty());
        let rec = load_agent(deps, &agent)?;
        let acct = OWNERS.may_load(deps.storage, &rec.owner)?.unwrap_or_default();
        evaluate(
            deps, env, &cfg, &agent, &rec, &acct, &vendor, amount, &key, &shown, memo, &[],
        )
    };
    match run() {
        Ok(d) => CheckPayResponse {
            ok: true,
            needs_approval: d.needs_approval,
            reason: None,
        },
        Err(e) => CheckPayResponse {
            ok: false,
            needs_approval: false,
            reason: Some(e.to_string()),
        },
    }
}
