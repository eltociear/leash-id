use cosmwasm_std::{
    coins, to_json_binary, Addr, BankMsg, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Order, Response, StdResult, Uint128,
};
use cw_storage_plus::Bound;

use crate::did::{self, MeId};
use crate::error::ContractError;
use crate::msg::{
    CheckPayResponse, ExecuteMsg, InstantiateMsg, MeMsg, MultiSend, Output, OwnerResponse,
    PayItem, PaymentsResponse, PolicyMsg, QueryMsg, WhoisResponse,
};
use crate::state::{
    AgentRec, Config, OwnerAcct, PayStatus, Payment, Policy, AGENTS, CONFIG, INVOICES, NEXT_ID,
    OWNERS, OWNER_AGENTS, PAYMENTS,
};

pub type Res = Result<Response<MeMsg>, ContractError>;

const MAX_VENDORS: usize = 32;
const MAX_BATCH: usize = 20;

pub fn instantiate(deps: DepsMut, _env: Env, _info: MessageInfo, msg: InstantiateMsg) -> Res {
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
    NEXT_ID.save(deps.storage, &1)?;
    Ok(Response::new().add_attribute("action", "instantiate"))
}

pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> Res {
    match msg {
        ExecuteMsg::Deposit {} => deposit(deps, info),
        ExecuteMsg::Withdraw { amount } => withdraw(deps, info, amount),
        ExecuteMsg::RegisterAgent { agent, policy } => register(deps, env, info, agent, policy),
        ExecuteMsg::UpdatePolicy { agent, policy } => update_policy(deps, env, info, agent, policy),
        ExecuteMsg::RevokeAgent { agent } => revoke(deps, info, agent),
        ExecuteMsg::Pay {
            vendor,
            amount,
            invoice,
            memo,
        } => pay(deps, env, info, vendor, amount, invoice, memo),
        ExecuteMsg::PayBatch { payments } => pay_batch(deps, env, info, payments),
        ExecuteMsg::Approve { id } => settle(deps, env, info, id, true),
        ExecuteMsg::Reject { id } => settle(deps, env, info, id, false),
    }
}

// ---------------- identity ----------------

fn invalid(msg: &str) -> ContractError {
    ContractError::Invalid {
        msg: msg.to_string(),
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

// ---------------- owner funds ----------------

fn deposit(deps: DepsMut, info: MessageInfo) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let id = verified(deps.as_ref(), &cfg, &info.sender)?;
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
        .add_attribute("amount", amt))
}

fn withdraw(deps: DepsMut, info: MessageInfo, amount: Uint128) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let mut acct = OWNERS
        .may_load(deps.storage, &info.sender)?
        .unwrap_or_default();
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
        .add_attribute("amount", amount))
}

// ---------------- leash management ----------------

fn build_policy(deps: Deps, env: &Env, agent: &Addr, p: PolicyMsg) -> Result<Policy, ContractError> {
    if p.label.trim().is_empty() || p.label.len() > 64 {
        return Err(invalid("label must be 1..64 chars"));
    }
    if p.daily_limit.is_zero() || p.per_tx_limit.is_zero() || p.per_tx_limit > p.daily_limit {
        return Err(invalid("need 0 < per_tx_limit <= daily_limit"));
    }
    if p.vendors.len() > MAX_VENDORS {
        return Err(invalid("too many vendors"));
    }
    if p.vendors.is_empty() && !p.any_verified_vendor {
        return Err(invalid("allow at least one vendor or any_verified_vendor"));
    }
    if p.expires_at <= env.block.time.seconds() {
        return Err(invalid("expires_at must be in the future"));
    }
    let mut vendors = Vec::with_capacity(p.vendors.len());
    for v in p.vendors {
        let a = deps.api.addr_validate(&v)?;
        if &a == agent {
            return Err(ContractError::SelfDealing {});
        }
        if !vendors.contains(&a) {
            vendors.push(a);
        }
    }
    Ok(Policy {
        label: p.label,
        per_tx_limit: p.per_tx_limit,
        daily_limit: p.daily_limit,
        approve_above: p.approve_above,
        vendors,
        any_verified_vendor: p.any_verified_vendor,
        expires_at: p.expires_at,
    })
}

fn register(deps: DepsMut, env: Env, info: MessageInfo, agent: String, p: PolicyMsg) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let id = verified(deps.as_ref(), &cfg, &info.sender)?;
    let agent = deps.api.addr_validate(&agent)?;
    if agent == info.sender {
        return Err(invalid("the agent key must differ from the owner key"));
    }
    if OWNERS.has(deps.storage, &agent) {
        return Err(invalid("an owner account cannot be leashed as an agent"));
    }
    if let Some(existing) = AGENTS.may_load(deps.storage, &agent)? {
        if existing.owner != info.sender {
            return Err(ContractError::AgentTaken {
                agent: agent.to_string(),
            });
        }
    }
    let policy = build_policy(deps.as_ref(), &env, &agent, p)?;
    let rec = AgentRec {
        owner: info.sender.clone(),
        policy,
        day: env.block.time.seconds() / 86400,
        spent: Uint128::zero(),
        revoked: false,
    };
    AGENTS.save(deps.storage, &agent, &rec)?;
    OWNER_AGENTS.save(deps.storage, (&info.sender, &agent), &())?;
    OWNERS.update(deps.storage, &info.sender, |a| -> StdResult<_> {
        Ok(a.unwrap_or_default())
    })?;
    Ok(Response::new()
        .add_attribute("action", "register_agent")
        .add_attribute("owner", info.sender)
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
    rec.policy = build_policy(deps.as_ref(), &env, &agent, p)?;
    rec.revoked = false;
    AGENTS.save(deps.storage, &agent, &rec)?;
    Ok(Response::new()
        .add_attribute("action", "update_policy")
        .add_attribute("agent", agent))
}

fn revoke(deps: DepsMut, info: MessageInfo, agent: String) -> Res {
    let (agent, mut rec) = owned(deps.as_ref(), &info, &agent)?;
    rec.revoked = true;
    AGENTS.save(deps.storage, &agent, &rec)?;
    Ok(Response::new()
        .add_attribute("action", "revoke_agent")
        .add_attribute("agent", agent))
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
    invoice: &str,
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
    if invoice.trim().is_empty() || invoice.len() > 64 {
        return Err(invalid("invoice must be 1..64 chars"));
    }
    // The human behind the agent must still hold a live ME ID. Suspend the ME ID and
    // every agent under it stops paying on the next block.
    let owner_id = verified(deps, cfg, &rec.owner)?;
    if vendor == agent {
        return Err(ContractError::SelfDealing {});
    }
    let vendor_id = did::lookup(deps, vendor.as_str());
    let listed = rec.policy.vendors.contains(vendor);
    let verified_vendor = vendor_id
        .as_ref()
        .map(|v| v.active && v.kyc_level >= cfg.min_kyc_level)
        .unwrap_or(false);
    if !(listed || (rec.policy.any_verified_vendor && verified_vendor)) {
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
    let today = env.block.time.seconds() / 86400;
    let spent = if rec.day == today {
        rec.spent
    } else {
        Uint128::zero()
    };
    if spent + amount > rec.policy.daily_limit {
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
    if let Some(id) = INVOICES.may_load(deps.storage, (&rec.owner, vendor, invoice))? {
        return Err(ContractError::DuplicateInvoice {
            invoice: invoice.to_string(),
            id,
        });
    }
    if in_flight.iter().any(|(v, i)| v == vendor && i == invoice) {
        return Err(ContractError::DuplicateInvoice {
            invoice: invoice.to_string(),
            id: 0,
        });
    }
    Ok(Decision {
        needs_approval: rec.policy.approve_above.map_or(false, |t| amount > t),
        owner_did: owner_id.did,
        vendor_did: vendor_id.map(|v| v.did),
    })
}

fn load_agent(deps: Deps, agent: &Addr) -> Result<AgentRec, ContractError> {
    AGENTS
        .may_load(deps.storage, agent)?
        .ok_or_else(|| ContractError::UnknownAgent {
            agent: agent.to_string(),
        })
}

/// Records one accepted payment; returns its id. Updates `rec`/`acct` in memory.
#[allow(clippy::too_many_arguments)]
fn record(
    deps: &mut DepsMut,
    env: &Env,
    agent: &Addr,
    rec: &mut AgentRec,
    acct: &mut OwnerAcct,
    vendor: &Addr,
    amount: Uint128,
    invoice: &str,
    memo: Option<String>,
    d: &Decision,
) -> StdResult<u64> {
    let today = env.block.time.seconds() / 86400;
    if rec.day != today {
        rec.day = today;
        rec.spent = Uint128::zero();
    }
    // Pending payments count against the daily limit too, so an agent cannot flood
    // the owner with approval requests.
    rec.spent += amount;
    if d.needs_approval {
        acct.reserved += amount;
    } else {
        acct.balance -= amount;
    }
    let id = NEXT_ID.load(deps.storage)?;
    NEXT_ID.save(deps.storage, &(id + 1))?;
    INVOICES.save(deps.storage, (&rec.owner, vendor, invoice), &id)?;
    PAYMENTS.save(
        deps.storage,
        id,
        &Payment {
            id,
            agent: agent.clone(),
            owner: rec.owner.clone(),
            owner_did: d.owner_did.clone(),
            vendor: vendor.clone(),
            vendor_did: d.vendor_did.clone(),
            amount,
            invoice: invoice.to_string(),
            memo,
            status: if d.needs_approval {
                PayStatus::Pending
            } else {
                PayStatus::Paid
            },
            created_at: env.block.time.seconds(),
            settled_at: if d.needs_approval {
                None
            } else {
                Some(env.block.time.seconds())
            },
        },
    )?;
    Ok(id)
}

fn pay(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vendor: String,
    amount: Uint128,
    invoice: String,
    memo: Option<String>,
) -> Res {
    if !info.funds.is_empty() {
        return Err(invalid("agents never attach funds"));
    }
    if memo.as_ref().map_or(false, |m| m.len() > 256) {
        return Err(invalid("memo must be <= 256 chars"));
    }
    let cfg = CONFIG.load(deps.storage)?;
    let agent = info.sender;
    let vendor = deps.api.addr_validate(&vendor)?;
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
        &invoice,
        &[],
    )?;
    let id = record(
        &mut deps, &env, &agent, &mut rec, &mut acct, &vendor, amount, &invoice, memo, &d,
    )?;
    AGENTS.save(deps.storage, &agent, &rec)?;
    OWNERS.save(deps.storage, &rec.owner, &acct)?;

    let mut res = Response::new()
        .add_attribute("action", "pay")
        .add_attribute("payment_id", id.to_string())
        .add_attribute("agent", &agent)
        .add_attribute("owner", &rec.owner)
        .add_attribute("owner_did", &d.owner_did)
        .add_attribute("vendor", &vendor)
        .add_attribute("vendor_did", d.vendor_did.clone().unwrap_or_default())
        .add_attribute("amount", amount)
        .add_attribute("invoice", &invoice);
    if d.needs_approval {
        res = res.add_attribute("status", "pending");
    } else {
        res = res.add_attribute("status", "paid").add_message(BankMsg::Send {
            to_address: vendor.to_string(),
            amount: coins(amount.u128(), cfg.denom),
        });
    }
    Ok(res)
}

fn pay_batch(mut deps: DepsMut, env: Env, info: MessageInfo, items: Vec<PayItem>) -> Res {
    if !info.funds.is_empty() {
        return Err(invalid("agents never attach funds"));
    }
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
    for it in items {
        let vendor = deps.api.addr_validate(&it.vendor)?;
        let d = evaluate(
            deps.as_ref(),
            &env,
            &cfg,
            &agent,
            &rec,
            &acct,
            &vendor,
            it.amount,
            &it.invoice,
            &in_flight,
        )?;
        if d.needs_approval {
            return Err(invalid(
                "batch payments must be under approve_above; send that one with Pay",
            ));
        }
        let id = record(
            &mut deps, &env, &agent, &mut rec, &mut acct, &vendor, it.amount, &it.invoice, None,
            &d,
        )?;
        ids.push(id.to_string());
        in_flight.push((vendor.clone(), it.invoice));
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
        .add_attribute("total", total))
}

fn settle(deps: DepsMut, env: Env, info: MessageInfo, id: u64, approve: bool) -> Res {
    let cfg = CONFIG.load(deps.storage)?;
    let mut p = PAYMENTS.load(deps.storage, id)?;
    if p.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    if p.status != PayStatus::Pending {
        return Err(ContractError::NotPending { id });
    }
    let mut acct = OWNERS.load(deps.storage, &p.owner)?;
    acct.reserved -= p.amount;
    let mut res = Response::new()
        .add_attribute("payment_id", id.to_string())
        .add_attribute("amount", p.amount);
    if approve {
        acct.balance -= p.amount;
        p.status = PayStatus::Paid;
        res = res
            .add_attribute("action", "approve")
            .add_message(BankMsg::Send {
                to_address: p.vendor.to_string(),
                amount: coins(p.amount.u128(), cfg.denom),
            });
    } else {
        p.status = PayStatus::Rejected;
        res = res.add_attribute("action", "reject");
    }
    p.settled_at = Some(env.block.time.seconds());
    PAYMENTS.save(deps.storage, id, &p)?;
    OWNERS.save(deps.storage, &p.owner, &acct)?;
    Ok(res)
}

// ---------------- queries ----------------

pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Whois { agent } => to_json_binary(&whois(deps, &env, agent)?),
        QueryMsg::Owner { owner } => {
            let owner = deps.api.addr_validate(&owner)?;
            let acct = OWNERS.may_load(deps.storage, &owner)?.unwrap_or_default();
            let agents = OWNER_AGENTS
                .prefix(&owner)
                .keys(deps.storage, None, None, Order::Ascending)
                .map(|k| k.map(|a| a.to_string()))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&OwnerResponse {
                me_id: did::lookup(deps, owner.as_str()),
                owner: owner.to_string(),
                balance: acct.balance,
                reserved: acct.reserved,
                agents,
            })
        }
        QueryMsg::CheckPay {
            agent,
            vendor,
            amount,
            invoice,
        } => to_json_binary(&check_pay(deps, &env, agent, vendor, amount, invoice)),
        QueryMsg::Payment { id } => to_json_binary(&PAYMENTS.load(deps.storage, id)?),
        QueryMsg::Payments { start_after, limit } => {
            let limit = limit.unwrap_or(30).min(100) as usize;
            let payments = PAYMENTS
                .range(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit)
                .map(|r| r.map(|(_, p)| p))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&PaymentsResponse { payments })
        }
        QueryMsg::MeId { address } => to_json_binary(&did::lookup(deps, &address)),
    }
}

fn whois(deps: Deps, env: &Env, agent: String) -> StdResult<WhoisResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let agent_addr = deps.api.addr_validate(&agent)?;
    let rec = AGENTS.load(deps.storage, &agent_addr)?;
    let me = did::lookup(deps, rec.owner.as_str());
    let verified = me
        .as_ref()
        .map(|m| m.active && m.kyc_level >= cfg.min_kyc_level)
        .unwrap_or(false);
    let today = env.block.time.seconds() / 86400;
    let spent = if rec.day == today {
        rec.spent
    } else {
        Uint128::zero()
    };
    let acct = OWNERS.may_load(deps.storage, &rec.owner)?.unwrap_or_default();
    Ok(WhoisResponse {
        agent,
        owner: rec.owner.to_string(),
        owner_me_id: me,
        verified,
        remaining_today: rec.policy.daily_limit.saturating_sub(spent),
        spent_today: spent,
        revoked: rec.revoked,
        expired: env.block.time.seconds() >= rec.policy.expires_at,
        owner_balance: acct.balance - acct.reserved,
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
) -> CheckPayResponse {
    let run = || -> Result<Decision, ContractError> {
        let cfg = CONFIG.load(deps.storage)?;
        let agent = deps.api.addr_validate(&agent)?;
        let vendor = deps.api.addr_validate(&vendor)?;
        let rec = load_agent(deps, &agent)?;
        let acct = OWNERS.may_load(deps.storage, &rec.owner)?.unwrap_or_default();
        evaluate(
            deps, env, &cfg, &agent, &rec, &acct, &vendor, amount, &invoice, &[],
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
