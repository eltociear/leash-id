use cosmwasm_std::{
    coins, to_json_binary, Addr, BankMsg, Binary, CosmosMsg, Deps, DepsMut, Env, Event,
    MessageInfo, Order, Response, StdError, StdResult, Storage, Uint128,
};
use cw_storage_plus::Bound;

use crate::did::{self, MeId};
use crate::me;
use crate::error::ContractError;
use crate::msg::{
    Bond, CheckPayResponse, ExecuteMsg, IdentityResponse, InstantiateMsg, MeMsg, MigrateMsg, MultiSend, OffersResponse,
    Output, OwnerResponse, PayItem, PaymentsResponse, PolicyMsg, QueryMsg, WhoisResponse,
};
use crate::state::{
    AgentRec, Approvers, Change, ChangeKind, Config, ContractVersion, Offer, OwnerAcct, PayStatus, Payment, Policy, AGENTS, APPROVERS, APPROVER_OF, BONDS, SUB_ACCOUNTS,
    AGENT_PAYMENTS, CONFIG, INVOICES, NEXT_ID, OFFERS, OWNERS, OWNER_AGENTS, OWNER_PAYMENTS,
    OWNER_PENDING, PAYMENTS, VERSION, CHANGES,
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
/// Spend is kept in hourly buckets. A bucket counts for 25 hours, so a payment made at 10:59 stops counting at
/// 11:00 the next day (24h01m later), never earlier: whatever 24 hours you pick, at most `daily_limit` left in them.
/// (With 24 buckets a full limit at 10:59 freed at 10:00, and twice the limit could go out in 23h01m.)
const WINDOW_HOURS: u64 = 25;
const MAX_INVOICE: usize = 64;
const MAX_MEMO: usize = 256;
const MAX_BONDS: usize = 5;
const MAX_APPROVERS: usize = 5;
/// A reviewed change must be applied within this long after its delay ends, or proposed again.
const APPLY_WINDOW: u64 = 7 * 86400;
/// How long a change that loosens control waits while the owner needs two or more signatures.
pub const CHANGE_DELAY: u64 = 86400;

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
        ExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, amount),
        ExecuteMsg::RegisterAgent { agent, policy } => offer(deps, env, info, agent, policy),
        ExecuteMsg::CancelOffer { agent } => cancel_offer(deps, info, agent),
        ExecuteMsg::AcceptLeash { owner } => accept(deps, env, info, owner),
        ExecuteMsg::LeaveLeash {} => leave(deps, env, info),
        ExecuteMsg::UpdatePolicy { agent, policy } => update_policy(deps, env, info, agent, policy),
        ExecuteMsg::RevokeAgent { agent } => revoke(deps, env, info, agent),
        ExecuteMsg::ResumeAgent { agent } => resume(deps, env, info, agent),
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
        ExecuteMsg::RegisterSubAccount { sub, main } => register_sub(deps, sub, main),
        ExecuteMsg::SetBond { deposit_ids } => set_bond(deps, env, info, deposit_ids),
        ExecuteMsg::SetApprovers { approvers, threshold } => set_approvers(deps, env, info, approvers, threshold),
        ExecuteMsg::ApplyChange { owner, key } => apply_change(deps, env, owner, key),
        ExecuteMsg::CancelChange { owner, key } => cancel_change(deps, info, owner, key),
        ExecuteMsg::ApproveChange { owner, key } => approve_change(deps, env, info, owner, key),
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

/// The ME ID behind `addr`: its own, or, for a registered EVM-form sub-account, its main account's, as long
/// as x/did still lists `addr` as that ME ID's sub-account. Returns the ME ID and the main account it came via.
pub fn resolve(deps: Deps, addr: &str) -> Option<(MeId, Option<String>)> {
    if let Some(id) = did::lookup(deps, addr) {
        return Some((id, None));
    }
    let a = Addr::unchecked(addr);
    let main = SUB_ACCOUNTS.may_load(deps.storage, &a).ok()??;
    let id = did::lookup(deps, main.as_str())?;
    // x/did keeps sub_account as the user typed it; bech32 is case-insensitive.
    id.sub_account.eq_ignore_ascii_case(addr).then(|| (id, Some(main.to_string())))
}

fn me_id_of(deps: Deps, addr: &str) -> Option<MeId> {
    resolve(deps, addr).map(|(id, _)| id)
}

/// The address must hold a live ME ID that is active and at the required KYC level.
fn verified(deps: Deps, cfg: &Config, addr: &Addr) -> Result<MeId, ContractError> {
    let id = me_id_of(deps, addr.as_str()).ok_or_else(|| ContractError::NoMeId {
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

/// Spend that still counts against the daily limit: everything from the last 24 hours (and up to one more hour).
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

fn withdraw(deps: DepsMut, env: Env, info: MessageInfo, amount: Uint128) -> Res {
    // Only an owner who deposited can withdraw. An agent (or anyone else) gets a plain refusal.
    let acct = OWNERS
        .may_load(deps.storage, &info.sender)?
        .ok_or(ContractError::Unauthorized {})?;
    let available = acct.balance - acct.reserved;
    if amount.is_zero() || amount > available {
        return Err(ContractError::InsufficientBudget { available, amount });
    }
    if guarded(deps.storage, &info.sender)? {
        return delay(deps.storage, &env, &info.sender, "withdraw", ChangeKind::Withdraw { amount }, "withdraw");
    }
    pay_out(deps, &info.sender, amount)
}

fn pay_out(deps: DepsMut, owner: &Addr, amount: Uint128) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let mut acct = OWNERS.load(deps.storage, owner)?;
    let available = acct.balance - acct.reserved;
    if amount.is_zero() || amount > available {
        return Err(ContractError::InsufficientBudget { available, amount });
    }
    acct.balance -= amount;
    OWNERS.save(deps.storage, owner, &acct)?;
    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: owner.to_string(),
            amount: coins(amount.u128(), cfg.denom),
        })
        .add_attribute("action", "withdraw")
        .add_attribute("owner", owner)
        .add_attribute("amount", amount)
        .add_attribute("balance", acct.balance))
}

// ---------------- leash management ----------------

fn build_policy(deps: Deps, env: &Env, owner: &Addr, agent: &Addr, p: PolicyMsg) -> Result<Policy, ContractError> {
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
        vendor_sane(deps, env, owner, agent, &a)?;
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
/// The same holds for a registered EVM sub-account: it is the same key as its main account.
fn vendor_sane(deps: Deps, env: &Env, owner: &Addr, agent: &Addr, vendor: &Addr) -> Result<(), ContractError> {
    let main = SUB_ACCOUNTS.may_load(deps.storage, vendor)?;
    for who in std::iter::once(vendor).chain(main.as_ref()) {
        let why = if who == agent {
            "an agent cannot pay itself"
        } else if who == owner {
            // the budget goes back to the owner only by `withdraw`, which co-signing can hold for review
            "an agent cannot pay its own owner"
        } else if *who == env.contract.address {
            "the contract cannot be a vendor"
        } else if AGENTS.has(deps.storage, who) {
            "a leashed agent cannot be a vendor"
        } else {
            continue;
        };
        return Err(ContractError::SelfDealing { why: why.into() });
    }
    // nor at any other address of the owner's ME ID (e.g. an EVM sub-account x/did binds to it)
    if let (Some(v), Some(o)) = (me_id_of(deps, vendor.as_str()), me_id_of(deps, owner.as_str())) {
        if v.did == o.did {
            return Err(ContractError::SelfDealing { why: "an agent cannot pay its own owner".into() });
        }
    }
    Ok(())
}

/// The party an invoice number is filed under. A registered EVM sub-account files under its main account, so
/// one vendor can't be paid the same invoice once at each of its addresses.
fn invoice_party(storage: &dyn Storage, vendor: &Addr) -> Addr {
    SUB_ACCOUNTS.may_load(storage, vendor).ok().flatten().unwrap_or_else(|| vendor.clone())
}

/// Every address an invoice to `vendor` may be filed under: the vendor itself, its main account (if it is a
/// registered sub-account), and that main account's registered sub-account. A payment made at one address before
/// the link was recorded stays filed under that address, so all of them are checked.
fn invoice_parties(deps: Deps, vendor: &Addr) -> Vec<Addr> {
    let party = invoice_party(deps.storage, vendor);
    let mut out = vec![vendor.clone()];
    if party != *vendor {
        out.push(party.clone());
    }
    if let Some(id) = did::lookup(deps, party.as_str()) {
        if let Ok(sub) = deps.api.addr_validate(&id.sub_account.to_ascii_lowercase()) {
            if SUB_ACCOUNTS.may_load(deps.storage, &sub).ok().flatten().as_ref() == Some(&party) && !out.contains(&sub) {
                out.push(sub);
            }
        }
    }
    out
}

/// The payment an invoice to this vendor (at any of its addresses) was handled by.
fn invoice_lookup(deps: Deps, owner: &Addr, vendor: &Addr, key: &str) -> StdResult<Option<u64>> {
    for v in invoice_parties(deps, vendor) {
        if let Some(id) = INVOICES.may_load(deps.storage, (owner, &v, key))? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Frees the invoice number held by payment `p`, exactly where `record` filed it.
fn invoice_forget(storage: &mut dyn Storage, p: &Payment) -> StdResult<()> {
    let party = p.invoice_party.as_ref().unwrap_or(&p.vendor);
    if INVOICES.may_load(storage, (&p.owner, party, p.invoice_key.as_str()))? == Some(p.id) {
        INVOICES.remove(storage, (&p.owner, party, p.invoice_key.as_str()));
    }
    Ok(())
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
    let policy = build_policy(deps.as_ref(), &env, &info.sender, &agent, p)?;
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
    if APPROVER_OF.prefix(&agent).keys(deps.storage, None, None, Order::Ascending).next().is_some() {
        return Err(invalid("a co-approver key cannot be leashed as an agent"));
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
    let active_from = if guarded(deps.storage, &owner)? { env.block.time.seconds() + CHANGE_DELAY } else { 0 };
    AGENTS.save(
        deps.storage,
        &agent,
        &AgentRec {
            owner: owner.clone(),
            policy: off.policy,
            spend: vec![],
            revoked: false,
            active_from,
        },
    )?;
    OFFERS.remove(deps.storage, (&agent, &owner));
    OWNER_AGENTS.save(deps.storage, (&owner, &agent), &())?;
    // a new leash starts clean: nothing queued against an earlier leash of this key carries over
    drop_changes_of(deps.storage, &owner, &agent);
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
    let new = build_policy(deps.as_ref(), &env, &rec.owner, &agent, p)?;
    if guarded(deps.storage, &rec.owner)? && loosens(&rec.policy, &new) {
        return delay(deps.storage, &env, &rec.owner, &format!("policy:{agent}"), ChangeKind::Policy(new), "update_policy");
    }
    // Only the policy changes: a revoked agent stays revoked, and the 24h spend window is kept. A looser
    // policy queued earlier is dropped: applying it later would quietly undo this (e.g. re-add a vendor just removed).
    rec.policy = new;
    CHANGES.remove(deps.storage, (&rec.owner, &format!("policy:{agent}")));
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
        invoice_forget(storage, p)?;
        p.status = PayStatus::Rejected;
    }
    p.settled_at = Some(env.block.time.seconds());
    PAYMENTS.save(storage, p.id, p)
}

fn revoke(deps: DepsMut, env: Env, info: MessageInfo, agent: String) -> Res {
    // Stopping money never waits for anyone: the owner, or any one of the owner's co-approvers, can revoke.
    let agent = deps.api.addr_validate(&agent)?;
    let mut rec = load_agent(deps.as_ref(), &agent)?;
    if rec.owner != info.sender && !approvers_of(deps.storage, &rec.owner)?.list.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }
    rec.revoked = true;
    AGENTS.save(deps.storage, &agent, &rec)?;
    // A revoke also drops whatever loosening of this leash was waiting.
    drop_changes_of(deps.storage, &rec.owner, &agent);
    let events = reject_pending_of(deps.storage, &env, &rec.owner, &agent)?;
    Ok(Response::new()
        .add_attribute("action", "revoke_agent")
        .add_attribute("owner", &rec.owner)
        .add_attribute("by", info.sender)
        .add_attribute("agent", agent)
        .add_attribute("rejected_pending", events.len().to_string())
        .add_events(events))
}

fn resume(deps: DepsMut, env: Env, info: MessageInfo, agent: String) -> Res {
    let (agent, mut rec) = owned(deps.as_ref(), &info, &agent)?;
    if !rec.revoked {
        return Err(invalid("agent is not revoked"));
    }
    if guarded(deps.storage, &rec.owner)? {
        return delay(deps.storage, &env, &rec.owner, &format!("resume:{agent}"), ChangeKind::Resume {}, "resume_agent");
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
    drop_changes_of(deps.storage, &rec.owner, &agent);
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
    let id = invoice_lookup(deps.as_ref(), &info.sender, &vendor, key.as_str())?
        .ok_or_else(|| invalid("no such invoice for this owner and vendor"))?;
    let p = PAYMENTS.load(deps.storage, id)?;
    if p.status == PayStatus::Pending {
        return Err(invalid("that invoice is pending; reject it instead"));
    }
    invoice_forget(deps.storage, &p)?;
    Ok(Response::new()
        .add_attribute("action", "release_invoice")
        .add_attribute("owner", info.sender)
        .add_attribute("vendor", vendor)
        .add_attribute("invoice_key", key)
        .add_attribute("payment_id", id.to_string()))
}

// ---------------- ME-native extras: sub-accounts and bonds ----------------

fn register_sub(deps: DepsMut, sub: String, main: String) -> Res {
    let sub = deps.api.addr_validate(&sub)?;
    let main = deps.api.addr_validate(&main)?;
    let id = did::lookup(deps.as_ref(), main.as_str()).ok_or_else(|| ContractError::NoMeId {
        addr: main.to_string(),
    })?;
    if !id.sub_account.eq_ignore_ascii_case(sub.as_str()) {
        return Err(ContractError::NotSubAccount {
            sub: sub.to_string(),
            main: main.to_string(),
        });
    }
    // A link is permanent here. Invoices paid through the sub-account are filed under this main account, so
    // re-pointing it would let them be paid again; if x/did moves the sub-account, it simply stops resolving.
    if let Some(old) = SUB_ACCOUNTS.may_load(deps.storage, &sub)? {
        if old != main {
            return Err(invalid(&format!("{sub} is already linked to {old}; a sub-account link can't be moved")));
        }
    }
    SUB_ACCOUNTS.save(deps.storage, &sub, &main)?;
    Ok(Response::new()
        .add_attribute("action", "register_sub_account")
        .add_attribute("sub", sub)
        .add_attribute("main", main)
        .add_attribute("did", id.did))
}

/// A deposit counts toward the bond only while it is locked, in the leash denom, and held by the ME ID's
/// main account. Deposits that were withdrawn simply stop answering and drop out.
fn bond_deposit(deps: Deps, env: &Env, cfg: &Config, me: &MeId, id: u64) -> Result<me::Deposit, String> {
    let d = me::fixed_deposit(deps, &me.address, id).ok_or("not found on ME Hub (withdrawn, or not this ME ID's)")?;
    if d.id != id {
        return Err("ME Hub answered with a different deposit".into());
    }
    if d.account != me.address {
        return Err("belongs to another account".into());
    }
    if d.denom != cfg.denom {
        return Err(format!("is in {}, not {}", d.denom, cfg.denom));
    }
    if d.end_time <= env.block.time.seconds() {
        return Err("has already unlocked".into());
    }
    Ok(d)
}

fn bond_of(deps: Deps, env: &Env, cfg: &Config, owner: &Addr, me: &MeId) -> Option<Bond> {
    let ids = BONDS.may_load(deps.storage, owner).ok()??;
    let deposits: Vec<me::Deposit> = ids.iter().filter_map(|i| bond_deposit(deps, env, cfg, me, *i).ok()).collect();
    if deposits.is_empty() {
        return None;
    }
    Some(Bond {
        amount: deposits.iter().fold(Uint128::zero(), |a, d| a.saturating_add(d.principal)),
        earliest_unlock: deposits.iter().map(|d| d.end_time).min().unwrap_or(0),
        deposits,
    })
}

fn set_bond(deps: DepsMut, env: Env, info: MessageInfo, ids: Vec<u64>) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let me = verified(deps.as_ref(), &cfg, &info.sender)?;
    if ids.len() > MAX_BONDS {
        return Err(invalid("at most 5 fixed deposits"));
    }
    let mut seen = vec![];
    let mut total = Uint128::zero();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        let d = bond_deposit(deps.as_ref(), &env, &cfg, &me, id).map_err(|why| ContractError::BadBond { id, why })?;
        total += d.principal;
        seen.push(id);
    }
    if seen.is_empty() {
        BONDS.remove(deps.storage, &info.sender);
    } else {
        BONDS.save(deps.storage, &info.sender, &seen)?;
    }
    Ok(Response::new()
        .add_attribute("action", "set_bond")
        .add_attribute("owner", info.sender)
        .add_attribute("owner_did", me.did)
        .add_attribute("deposits", seen.len().to_string())
        .add_attribute("bond", total))
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
    if env.block.time.seconds() < rec.active_from {
        return Err(ContractError::NotActiveYet { at: rec.active_from });
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
    let party = invoice_party(deps.storage, vendor);
    if in_flight.iter().any(|(v, k)| *v == party && k == invoice_key) {
        return Err(ContractError::DuplicateInBatch {
            invoice: shown.to_string(),
        });
    }
    if let Some(id) = invoice_lookup(deps, &rec.owner, vendor, invoice_key)? {
        return Err(ContractError::DuplicateInvoice {
            invoice: shown.to_string(),
            id,
        });
    }
    vendor_sane(deps, env, &rec.owner, agent, vendor)?;
    let vendor_id = me_id_of(deps, vendor.as_str());
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
        invoice_party: Some(invoice_party(storage, vendor)),
        memo,
        status: if d.needs_approval {
            PayStatus::Pending
        } else {
            PayStatus::Paid
        },
        approved_by: vec![],
        created_at: now,
        settled_at: if d.needs_approval { None } else { Some(now) },
    };
    let party = p.invoice_party.clone().unwrap_or_else(|| vendor.clone());
    INVOICES.save(storage, (&rec.owner, &party, key), &id)?;
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
        in_flight.push((invoice_party(deps.storage, &vendor), key));
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

fn approvers_of(storage: &dyn Storage, owner: &Addr) -> StdResult<Approvers> {
    Ok(APPROVERS.may_load(storage, owner)?.unwrap_or(Approvers { list: vec![], threshold: 1 }))
}

fn set_approvers(deps: DepsMut, env: Env, info: MessageInfo, list: Vec<String>, threshold: u8) -> Res {
    let owner = info.sender;
    let new = checked_approvers(deps.as_ref(), &owner, list.iter().map(|a| deps.api.addr_validate(a)).collect::<StdResult<_>>()?, threshold)?;
    let old = approvers_of(deps.storage, &owner)?;
    // While two or more signatures are needed, a new signer set must still need the old one's agreement:
    // nobody removed, and the threshold up by at least as many signers as are added (so any signatures that
    // meet the new rule include enough old signers to meet the old one). Anything else, e.g. fewer signatures,
    // or a friendly co-approver added at the same threshold, waits, and any current co-approver can cancel it.
    let added = new.list.iter().filter(|a| !old.list.contains(a)).count();
    let removed = old.list.iter().any(|a| !new.list.contains(a));
    if old.threshold >= 2 && (removed || (new.threshold as usize) < old.threshold as usize + added) {
        return delay(deps.storage, &env, &owner, "approvers", ChangeKind::Approvers(new), "set_approvers");
    }
    store_approvers(deps.storage, &owner, &old, &new)?;
    Ok(Response::new()
        .add_attribute("action", "set_approvers")
        .add_attribute("owner", owner)
        .add_attribute("approvers", new.list.len().to_string())
        .add_attribute("threshold", new.threshold.to_string()))
}

fn store_approvers(storage: &mut dyn Storage, owner: &Addr, old: &Approvers, new: &Approvers) -> StdResult<()> {
    for a in &old.list {
        APPROVER_OF.remove(storage, (a, owner));
    }
    for a in &new.list {
        APPROVER_OF.save(storage, (a, owner), &())?;
    }
    if new.list.is_empty() && new.threshold == 1 {
        APPROVERS.remove(storage, owner);
    } else {
        APPROVERS.save(storage, owner, new)?;
    }
    Ok(())
}

fn checked_approvers(deps: Deps, owner: &Addr, list: Vec<Addr>, threshold: u8) -> Result<Approvers, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    // Two-person approval means two people: every signer must hold a different ME ID. An owner's own EVM
    // sub-account resolves to the owner's ME ID, so it can't be the owner's co-approver.
    let mut dids = vec![verified(deps, &cfg, owner)?.did];
    if list.len() > MAX_APPROVERS {
        return Err(invalid("at most 5 co-approvers"));
    }
    let mut addrs: Vec<Addr> = vec![];
    for a in list {
        if a == *owner || addrs.contains(&a) {
            continue;
        }
        if AGENTS.has(deps.storage, &a) {
            return Err(invalid("an agent key cannot be a co-approver"));
        }
        // Each co-approver is a verified human too, checked now and again whenever they sign.
        let did = verified(deps, &cfg, &a)?.did;
        if dids.contains(&did) {
            return Err(invalid(&format!(
                "{a} holds ME ID {did}, the same person as you or another co-approver; each signer needs their own ME ID"
            )));
        }
        dids.push(did);
        addrs.push(a);
    }
    if threshold == 0 || threshold as usize > addrs.len() + 1 {
        return Err(invalid("threshold must be between 1 and the number of signers (co-approvers + you)"));
    }
    Ok(Approvers { list: addrs, threshold })
}

// ---------------- the review delay for loosening changes ----------------

/// Does this owner need two or more signatures? Then changes that loosen control wait `CHANGE_DELAY`.
fn guarded(storage: &dyn Storage, owner: &Addr) -> StdResult<bool> {
    Ok(approvers_of(storage, owner)?.threshold >= 2)
}

/// Would `new` let the agent move money that `old` wouldn't? Any one of these makes the whole change wait.
fn loosens(old: &Policy, new: &Policy) -> bool {
    let hold_loosened = match (old.approve_above, new.approve_above) {
        (Some(a), Some(b)) => b > a,
        (Some(_), None) => true,
        (None, _) => false,
    };
    new.per_tx_limit > old.per_tx_limit
        || new.daily_limit > old.daily_limit
        || hold_loosened
        || new.vendors.iter().any(|v| !old.vendors.contains(v))
        || (new.any_verified_vendor && !old.any_verified_vendor)
        || new.expires_at > old.expires_at
}

fn delay(storage: &mut dyn Storage, env: &Env, owner: &Addr, key: &str, kind: ChangeKind, action: &str) -> Res {
    let now = env.block.time.seconds();
    let at = now + CHANGE_DELAY;
    // a new proposal under the same key replaces the old one, and its 24 h start again
    CHANGES.save(storage, (owner, key), &Change { kind, proposed_at: now, effective_at: at, approved_by: vec![] })?;
    Ok(Response::new()
        .add_attribute("action", action)
        .add_attribute("owner", owner)
        .add_attribute("delayed", key)
        .add_attribute("effective_at", at.to_string()))
}

fn drop_changes_of(storage: &mut dyn Storage, owner: &Addr, agent: &Addr) {
    CHANGES.remove(storage, (owner, &format!("policy:{agent}")));
    CHANGES.remove(storage, (owner, &format!("resume:{agent}")));
}

fn load_change(storage: &dyn Storage, env: &Env, owner: &Addr, key: &str) -> Result<Change, ContractError> {
    let c = CHANGES
        .may_load(storage, (owner, key))?
        .ok_or_else(|| invalid(&format!("no change {key} waiting for {owner}")))?;
    if env.block.time.seconds() > c.effective_at + APPLY_WINDOW {
        return Err(invalid(&format!("change {key} wasn't applied within 7 days of its review; propose it again")));
    }
    Ok(c)
}

/// Anyone may apply a change once its delay is over (and within 7 days after); it is checked again as if made now.
fn apply_change(deps: DepsMut, env: Env, owner: String, key: String) -> Res {
    let owner = deps.api.addr_validate(&owner)?;
    let c = load_change(deps.storage, &env, &owner, &key)?;
    if env.block.time.seconds() < c.effective_at {
        return Err(invalid(&format!("change {key} can be applied from {} (unix s)", c.effective_at)));
    }
    do_apply(deps, env, owner, key, c)
}

/// The signers agree to a waiting change: once as many people (ME IDs) as release a held payment have agreed,
/// the owner's proposal counting as theirs, it applies at once. So one co-approver alone can't hold the owner
/// hostage when others agree, and nobody has to wait the 24 h for a change everyone wants.
fn approve_change(deps: DepsMut, env: Env, info: MessageInfo, owner: String, key: String) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let owner = deps.api.addr_validate(&owner)?;
    let signers = approvers_of(deps.storage, &owner)?;
    if info.sender != owner && !signers.list.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }
    let mut c = load_change(deps.storage, &env, &owner, &key)?;
    verified(deps.as_ref(), &cfg, &info.sender)?;
    if !c.approved_by.contains(&info.sender) {
        c.approved_by.push(info.sender.clone());
    }
    // one per live ME ID, among the current signers (the owner always counts as having proposed it)
    let mut dids: Vec<String> = vec![];
    for a in std::iter::once(&owner).chain(c.approved_by.iter()) {
        if *a != owner && !signers.list.contains(a) {
            continue;
        }
        if let Ok(m) = verified(deps.as_ref(), &cfg, a) {
            if !dids.contains(&m.did) {
                dids.push(m.did);
            }
        }
    }
    if dids.len() < signers.threshold as usize {
        CHANGES.save(deps.storage, (&owner, &key), &c)?;
        return Ok(Response::new()
            .add_attribute("action", "approve_change")
            .add_attribute("owner", &owner)
            .add_attribute("key", &key)
            .add_attribute("approvals", dids.len().to_string())
            .add_attribute("needed", signers.threshold.to_string()));
    }
    Ok(do_apply(deps, env, owner, key, c)?.add_attribute("approvals", dids.len().to_string()))
}

fn do_apply(deps: DepsMut, env: Env, owner: Addr, key: String, c: Change) -> Res {
    CHANGES.remove(deps.storage, (&owner, &key));
    if let ChangeKind::Withdraw { amount } = c.kind {
        // the budget is re-checked now: what's reserved for held payments since then stays put
        return Ok(pay_out(deps, &owner, amount)?.add_attribute("key", key));
    }
    let res = Response::new()
        .add_attribute("action", "apply_change")
        .add_attribute("owner", &owner)
        .add_attribute("key", &key);
    match c.kind {
        ChangeKind::Withdraw { .. } => unreachable!(),
        ChangeKind::Approvers(a) => {
            let new = checked_approvers(deps.as_ref(), &owner, a.list, a.threshold)?;
            let old = approvers_of(deps.storage, &owner)?;
            store_approvers(deps.storage, &owner, &old, &new)?;
            Ok(res.add_attribute("threshold", new.threshold.to_string()))
        }
        ChangeKind::Policy(pol) => {
            let agent = agent_of_key(deps.as_ref(), &key, "policy:")?;
            let mut rec = load_agent(deps.as_ref(), &agent)?;
            if rec.owner != owner {
                return Err(invalid("that agent has another owner now"));
            }
            if env.block.time.seconds() >= pol.expires_at {
                return Err(ContractError::Expired {});
            }
            for v in &pol.vendors {
                vendor_sane(deps.as_ref(), &env, &owner, &agent, v)?;
            }
            rec.policy = pol;
            AGENTS.save(deps.storage, &agent, &rec)?;
            Ok(res.add_attribute("agent", agent))
        }
        ChangeKind::Resume {} => {
            let agent = agent_of_key(deps.as_ref(), &key, "resume:")?;
            let mut rec = load_agent(deps.as_ref(), &agent)?;
            if rec.owner != owner {
                return Err(invalid("that agent has another owner now"));
            }
            rec.revoked = false;
            AGENTS.save(deps.storage, &agent, &rec)?;
            Ok(res.add_attribute("agent", agent))
        }
    }
}

fn agent_of_key(deps: Deps, key: &str, prefix: &str) -> Result<Addr, ContractError> {
    let a = key.strip_prefix(prefix).ok_or_else(|| invalid("bad change key"))?;
    Ok(deps.api.addr_validate(a)?)
}

/// The owner, or any one of the owner's current co-approvers, can cancel a waiting change.
fn cancel_change(deps: DepsMut, info: MessageInfo, owner: String, key: String) -> Res {
    let owner = deps.api.addr_validate(&owner)?;
    if info.sender != owner && !approvers_of(deps.storage, &owner)?.list.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }
    if !CHANGES.has(deps.storage, (&owner, &key)) {
        return Err(invalid(&format!("no change {key} waiting for {owner}")));
    }
    CHANGES.remove(deps.storage, (&owner, &key));
    Ok(Response::new()
        .add_attribute("action", "cancel_change")
        .add_attribute("owner", owner)
        .add_attribute("key", key)
        .add_attribute("by", info.sender))
}

fn settle(deps: DepsMut, env: Env, info: MessageInfo, id: u64, approve: bool) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let mut p = PAYMENTS
        .may_load(deps.storage, id)?
        .ok_or_else(|| invalid("no such payment"))?;
    let signers = approvers_of(deps.storage, &p.owner)?;
    let signer = info.sender;
    if p.owner != signer && !signers.list.contains(&signer) {
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
        .add_attribute("signer", &signer)
        .add_attribute("vendor", &p.vendor)
        .add_attribute("amount", p.amount);
    if !approve {
        // Any one signer can stop money, even with a suspended ME ID; nobody can move it alone.
        close_pending(deps.storage, &mut acct, &mut p, &env, false)?;
        OWNERS.save(deps.storage, &p.owner, &acct)?;
        return Ok(res.add_event(payment_event("rejected", &p)));
    }
    if env.block.time.seconds() > p.created_at + PENDING_TTL {
        return Err(ContractError::PendingExpired { id });
    }
    // Approving moves money, so the signer, and the owner whose budget it is, must hold a live ME ID.
    let signer_did = verified(deps.as_ref(), &cfg, &signer)?.did;
    verified(deps.as_ref(), &cfg, &p.owner)?;
    // Not to themselves, at any of their addresses (an EVM sub-account resolves to its main account's ME ID).
    if signer == p.vendor || me_id_of(deps.as_ref(), p.vendor.as_str()).map_or(false, |v| v.did == signer_did) {
        return Err(ContractError::SelfDealing { why: "a signer cannot approve a payment to themselves".into() });
    }
    // Count only signatures that still stand (still a signer, ME ID still live), one per person (ME ID).
    let standing_dids = |list: &[Addr]| {
        let mut seen: Vec<String> = vec![];
        for a in list {
            if *a != p.owner && !signers.list.contains(a) {
                continue;
            }
            if let Ok(m) = verified(deps.as_ref(), &cfg, a) {
                if !seen.contains(&m.did) {
                    seen.push(m.did);
                }
            }
        }
        seen
    };
    let before = standing_dids(&p.approved_by);
    let standing = if p.approved_by.contains(&signer) || before.contains(&signer_did) {
        // Signed already. If enough signatures stand again (a lapsed co-approver's ME ID came back), any signer
        // may release it now; otherwise a second signature from the same person changes nothing.
        if before.len() < signers.threshold as usize {
            return Err(ContractError::AlreadyApproved { id, have: before.len() as u8, need: signers.threshold });
        }
        before.len()
    } else {
        p.approved_by.push(signer);
        before.len() + 1
    };
    res = res
        .add_attribute("approvals", standing.to_string())
        .add_attribute("needed", signers.threshold.to_string());
    if standing < signers.threshold as usize {
        PAYMENTS.save(deps.storage, id, &p)?;
        return Ok(res.add_event(payment_event("approval", &p)));
    }
    // The vendor must still be one this leash may pay (a "verified vendor" may have lost its ME ID), and still
    // a payee at all: not since leashed as an agent, not the agent's own address. An expired leash pays nothing.
    let rec = load_agent(deps.as_ref(), &p.agent)?;
    if env.block.time.seconds() >= rec.policy.expires_at {
        return Err(ContractError::Expired {});
    }
    vendor_sane(deps.as_ref(), &env, &p.owner, &p.agent, &p.vendor)?;
    let vid = me_id_of(deps.as_ref(), p.vendor.as_str());
    if !vendor_allowed(&cfg, &rec.policy, &p.vendor, vid.as_ref()) {
        return Err(ContractError::VendorNotAllowed {
            vendor: p.vendor.to_string(),
        });
    }
    close_pending(deps.storage, &mut acct, &mut p, &env, true)?;
    OWNERS.save(deps.storage, &p.owner, &acct)?;
    Ok(res
        .add_message(BankMsg::Send {
            to_address: p.vendor.to_string(),
            amount: coins(p.amount.u128(), cfg.denom),
        })
        .add_event(payment_event("approved", &p)))
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
                me_id: me_id_of(deps, owner.as_str()),
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
                Ok((_, key)) => match invoice_lookup(deps, &owner, &vendor, key.as_str())? {
                    Some(id) => PAYMENTS.may_load(deps.storage, id)?,
                    None => None,
                },
                Err(_) => None,
            };
            to_json_binary(&found)
        }
        QueryMsg::MeId { address } => to_json_binary(&me_id_of(deps, &address)),
        QueryMsg::Approvers { owner } => to_json_binary(&approvers_of(deps.storage, &deps.api.addr_validate(&owner)?)?),
        QueryMsg::Changes { owner } => {
            let owner = deps.api.addr_validate(&owner)?;
            let list = CHANGES
                .prefix(&owner)
                .range(deps.storage, None, None, Order::Ascending)
                .take(MAX_LIMIT as usize)
                .collect::<StdResult<Vec<(String, Change)>>>()?;
            to_json_binary(&list)
        }
        QueryMsg::ApproverOf { approver } => {
            let a = deps.api.addr_validate(&approver)?;
            let owners = APPROVER_OF
                .prefix(&a)
                .keys(deps.storage, None, None, Order::Ascending)
                .take(MAX_LIMIT as usize)
                .map(|k| k.map(|o| o.to_string()))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&owners)
        }
        QueryMsg::Identity { address } => {
            let a = deps.api.addr_validate(&address)?;
            let r = resolve(deps, a.as_str());
            let cfg = CONFIG.load(deps.storage)?;
            let bond = r.as_ref().and_then(|(id, _)| bond_of(deps, &env, &cfg, &a, id));
            to_json_binary(&IdentityResponse {
                address,
                via_main: r.as_ref().and_then(|(_, v)| v.clone()),
                me_id: r.map(|(id, _)| id),
                bond,
            })
        }
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
    let me = me_id_of(deps, rec.owner.as_str());
    let owner_bond = me.as_ref().and_then(|id| bond_of(deps, env, &cfg, &rec.owner, id));
    let id_check = verified(deps, &cfg, &rec.owner);
    let spent = spent_24h(&rec, env);
    let remaining = rec.policy.daily_limit.saturating_sub(spent);
    let acct = OWNERS.may_load(deps.storage, &rec.owner)?.unwrap_or_default();
    let available = acct.balance - acct.reserved;
    let expired = env.block.time.seconds() >= rec.policy.expires_at;
    let now = env.block.time.seconds();
    let pending_change = CHANGES.may_load(deps.storage, (&rec.owner, &format!("policy:{agent}")))?
        .or(CHANGES.may_load(deps.storage, (&rec.owner, &format!("resume:{agent}")))?);
    let blocked_by = if rec.revoked {
        Some("revoked by the owner or a co-approver".to_string())
    } else if expired {
        Some("leash expired".to_string())
    } else if now < rec.active_from {
        Some(ContractError::NotActiveYet { at: rec.active_from }.to_string())
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
        owner_bond,
        verified: id_check.is_ok(),
        min_kyc_level: cfg.min_kyc_level,
        spent_24h: spent,
        remaining_24h: remaining,
        revoked: rec.revoked,
        expired,
        owner_balance: available,
        pending_count: acct.pending,
        approvals_needed: approvers_of(deps.storage, &rec.owner)?.threshold,
        can_pay_now: blocked_by.is_none(),
        blocked_by,
        active_from: (now < rec.active_from).then_some(rec.active_from),
        pending_change,
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
