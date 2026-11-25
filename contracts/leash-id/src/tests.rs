//! Unit tests with a fake ME Hub DID module behind the Stargate query.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
use cosmwasm_std::{
    coins, from_json, to_json_vec, Addr, BankMsg, Binary, ContractResult, CosmosMsg, Empty, Env,
    OwnedDeps, Querier, QuerierResult, QueryRequest, Response, SystemError, SystemResult,
    Timestamp, Uint128,
};

use crate::contract::{execute, instantiate, invoice_key, migrate, query, MAX_PENDING, PENDING_TTL};
use crate::did::DID_QUERY_PATH;
use crate::me::FIXED_DEPOSIT_QUERY_PATH;
use crate::error::ContractError;
use crate::msg::*;
use crate::state::{ContractVersion, PayStatus, Payment, VERSION};

type Dids = Rc<RefCell<HashMap<String, (String, &'static str, &'static str)>>>;
/// main account -> its EVM-form sub-account, as x/did would list it
type Subs = Rc<RefCell<HashMap<String, String>>>;
/// (account, id) -> (denom, principal, end_time as RFC 3339)
type Deposits = Rc<RefCell<HashMap<(String, u64), (&'static str, u128, String)>>>;

struct DidQuerier {
    base: MockQuerier,
    dids: Dids,
    subs: Subs,
    deposits: Deposits,
}

fn read_str(d: &[u8], at: usize) -> (String, usize) {
    let len = d[at] as usize;
    (String::from_utf8(d[at + 1..at + 1 + len].to_vec()).unwrap(), at + 1 + len)
}

impl Querier for DidQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let req: QueryRequest<Empty> = match from_json(bin_request) {
            Ok(r) => r,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: e.to_string(),
                    request: Binary::from(bin_request),
                })
            }
        };
        if let QueryRequest::Stargate { path, data } = &req {
            let d = data.as_slice();
            assert_eq!(d[0], 0x0a);
            let (addr, next) = read_str(d, 1);
            if path == FIXED_DEPOSIT_QUERY_PATH {
                assert_eq!(d[next], 0x10);
                let id = d[next + 1] as u64;
                return match self.deposits.borrow().get(&(addr.clone(), id)) {
                    Some((denom, amount, end)) => SystemResult::Ok(ContractResult::Ok(Binary::from(
                        format!(r#"{{"FixedDeposit":{{"id":"{id}","account":"{addr}","principal":{{"denom":"{denom}","amount":"{amount}"}},"end_time":"{end}","term":"1"}}}}"#).into_bytes(),
                    ))),
                    None => SystemResult::Ok(ContractResult::Err("fixed deposit not found".into())),
                };
            }
            assert_eq!(path, DID_QUERY_PATH, "only whitelisted ME Hub queries are used");
            let sub = self.subs.borrow().get(&addr).cloned().unwrap_or_default();
            return match self.dids.borrow().get(&addr) {
                Some((did, status, level)) => SystemResult::Ok(ContractResult::Ok(
                    Binary::from(
                        format!(r#"{{"info":{{"did":"{did}","address":"{addr}","pubkey":"","status":"{status}","regionId":"","kycLevel":"{level}","sub_account":"{sub}"}}}}"#)
                            .into_bytes(),
                    ),
                )),
                None => SystemResult::Ok(ContractResult::Err("did not found".into())),
            };
        }
        self.base.raw_query(bin_request)
    }
}

type R = Result<Response<MeMsg>, ContractError>;

struct T {
    deps: OwnedDeps<MockStorage, MockApi, DidQuerier>,
    dids: Dids,
    subs: Subs,
    deposits: Deposits,
    env: Env,
    owner: Addr,
    agent: Addr,
    vendor: Addr,
    shop: Addr,
    stranger: Addr,
}

const DAY: u64 = 86400;
const HOUR: u64 = 3600;

fn setup() -> T {
    let api = MockApi::default().with_prefix("me");
    let dids: Dids = Rc::new(RefCell::new(HashMap::new()));
    let subs: Subs = Rc::new(RefCell::new(HashMap::new()));
    let deposits: Deposits = Rc::new(RefCell::new(HashMap::new()));
    let owner = api.addr_make("owner");
    let agent = api.addr_make("agent");
    let vendor = api.addr_make("vendor");
    let shop = api.addr_make("shop");
    let stranger = api.addr_make("stranger");
    dids.borrow_mut().insert(
        owner.to_string(),
        ("1000000000001".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"),
    );
    dids.borrow_mut().insert(
        shop.to_string(),
        ("1000000000002".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"),
    );
    let mut deps = OwnedDeps {
        storage: MockStorage::default(),
        api,
        querier: DidQuerier {
            base: MockQuerier::default(),
            dids: dids.clone(),
            subs: subs.clone(),
            deposits: deposits.clone(),
        },
        custom_query_type: std::marker::PhantomData,
    };
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(100 * DAY + HOUR);
    instantiate(
        deps.as_mut(),
        env.clone(),
        mock_info("creator", &[]),
        InstantiateMsg {
            min_kyc_level: 2,
            denom: "umec".into(),
        },
    )
    .unwrap();
    let mut t = T {
        deps,
        dids,
        subs,
        deposits,
        env,
        owner,
        agent,
        vendor,
        shop,
        stranger,
    };
    let (o, a) = (t.owner.clone(), t.agent.clone());
    t.exec(&o, ExecuteMsg::Deposit {}, &coins(1_000, "umec")).unwrap();
    let p = t.policy();
    t.leash(&o, &a, p).unwrap();
    t
}

impl T {
    fn policy(&self) -> PolicyMsg {
        PolicyMsg {
            label: "ap-clerk".into(),
            per_tx_limit: Uint128::new(200),
            daily_limit: Uint128::new(300),
            approve_above: Some(Uint128::new(150)),
            vendors: vec![self.vendor.to_string()],
            any_verified_vendor: false,
            expires_at: self.env.block.time.seconds() + 30 * DAY,
        }
    }
    fn exec(&mut self, who: &Addr, msg: ExecuteMsg, funds: &[cosmwasm_std::Coin]) -> R {
        let r = execute(self.deps.as_mut(), self.env.clone(), mock_info(who.as_str(), funds), msg);
        // wasmd refuses the whole tx if any attribute key or value is empty (found live on ME Hub).
        if let Ok(res) = &r {
            let attrs = res.attributes.iter().chain(res.events.iter().flat_map(|e| e.attributes.iter()));
            for a in attrs {
                assert!(!a.key.trim().is_empty() && !a.value.trim().is_empty(), "empty attribute {:?}", a);
            }
        }
        r
    }
    /// Offer + accept: the two steps that put a leash in force.
    fn leash(&mut self, owner: &Addr, agent: &Addr, p: PolicyMsg) -> R {
        self.exec(owner, ExecuteMsg::RegisterAgent { agent: agent.to_string(), policy: p }, &[])?;
        self.exec(agent, ExecuteMsg::AcceptLeash { owner: owner.to_string() }, &[])
    }
    fn set_policy(&mut self, p: PolicyMsg) {
        let (o, a) = (self.owner.clone(), self.agent.clone());
        self.exec(&o, ExecuteMsg::UpdatePolicy { agent: a.to_string(), policy: p }, &[]).unwrap();
    }
    fn pay(&mut self, vendor: &Addr, amount: u128, inv: &str) -> R {
        let a = self.agent.clone();
        self.exec(
            &a,
            ExecuteMsg::Pay {
                vendor: vendor.to_string(),
                amount: Uint128::new(amount),
                invoice: inv.into(),
                memo: None,
            },
            &[],
        )
    }
    fn batch(&mut self, items: &[(&Addr, u128, &str)]) -> R {
        let a = self.agent.clone();
        let payments = items
            .iter()
            .map(|(v, amt, inv)| PayItem { vendor: v.to_string(), amount: Uint128::new(*amt), invoice: inv.to_string() })
            .collect();
        self.exec(&a, ExecuteMsg::PayBatch { payments }, &[])
    }
    fn owner_does(&mut self, msg: ExecuteMsg) -> R {
        let o = self.owner.clone();
        self.exec(&o, msg, &[])
    }
    fn q<Q: serde::de::DeserializeOwned>(&self, msg: QueryMsg) -> Q {
        from_json(query(self.deps.as_ref(), self.env.clone(), msg).unwrap()).unwrap()
    }
    fn whois(&self) -> WhoisResponse {
        self.q(QueryMsg::Whois { agent: self.agent.to_string() })
    }
    fn payment(&self, id: u64) -> Payment {
        self.q(QueryMsg::Payment { id })
    }
    fn check(&self, vendor: &Addr, amount: u128, inv: &str) -> CheckPayResponse {
        self.q(QueryMsg::CheckPay {
            agent: self.agent.to_string(),
            vendor: vendor.to_string(),
            amount: Uint128::new(amount),
            invoice: inv.into(),
            memo: None,
        })
    }
    fn set_did(&self, addr: &Addr, status: &'static str, level: &'static str) {
        let mut m = self.dids.borrow_mut();
        let did = m.get(addr.as_str()).map(|x| x.0.clone()).unwrap_or("1000000000099".into());
        m.insert(addr.to_string(), (did, status, level));
    }
    fn later(&mut self, secs: u64) {
        self.env.block.time = Timestamp::from_seconds(self.env.block.time.seconds() + secs);
    }
}

fn sent(res: &Response<MeMsg>) -> Vec<(String, u128)> {
    res.messages
        .iter()
        .filter_map(|m| match &m.msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                Some((to_address.clone(), amount[0].amount.u128()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn whois_shows_the_verified_human() {
    let t = setup();
    let w = t.whois();
    assert!(w.verified && w.can_pay_now && w.blocked_by.is_none());
    assert_eq!(w.owner, t.owner.to_string());
    assert_eq!(w.owner_me_id.unwrap().did, "1000000000001");
    assert_eq!(w.remaining_24h, Uint128::new(300));
    assert_eq!(w.owner_balance, Uint128::new(1_000));
    assert_eq!(w.min_kyc_level, 2);
    // an unknown address gets a clear "not registered", which the verifier page relies on
    let err = query(t.deps.as_ref(), t.env.clone(), QueryMsg::Whois { agent: t.stranger.to_string() }).unwrap_err();
    assert!(err.to_string().contains("not registered"), "{err}");
}

#[test]
fn pays_allowlisted_vendor_and_records_both_dids() {
    let mut t = setup();
    let v = t.vendor.clone();
    let res = t.pay(&v, 100, "INV-1").unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 100)]);
    let ev = res.events.iter().find(|e| e.ty == "leash_payment").expect("payment event");
    let get = |k: &str| ev.attributes.iter().find(|a| a.key == k).map(|a| a.value.clone());
    assert_eq!(get("status").as_deref(), Some("paid"));
    assert_eq!(get("owner_did").as_deref(), Some("1000000000001"));
    assert_eq!(get("invoice_key").as_deref(), Some("INV1"));
    let p = t.payment(1);
    assert_eq!(p.status, PayStatus::Paid);
    assert_eq!(p.owner_did, "1000000000001");
    assert_eq!(p.vendor_did, None);
    assert_eq!(t.whois().owner_balance, Uint128::new(900));
}

#[test]
fn owner_without_me_id_cannot_leash_anything() {
    let mut t = setup();
    let (s, shop) = (t.stranger.clone(), t.shop.clone());
    let err = t.exec(&s, ExecuteMsg::Deposit {}, &coins(10, "umec")).unwrap_err();
    assert!(matches!(err, ContractError::NoMeId { .. }));
    let p = t.policy();
    let err = t.exec(&s, ExecuteMsg::RegisterAgent { agent: shop.to_string(), policy: p }, &[]).unwrap_err();
    assert!(matches!(err, ContractError::NoMeId { .. }));
}

#[test]
fn low_kyc_owner_is_refused() {
    let mut t = setup();
    let s = t.stranger.clone();
    t.set_did(&s, "DID_STATUS_ACTIVE", "KYC_LEVEL_ONE");
    let err = t.exec(&s, ExecuteMsg::Deposit {}, &coins(10, "umec")).unwrap_err();
    assert!(matches!(err, ContractError::KycTooLow { have: 1, need: 2, .. }));
}

#[test]
fn suspending_the_owner_me_id_freezes_the_agent_immediately() {
    let mut t = setup();
    let (o, v) = (t.owner.clone(), t.vendor.clone());
    t.set_did(&o, "DID_STATUS_INACTIVE ", "KYC_LEVEL_TWO");
    let w = t.whois();
    assert!(!w.verified && !w.can_pay_now);
    assert!(w.blocked_by.unwrap().contains("not active"));
    let err = t.pay(&v, 10, "INV-2").unwrap_err();
    assert!(matches!(err, ContractError::MeIdInactive { .. }));
    t.set_did(&o, "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO");
    t.pay(&v, 10, "INV-2").unwrap();
}

#[test]
fn vendor_rules() {
    let mut t = setup();
    let (shop, stranger, agent) = (t.shop.clone(), t.stranger.clone(), t.agent.clone());
    // shop has an ME ID but is not on the list and any_verified_vendor is off
    assert!(matches!(t.pay(&shop, 10, "A").unwrap_err(), ContractError::VendorNotAllowed { .. }));
    let mut p = t.policy();
    p.any_verified_vendor = true;
    t.set_policy(p);
    t.pay(&shop, 10, "A").unwrap();
    assert_eq!(t.payment(1).vendor_did.as_deref(), Some("1000000000002"));
    // a vendor without ME ID is still refused
    assert!(matches!(t.pay(&stranger, 10, "B").unwrap_err(), ContractError::VendorNotAllowed { .. }));
    // money has to leave the leash: not the agent, not the contract, not another leashed agent
    assert!(matches!(t.pay(&agent, 10, "C").unwrap_err(), ContractError::SelfDealing { .. }));
    let contract = t.env.contract.address.clone();
    assert!(matches!(t.pay(&contract, 10, "C").unwrap_err(), ContractError::SelfDealing { .. }));
    let o = t.owner.clone();
    let p = t.policy();
    t.leash(&o, &shop, p).unwrap(); // shop now also works as an agent of this owner
    assert!(matches!(t.pay(&shop, 10, "D").unwrap_err(), ContractError::SelfDealing { .. }));
}

#[test]
fn daily_limit_is_a_rolling_24_hours() {
    let mut t = setup();
    let v = t.vendor.clone();
    assert!(matches!(t.pay(&v, 201, "BIG").unwrap_err(), ContractError::OverPerTx { .. }));
    // Spend the whole limit just before UTC midnight...
    t.env.block.time = Timestamp::from_seconds(101 * DAY - 60);
    t.pay(&v, 150, "1").unwrap();
    t.pay(&v, 150, "2").unwrap();
    assert!(matches!(t.pay(&v, 1, "3").unwrap_err(), ContractError::OverDaily { .. }));
    // ...midnight is not a reset: 2 minutes later it is still refused.
    t.later(120);
    let err = t.pay(&v, 1, "3").unwrap_err();
    assert!(err.to_string().contains("in the last 24h"), "{err}");
    assert_eq!(t.whois().blocked_by.as_deref(), Some("24h limit used up"));
    t.later(22 * HOUR);
    assert!(t.pay(&v, 1, "3").is_err());
    t.later(2 * HOUR);
    t.pay(&v, 1, "3").unwrap();
    assert_eq!(t.whois().spent_24h, Uint128::new(1));
}

#[test]
fn duplicate_invoice_is_refused_whatever_the_spelling() {
    let mut t = setup();
    let v = t.vendor.clone();
    t.pay(&v, 10, "INV-7").unwrap();
    for again in ["INV-7", "inv-7", " INV_7 ", "inv 7", "INV.7", "INV/7"] {
        assert!(
            matches!(t.pay(&v, 10, again).unwrap_err(), ContractError::DuplicateInvoice { id: 1, .. }),
            "{again}"
        );
    }
    // look-alike Unicode can't dodge the match: it is refused outright
    for odd in ["ＩＮＶ-7", "INV\u{2011}7", "INV-７", "---", "", "   "] {
        assert!(matches!(t.pay(&v, 10, odd).unwrap_err(), ContractError::Invalid { .. }), "{odd:?}");
    }
    assert!(matches!(t.pay(&v, 10, &"9".repeat(65)).unwrap_err(), ContractError::Invalid { .. }));
    // a re-sent invoice is reported as a duplicate even once the daily limit is used up
    t.pay(&v, 140, "INV-8").unwrap();
    t.pay(&v, 140, "INV-9").unwrap();
    assert!(matches!(t.pay(&v, 140, "INV-8").unwrap_err(), ContractError::DuplicateInvoice { id: 2, .. }));
    // and the invoice lookup finds it under any spelling
    let found: Option<Payment> = t.q(QueryMsg::Invoice {
        owner: t.owner.to_string(),
        vendor: v.to_string(),
        invoice: "inv 8".into(),
    });
    assert_eq!(found.unwrap().id, 2);
    assert_eq!(invoice_key(" inv-1001 ").unwrap(), ("inv-1001".to_string(), "INV1001".to_string()));
}

#[test]
fn owner_can_release_an_invoice_number() {
    let mut t = setup();
    let (v, a) = (t.vendor.clone(), t.agent.clone());
    t.pay(&v, 10, "INV-1").unwrap();
    let release = ExecuteMsg::ReleaseInvoice { vendor: v.to_string(), invoice: "inv-1".into() };
    // the agent has no invoices of its own to release
    assert!(t.exec(&a, release.clone(), &[]).is_err());
    t.owner_does(release).unwrap();
    t.pay(&v, 10, "INV-1").unwrap();
}

#[test]
fn approval_flow_reserves_funds_and_reject_frees_the_invoice() {
    let mut t = setup();
    let (v, a) = (t.vendor.clone(), t.agent.clone());
    let res = t.pay(&v, 180, "BIG-1").unwrap();
    assert!(sent(&res).is_empty());
    let w = t.whois();
    assert_eq!((w.owner_balance, w.pending_count), (Uint128::new(820), 1));
    // owner cannot withdraw the reserved part
    assert!(t.owner_does(ExecuteMsg::Withdraw { amount: Uint128::new(900) }).is_err());
    // agent cannot approve its own payment
    assert_eq!(t.exec(&a, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(), ContractError::Unauthorized {});
    let pending: PaymentsResponse = t.q(QueryMsg::Pending { owner: t.owner.to_string(), start_after: None, limit: None });
    assert_eq!(pending.payments.len(), 1);
    let res = t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 180)]);
    assert!(matches!(t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap_err(), ContractError::NotPending { id: 1 }));
    // reject returns the reservation and frees the invoice number for a corrected invoice
    t.later(DAY);
    t.pay(&v, 160, "BIG-2").unwrap();
    t.owner_does(ExecuteMsg::Reject { id: 2 }).unwrap();
    let w = t.whois();
    assert_eq!((w.owner_balance, w.pending_count), (Uint128::new(820), 0));
    t.later(DAY);
    t.pay(&v, 155, "big-2").unwrap();
    assert_eq!(t.payment(3).status, PayStatus::Pending);
}

#[test]
fn approval_rechecks_owner_vendor_and_age() {
    let mut t = setup();
    let (v, o, shop) = (t.vendor.clone(), t.owner.clone(), t.shop.clone());
    let mut p = t.policy();
    p.any_verified_vendor = true;
    t.set_policy(p);
    t.pay(&v, 180, "BIG-1").unwrap();
    t.pay(&v, 100, "SMALL").unwrap(); // under approve_above: paid, not pending
    t.later(DAY);
    t.pay(&shop, 170, "SHOP-1").unwrap();
    // a suspended owner can reject but never approve
    t.set_did(&o, "DID_STATUS_INACTIVE", "KYC_LEVEL_TWO");
    assert!(matches!(t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap_err(), ContractError::MeIdInactive { .. }));
    t.set_did(&o, "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO");
    // a "verified vendor" that lost its ME ID meanwhile can't be paid by approving
    t.set_did(&shop, "DID_STATUS_INACTIVE", "KYC_LEVEL_TWO");
    assert!(matches!(t.owner_does(ExecuteMsg::Approve { id: 3 }).unwrap_err(), ContractError::VendorNotAllowed { .. }));
    assert!(sent(&t.owner_does(ExecuteMsg::Reject { id: 3 }).unwrap()).is_empty());
    // a payment that waited more than 7 days has to be re-sent
    t.later(PENDING_TTL);
    assert!(matches!(t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap_err(), ContractError::PendingExpired { id: 1 }));
    t.owner_does(ExecuteMsg::Reject { id: 1 }).unwrap();
}

#[test]
fn the_approval_queue_is_bounded() {
    let mut t = setup();
    let (o, v) = (t.owner.clone(), t.vendor.clone());
    t.exec(&o, ExecuteMsg::Deposit {}, &coins(100_000, "umec")).unwrap();
    let mut p = t.policy();
    p.daily_limit = Uint128::new(100_000);
    p.approve_above = Some(Uint128::new(1));
    t.set_policy(p);
    for i in 0..MAX_PENDING {
        t.pay(&v, 2, &format!("P{i}")).unwrap();
    }
    assert!(matches!(t.pay(&v, 2, "ONE-MORE").unwrap_err(), ContractError::TooManyPending { .. }));
    // payments that need no approval still go through
    t.pay(&v, 1, "SMALL").unwrap();
}

#[test]
fn revoke_sticks_until_resume_and_clears_the_queue() {
    let mut t = setup();
    let (v, s, a) = (t.vendor.clone(), t.stranger.clone(), t.agent.clone());
    // someone else cannot revoke the agent
    assert_eq!(t.exec(&s, ExecuteMsg::RevokeAgent { agent: a.to_string() }, &[]).unwrap_err(), ContractError::Unauthorized {});
    t.pay(&v, 180, "HELD").unwrap();
    let res = t.owner_does(ExecuteMsg::RevokeAgent { agent: a.to_string() }).unwrap();
    assert!(res.attributes.iter().any(|x| x.key == "rejected_pending" && x.value == "1"));
    assert_eq!(t.payment(1).status, PayStatus::Rejected);
    assert_eq!(t.whois().owner_balance, Uint128::new(1_000));
    assert_eq!(t.pay(&v, 1, "R").unwrap_err(), ContractError::Revoked {});
    // tightening the policy of a revoked (e.g. stolen) key must not bring it back
    let p = t.policy();
    t.set_policy(p);
    assert_eq!(t.pay(&v, 1, "R").unwrap_err(), ContractError::Revoked {});
    assert_eq!(t.whois().blocked_by.as_deref(), Some("revoked by the owner"));
    t.owner_does(ExecuteMsg::ResumeAgent { agent: a.to_string() }).unwrap();
    t.pay(&v, 1, "R").unwrap();
    t.later(31 * DAY);
    assert_eq!(t.pay(&v, 1, "R2").unwrap_err(), ContractError::Expired {});
}

#[test]
fn a_leash_needs_the_agent_to_accept_it() {
    let mut t = setup();
    let (o, s, a, shop) = (t.owner.clone(), t.stranger.clone(), t.agent.clone(), t.shop.clone());
    t.set_did(&s, "DID_STATUS_ACTIVE", "KYC_LEVEL_THREE");
    let p = t.policy();
    // an attacker can offer, but can't take over an agent that already accepted a leash
    t.exec(&s, ExecuteMsg::RegisterAgent { agent: a.to_string(), policy: p.clone() }, &[]).unwrap();
    assert!(matches!(
        t.exec(&a, ExecuteMsg::AcceptLeash { owner: s.to_string() }, &[]).unwrap_err(),
        ContractError::AgentTaken { .. }
    ));
    // nor squat a fresh address: an offer alone puts nobody behind it
    let fresh = t.deps.api.addr_make("fresh");
    t.exec(&s, ExecuteMsg::RegisterAgent { agent: fresh.to_string(), policy: p.clone() }, &[]).unwrap();
    assert!(query(t.deps.as_ref(), t.env.clone(), QueryMsg::Whois { agent: fresh.to_string() }).is_err());
    let offers: OffersResponse = t.q(QueryMsg::Offers { agent: fresh.to_string() });
    assert_eq!(offers.offers.len(), 1);
    // the real owner offers too, and the agent picks it
    t.leash(&o, &fresh, p.clone()).unwrap();
    let w: WhoisResponse = t.q(QueryMsg::Whois { agent: fresh.to_string() });
    assert_eq!(w.owner, o.to_string());
    // accepting needs an offer
    assert!(matches!(
        t.exec(&shop, ExecuteMsg::AcceptLeash { owner: o.to_string() }, &[]).unwrap_err(),
        ContractError::NoOffer { .. }
    ));
    // an agent can leave; its held payments are rejected and the budget comes back
    let v = t.vendor.clone();
    t.pay(&v, 180, "HELD").unwrap();
    t.exec(&a, ExecuteMsg::LeaveLeash {}, &[]).unwrap();
    assert_eq!(t.payment(1).status, PayStatus::Rejected);
    assert!(matches!(t.pay(&v, 1, "X").unwrap_err(), ContractError::UnknownAgent { .. }));
    let ow: OwnerResponse = t.q(QueryMsg::Owner { owner: o.to_string() });
    assert_eq!((ow.reserved, ow.pending_count, ow.agents.len()), (Uint128::zero(), 0, 1));
    // an agent key can't become an owner
    let fresh2 = fresh.clone();
    t.set_did(&fresh2, "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO");
    assert!(t.exec(&fresh2, ExecuteMsg::Deposit {}, &coins(1, "umec")).is_err());
}

#[test]
fn policy_is_validated() {
    let mut t = setup();
    let a = t.agent.to_string();
    let bad = |f: &dyn Fn(&mut PolicyMsg), t: &mut T| {
        let mut p = t.policy();
        f(&mut p);
        let r = t.owner_does(ExecuteMsg::UpdatePolicy { agent: a.clone(), policy: p });
        assert!(matches!(r.unwrap_err(), ContractError::Invalid { .. }));
    };
    bad(&|p| p.approve_above = Some(Uint128::new(200)), &mut t); // == per_tx: would never trigger
    bad(&|p| p.approve_above = Some(Uint128::zero()), &mut t);
    bad(&|p| p.per_tx_limit = Uint128::new(301), &mut t);
    bad(&|p| p.expires_at += 400 * DAY, &mut t);
    bad(&|p| p.label = " ".into(), &mut t);
    bad(&|p| { p.vendors.clear(); p.any_verified_vendor = false; }, &mut t);
}

#[test]
fn only_deposit_takes_funds() {
    let mut t = setup();
    let (v, a, o) = (t.vendor.clone(), t.agent.clone(), t.owner.clone());
    let pay = ExecuteMsg::Pay { vendor: v.to_string(), amount: Uint128::new(1), invoice: "X".into(), memo: None };
    assert_eq!(t.exec(&a, pay, &coins(5, "umec")).unwrap_err(), ContractError::NonPayable {});
    let p = t.policy();
    assert_eq!(
        t.exec(&o, ExecuteMsg::UpdatePolicy { agent: a.to_string(), policy: p }, &coins(5, "umec")).unwrap_err(),
        ContractError::NonPayable {}
    );
    assert_eq!(t.exec(&o, ExecuteMsg::Withdraw { amount: Uint128::new(1) }, &coins(5, "umec")).unwrap_err(), ContractError::NonPayable {});
    // withdraw is for owners who deposited; the agent is refused outright
    assert_eq!(t.exec(&a, ExecuteMsg::Withdraw { amount: Uint128::new(1) }, &[]).unwrap_err(), ContractError::Unauthorized {});
}

#[test]
fn check_pay_matches_pay() {
    let mut t = setup();
    let (v, shop) = (t.vendor.clone(), t.shop.clone());
    let cases: Vec<(Addr, u128, &str)> = vec![
        (v.clone(), 10, "OK1"),
        (v.clone(), 180, "PENDING"),
        (shop.clone(), 10, "NOTLISTED"),
        (v.clone(), 500, "TOOBIG"),
        (v.clone(), 10, "ok-1"),
        (v.clone(), 10, "ＮＯ"),
    ];
    for (vendor, amt, inv) in cases {
        let c = t.check(&vendor, amt, inv);
        let r = t.pay(&vendor, amt, inv);
        assert_eq!(c.ok, r.is_ok(), "{inv}: {:?} vs {:?}", c, r.as_ref().err());
        if let Err(e) = r {
            assert_eq!(c.reason.unwrap(), e.to_string());
        } else {
            assert_eq!(c.needs_approval, inv == "PENDING");
        }
    }
    // memo length is part of the dry run too
    let long: CheckPayResponse = t.q(QueryMsg::CheckPay {
        agent: t.agent.to_string(),
        vendor: v.to_string(),
        amount: Uint128::new(1),
        invoice: "M".into(),
        memo: Some("x".repeat(257)),
    });
    assert!(!long.ok);
}

#[test]
fn history_is_per_agent_and_newest_first() {
    let mut t = setup();
    let (o, v, shop) = (t.owner.clone(), t.vendor.clone(), t.shop.clone());
    let p = t.policy();
    t.leash(&o, &shop, p).unwrap();
    for i in 0..5 {
        t.pay(&v, 1, &format!("A{i}")).unwrap();
        t.exec(&shop, ExecuteMsg::Pay { vendor: v.to_string(), amount: Uint128::new(1), invoice: format!("S{i}"), memo: None }, &[]).unwrap();
    }
    let mine: PaymentsResponse = t.q(QueryMsg::PaymentsByAgent { agent: t.agent.to_string(), start_before: None, limit: Some(3) });
    assert_eq!(mine.payments.iter().map(|p| p.id).collect::<Vec<_>>(), vec![9, 7, 5]);
    let next: PaymentsResponse = t.q(QueryMsg::PaymentsByAgent { agent: t.agent.to_string(), start_before: Some(5), limit: None });
    assert_eq!(next.payments.iter().map(|p| p.id).collect::<Vec<_>>(), vec![3, 1]);
    let all: PaymentsResponse = t.q(QueryMsg::PaymentsByOwner { owner: o.to_string(), start_before: None, limit: None });
    assert_eq!(all.payments.len(), 10);
    assert_eq!(all.payments[0].id, 10);
}

#[test]
fn batch_uses_me_multi_send() {
    let mut t = setup();
    let mut p = t.policy();
    p.any_verified_vendor = true;
    t.set_policy(p);
    let (v, shop) = (t.vendor.clone(), t.shop.clone());
    let res = t.batch(&[(&v, 40, "B1"), (&shop, 60, "B2")]).unwrap();
    let json = String::from_utf8(to_json_vec(&res.messages[0].msg).unwrap()).unwrap();
    assert!(json.starts_with(r#"{"custom":{"multi_send":{"amount":[{"denom":"umec","amount":"100"}]"#), "{json}");
    assert_eq!(res.events.iter().filter(|e| e.ty == "leash_payment").count(), 2);
    assert_eq!(t.whois().spent_24h, Uint128::new(100));
    // the same invoice twice inside one batch is refused, under any spelling
    assert!(matches!(t.batch(&[(&v, 1, "D-1"), (&v, 1, "d1")]).unwrap_err(), ContractError::DuplicateInBatch { .. }));
    // and a batch can never smuggle a payment that needs approval
    assert!(matches!(t.batch(&[(&v, 180, "E")]).unwrap_err(), ContractError::Invalid { .. }));
}

#[test]
fn migrate_only_moves_forward() {
    let mut t = setup();
    let env = t.env.clone();
    assert!(migrate(t.deps.as_mut(), env.clone(), MigrateMsg {}).is_err()); // same version
    VERSION
        .save(&mut t.deps.storage, &ContractVersion { contract: "crates.io:leash-id".into(), version: "0.1.0".into() })
        .unwrap();
    migrate(t.deps.as_mut(), env.clone(), MigrateMsg {}).unwrap();
    VERSION
        .save(&mut t.deps.storage, &ContractVersion { contract: "crates.io:other".into(), version: "0.0.1".into() })
        .unwrap();
    assert!(migrate(t.deps.as_mut(), env, MigrateMsg {}).is_err());
    let v: ContractVersion = from_json(query(t.deps.as_ref(), t.env.clone(), QueryMsg::Version {}).unwrap()).unwrap();
    assert_eq!(v.contract, "crates.io:other");
}

fn iso(secs: u64) -> String {
    // inverse of rfc3339_seconds for whole days+seconds, enough for tests
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.5Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[test]
fn an_evm_sub_account_counts_as_its_main_me_id() {
    let mut t = setup();
    let (shop, stranger) = (t.shop.clone(), t.stranger.clone());
    let shop_evm = t.deps.api.addr_make("shop-evm");
    let mut p = t.policy();
    p.any_verified_vendor = true;
    t.set_policy(p);
    // the merchant takes payment at its EVM-form address: on its own, that address has no ME ID
    assert!(matches!(t.pay(&shop_evm, 10, "S-1").unwrap_err(), ContractError::VendorNotAllowed { .. }));
    let reg = |sub: &Addr, main: &Addr| ExecuteMsg::RegisterSubAccount { sub: sub.to_string(), main: main.to_string() };
    // nobody can register a link x/did doesn't have
    assert!(matches!(t.exec(&stranger, reg(&shop_evm, &shop), &[]).unwrap_err(), ContractError::NotSubAccount { .. }));
    t.subs.borrow_mut().insert(shop.to_string(), shop_evm.to_string());
    assert!(matches!(t.exec(&stranger, reg(&shop_evm, &t.owner.clone()), &[]).unwrap_err(), ContractError::NotSubAccount { .. }));
    // anyone may record a link x/did does have
    t.exec(&stranger, reg(&shop_evm, &shop), &[]).unwrap();
    t.pay(&shop_evm, 10, "S-1").unwrap();
    assert_eq!(t.payment(1).vendor_did.as_deref(), Some("1000000000002"));
    let idr: IdentityResponse = t.q(QueryMsg::Identity { address: shop_evm.to_string() });
    assert_eq!(idr.via_main.as_deref(), Some(shop.as_str()));
    // an agent whose key also holds an ME ID can't pay itself through its own EVM form
    let (o, a) = (t.owner.clone(), t.agent.clone());
    let p = t.policy();
    t.leash(&o, &shop, p).unwrap(); // shop's key is now a leashed agent
    let a_pay = ExecuteMsg::Pay { vendor: shop_evm.to_string(), amount: Uint128::new(10), invoice: "S-SELF".into(), memo: None };
    assert!(matches!(t.exec(&a, a_pay, &[]).unwrap_err(), ContractError::SelfDealing { .. }));
    t.exec(&shop, ExecuteMsg::LeaveLeash {}, &[]).unwrap();
    // x/did may hold the sub-account in upper case; it still matches
    t.subs.borrow_mut().insert(shop.to_string(), shop_evm.to_string().to_uppercase());
    t.pay(&shop_evm, 10, "S-UPPER").unwrap();
    t.subs.borrow_mut().insert(shop.to_string(), shop_evm.to_string());
    // if x/did drops the binding, the link stops counting immediately
    t.subs.borrow_mut().remove(shop.as_str());
    assert!(matches!(t.pay(&shop_evm, 10, "S-2").unwrap_err(), ContractError::VendorNotAllowed { .. }));
    let idr: IdentityResponse = t.q(QueryMsg::Identity { address: shop_evm.to_string() });
    assert!(idr.me_id.is_none());
}

#[test]
fn owners_can_show_a_fixed_deposit_bond() {
    let mut t = setup();
    let (o, s) = (t.owner.clone(), t.stranger.clone());
    let now = t.env.block.time.seconds();
    t.deposits.borrow_mut().insert((o.to_string(), 1), ("umec", 5_000, iso(now + 10 * DAY)));
    t.deposits.borrow_mut().insert((o.to_string(), 2), ("umec", 2_000, iso(now + 20 * DAY)));
    t.deposits.borrow_mut().insert((o.to_string(), 3), ("uother", 9_000, iso(now + 20 * DAY)));
    t.deposits.borrow_mut().insert((o.to_string(), 4), ("umec", 9_000, iso(now - DAY)));
    assert!(t.whois().owner_bond.is_none());
    for bad in [3u64, 4, 99] {
        assert!(matches!(t.owner_does(ExecuteMsg::SetBond { deposit_ids: vec![1, bad] }).unwrap_err(), ContractError::BadBond { .. }), "{bad}");
    }
    // someone without an ME ID can't post a bond
    assert!(matches!(t.exec(&s, ExecuteMsg::SetBond { deposit_ids: vec![1] }, &[]).unwrap_err(), ContractError::NoMeId { .. }));
    t.owner_does(ExecuteMsg::SetBond { deposit_ids: vec![1, 2, 2] }).unwrap();
    let b = t.whois().owner_bond.unwrap();
    assert_eq!((b.amount, b.earliest_unlock, b.deposits.len()), (Uint128::new(7_000), now + 10 * DAY, 2));
    // read live: once #1 unlocks it no longer counts, and a withdrawn deposit just drops out
    t.later(15 * DAY);
    assert_eq!(t.whois().owner_bond.unwrap().amount, Uint128::new(2_000));
    t.deposits.borrow_mut().remove(&(o.to_string(), 2));
    assert!(t.whois().owner_bond.is_none());
    t.owner_does(ExecuteMsg::SetBond { deposit_ids: vec![] }).unwrap();
}

/// Random operation sequences against the accounting invariants:
///   contract bank balance == sum of owner balances,
///   each owner's reserved == sum of their pending payments, pending count matches, reserved <= balance.
/// Bank balance is tracked from what the contract actually sends (BankMsg::Send, multi_send) and receives.
#[test]
fn accounting_invariants_hold_under_random_operations() {
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }
    let mut ok_ops = [0u32; 12];
    for seed in 1..=40u64 {
        let mut t = setup();
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let api = t.deps.api;
        let owner2 = api.addr_make("owner2");
        t.dids.borrow_mut().insert(owner2.to_string(), ("1000000000003".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
        let agent2 = api.addr_make("agent2");
        let agent3 = api.addr_make("agent3");
        let (o1, a1, v, shop) = (t.owner.clone(), t.agent.clone(), t.vendor.clone(), t.shop.clone());
        let mut p = t.policy();
        p.any_verified_vendor = true;
        t.set_policy(p.clone());
        t.exec(&owner2, ExecuteMsg::Deposit {}, &coins(2_000, "umec")).unwrap();
        t.leash(&owner2, &agent2, p.clone()).unwrap();
        t.leash(&o1, &agent3, p.clone()).unwrap();
        // owner2 needs two signatures: themselves and a co-approver
        let cfo2 = api.addr_make("cfo2");
        t.dids.borrow_mut().insert(cfo2.to_string(), ("1000000000004".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
        t.exec(&owner2, ExecuteMsg::SetApprovers { approvers: vec![cfo2.to_string()], threshold: 2 }, &[]).unwrap();
        let mut bank: u128 = 3_000;
        let owners = [o1.clone(), owner2.clone()];
        let agents = [a1.clone(), agent2.clone(), agent3.clone()];
        let vendors = [v.clone(), shop.clone()];
        for step in 0..250 {
            let op = match rng.pick(15) {
                12..=14 => 6, // approvals need more tries now that some owners need two signatures
                x => x,
            };
            let owner = owners[rng.pick(2) as usize].clone();
            let agent = agents[rng.pick(3) as usize].clone();
            let vendor = vendors[rng.pick(2) as usize].clone();
            let amt = 1 + rng.pick(220) as u128;
            let inv = format!("I{}", rng.pick(40));
            let r = match op {
                0 => t.exec(&owner, ExecuteMsg::Deposit {}, &coins(amt, "umec")),
                1 => t.exec(&owner, ExecuteMsg::Withdraw { amount: Uint128::new(amt) }, &[]),
                2..=4 => t.exec(&agent, ExecuteMsg::Pay { vendor: vendor.to_string(), amount: Uint128::new(amt), invoice: inv, memo: None }, &[]),
                5 => t.exec(&agent, ExecuteMsg::PayBatch { payments: vec![
                    PayItem { vendor: vendor.to_string(), amount: Uint128::new(amt % 100 + 1), invoice: inv.clone() },
                    PayItem { vendor: vendors[0].to_string(), amount: Uint128::new(amt % 50 + 1), invoice: format!("{inv}b") },
                ] }, &[]),
                6 | 7 => {
                    let id = 1 + rng.pick(30);
                    let signer = if rng.pick(3) == 0 { cfo2.clone() } else { owner.clone() };
                    t.exec(&signer, if op == 6 { ExecuteMsg::Approve { id } } else { ExecuteMsg::Reject { id } }, &[])
                }
                8 => t.exec(&owner, ExecuteMsg::RevokeAgent { agent: agent.to_string() }, &[]),
                9 => t.exec(&owner, ExecuteMsg::ResumeAgent { agent: agent.to_string() }, &[]),
                10 => t.exec(&owner, ExecuteMsg::ReleaseInvoice { vendor: vendor.to_string(), invoice: inv }, &[]),
                _ => {
                    t.later(rng.pick(30) * HOUR);
                    continue;
                }
            };
            if r.is_ok() {
                ok_ops[op as usize] += 1;
            }
            if op == 0 && r.is_ok() {
                bank += amt;
            }
            if let Ok(res) = &r {
                for m in &res.messages {
                    match &m.msg {
                        CosmosMsg::Bank(BankMsg::Send { amount, .. }) => bank -= amount[0].amount.u128(),
                        CosmosMsg::Custom(MeMsg::MultiSend(ms)) => {
                            let outs: u128 = ms.output.iter().map(|o| o.amount[0].amount.u128()).sum();
                            assert_eq!(outs, ms.amount[0].amount.u128(), "multi_send inputs == outputs");
                            bank -= outs;
                        }
                        other => panic!("unexpected message {other:?}"),
                    }
                }
            }
            // invariants
            let mut total = 0u128;
            for o in &owners {
                let or: OwnerResponse = t.q(QueryMsg::Owner { owner: o.to_string() });
                total += or.balance.u128();
                assert!(or.reserved <= or.balance, "seed {seed} step {step}: reserved > balance");
                let pend: PaymentsResponse = t.q(QueryMsg::Pending { owner: o.to_string(), start_after: None, limit: Some(100) });
                let sum: u128 = pend.payments.iter().map(|p| p.amount.u128()).sum();
                assert_eq!(sum, or.reserved.u128(), "seed {seed} step {step}: reserved != pending sum");
                assert_eq!(pend.payments.len() as u32, or.pending_count, "seed {seed} step {step}: pending count");
                assert!(pend.payments.iter().all(|p| p.status == PayStatus::Pending));
            }
            assert_eq!(total, bank, "seed {seed} step {step} op {op}: owner balances != contract bank balance");
        }
    }
    // the sequences must actually exercise every path, not just bounce off refusals
    for (op, name) in [(0, "deposit"), (1, "withdraw"), (2, "pay"), (5, "batch"), (6, "approve"), (7, "reject"), (8, "revoke"), (9, "resume"), (10, "release")] {
        assert!(ok_ops[op] >= 5, "only {} successful {name}", ok_ops[op]);
    }
}

#[test]
fn held_payments_can_need_two_signatures() {
    let mut t = setup();
    let (o, v, s, a) = (t.owner.clone(), t.vendor.clone(), t.stranger.clone(), t.agent.clone());
    let cfo = t.deps.api.addr_make("cfo");
    let set = |list: Vec<&Addr>, threshold: u8| ExecuteMsg::SetApprovers { approvers: list.iter().map(|x| x.to_string()).collect(), threshold };
    // co-approvers must be verified humans, not agent keys, and the threshold must be reachable
    assert!(matches!(t.owner_does(set(vec![&cfo], 2)).unwrap_err(), ContractError::NoMeId { .. }));
    t.dids.borrow_mut().insert(cfo.to_string(), ("1000000000007".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
    assert!(matches!(t.owner_does(set(vec![&a], 2)).unwrap_err(), ContractError::Invalid { .. }));
    assert!(matches!(t.owner_does(set(vec![&cfo], 3)).unwrap_err(), ContractError::Invalid { .. }));
    assert!(matches!(t.owner_does(set(vec![&cfo], 0)).unwrap_err(), ContractError::Invalid { .. }));
    t.owner_does(set(vec![&cfo], 2)).unwrap();
    assert_eq!(t.whois().approvals_needed, 2);
    let owners: Vec<String> = t.q(QueryMsg::ApproverOf { approver: cfo.to_string() });
    assert_eq!(owners, vec![o.to_string()]);

    // one signature isn't enough; the same signer twice doesn't count; outsiders can't sign
    t.pay(&v, 180, "BIG-1").unwrap();
    let res = t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap();
    assert!(sent(&res).is_empty());
    assert_eq!(t.payment(1).status, PayStatus::Pending);
    assert!(matches!(t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap_err(), ContractError::AlreadyApproved { have: 1, need: 2, .. }));
    assert_eq!(t.exec(&s, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(), ContractError::Unauthorized {});
    assert_eq!(t.exec(&a, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(), ContractError::Unauthorized {});
    let res = t.exec(&cfo, ExecuteMsg::Approve { id: 1 }, &[]).unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 180)]);
    assert_eq!(t.payment(1).approved_by, vec![o.clone(), cfo.clone()]);

    // a signature stops counting if the signer's ME ID is suspended before the payment is released
    t.later(DAY);
    t.pay(&v, 170, "BIG-2").unwrap();
    t.exec(&cfo, ExecuteMsg::Approve { id: 2 }, &[]).unwrap();
    t.set_did(&cfo, "DID_STATUS_INACTIVE", "KYC_LEVEL_TWO");
    let res = t.owner_does(ExecuteMsg::Approve { id: 2 }).unwrap();
    assert!(sent(&res).is_empty(), "the suspended co-approver's earlier signature must not count");
    assert!(matches!(t.exec(&cfo, ExecuteMsg::Approve { id: 2 }, &[]).unwrap_err(), ContractError::MeIdInactive { .. }));
    // ...but any one signer can still stop it, suspended or not
    t.exec(&cfo, ExecuteMsg::Reject { id: 2 }, &[]).unwrap();
    assert_eq!(t.payment(2).status, PayStatus::Rejected);
    t.set_did(&cfo, "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO");

    // a co-approver can't approve a payment to themselves, and can't become an agent
    let mut p = t.policy();
    p.vendors.push(cfo.to_string());
    t.set_policy(p.clone());
    t.later(DAY);
    t.pay(&cfo, 170, "TO-CFO").unwrap();
    t.owner_does(ExecuteMsg::Approve { id: 3 }).unwrap();
    assert!(matches!(t.exec(&cfo, ExecuteMsg::Approve { id: 3 }, &[]).unwrap_err(), ContractError::SelfDealing { .. }));
    t.owner_does(ExecuteMsg::RegisterAgent { agent: cfo.to_string(), policy: t.policy() }).unwrap();
    assert!(matches!(t.exec(&cfo, ExecuteMsg::AcceptLeash { owner: o.to_string() }, &[]).unwrap_err(), ContractError::Invalid { .. }));

    // back to the owner alone
    t.owner_does(set(vec![], 1)).unwrap();
    assert_eq!(t.whois().approvals_needed, 1);
    let owners: Vec<String> = t.q(QueryMsg::ApproverOf { approver: cfo.to_string() });
    assert!(owners.is_empty());
    assert_eq!(t.exec(&cfo, ExecuteMsg::Reject { id: 3 }, &[]).unwrap_err(), ContractError::Unauthorized {});
}

/// Garbage in, no panic out: the DID and fixed-deposit parsers and the invoice key never crash on arbitrary
/// bytes (a node answering oddly must fail closed, not abort the transaction), and the invoice key keeps its shape.
#[test]
fn parsers_survive_arbitrary_input() {
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let mut next = || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x };
    let pieces: [&[u8]; 12] = [b"{\"info\":{", b"\"did\":\"7\"", b"\"status\":\"DID_STATUS_ACTIVE\"", b"\"kycLevel\":\"KYC_LEVEL_TWO\"", b"}", b",",
        b"{\"FixedDeposit\":{", b"\"principal\":{\"denom\":\"umec\",\"amount\":\"99999999999999999999999999999999999999\"}", b"\"end_time\":\"2026-02-30T25:61:61+99:99\"", b"\x0a\x80\x80\x80", b"\xff", b"\x12\x05me1ab"];
    for _ in 0..20_000 {
        let n = (next() % 24) as usize;
        let mut buf = Vec::new();
        for _ in 0..n {
            if next() % 3 == 0 {
                buf.extend((0..(next() % 12)).map(|_| next() as u8));
            } else {
                buf.extend_from_slice(pieces[(next() % pieces.len() as u64) as usize]);
            }
        }
        let _ = crate::did::parse_response(&buf);
        let _ = crate::me::parse_response(&buf);
        if let Ok(s) = std::str::from_utf8(&buf) {
            let _ = crate::me::rfc3339_seconds(s);
            if let Ok((shown, key)) = invoice_key(s) {
                assert!(!key.is_empty() && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()), "{key:?}");
                assert!(shown.chars().count() <= 64 && shown == shown.trim());
            }
        }
    }
}

/// One vendor, two addresses (its main account and its registered EVM sub-account): an invoice is still paid once.
#[test]
fn an_invoice_is_paid_once_across_a_vendors_addresses() {
    let mut t = setup();
    let (shop, stranger) = (t.shop.clone(), t.stranger.clone());
    let shop_evm = t.deps.api.addr_make("shop-evm");
    let mut p = t.policy();
    p.any_verified_vendor = true;
    p.vendors.push(shop_evm.to_string());
    t.set_policy(p);
    // paid at the EVM address before anyone recorded the link
    t.pay(&shop_evm, 10, "PRE-1").unwrap();
    t.subs.borrow_mut().insert(shop.to_string(), shop_evm.to_string());
    t.exec(&stranger, ExecuteMsg::RegisterSubAccount { sub: shop_evm.to_string(), main: shop.to_string() }, &[]).unwrap();
    // ...is still a duplicate at that address after the link
    assert!(matches!(t.pay(&shop_evm, 10, "pre 1").unwrap_err(), ContractError::DuplicateInvoice { id: 1, .. }));
    // paid at the main account, then again at the sub-account, spelled differently: refused
    t.pay(&shop, 10, "INV-9").unwrap();
    assert!(matches!(t.pay(&shop_evm, 10, "inv_9").unwrap_err(), ContractError::DuplicateInvoice { id: 2, .. }));
    // and the other way round
    t.pay(&shop_evm, 10, "INV-10").unwrap();
    assert!(matches!(t.pay(&shop, 10, "INV-10").unwrap_err(), ContractError::DuplicateInvoice { id: 3, .. }));
    let found: Option<Payment> = t.q(QueryMsg::Invoice { owner: t.owner.to_string(), vendor: shop.to_string(), invoice: "inv 10".into() });
    assert_eq!(found.map(|p| p.id), Some(3));
    // both addresses in one batch
    assert!(matches!(t.batch(&[(&shop, 5, "B-1"), (&shop_evm, 5, "B-1")]).unwrap_err(), ContractError::DuplicateInBatch { .. }));
    // the owner can still release the number, from either address
    t.owner_does(ExecuteMsg::ReleaseInvoice { vendor: shop_evm.to_string(), invoice: "INV-9".into() }).unwrap();
    t.pay(&shop, 10, "INV-9").unwrap();
}

/// Two-person approval needs two people: two addresses of one ME ID count once, whichever way they got there.
#[test]
fn two_signatures_must_come_from_two_me_ids() {
    let mut t = setup();
    let (o, v, s) = (t.owner.clone(), t.vendor.clone(), t.stranger.clone());
    let set = |list: Vec<&Addr>, threshold: u8| ExecuteMsg::SetApprovers { approvers: list.iter().map(|x| x.to_string()).collect(), threshold };
    let reg = |sub: &Addr, main: &Addr| ExecuteMsg::RegisterSubAccount { sub: sub.to_string(), main: main.to_string() };
    // the owner's own EVM sub-account is the owner, not a second person
    let owner_evm = t.deps.api.addr_make("owner-evm");
    t.subs.borrow_mut().insert(o.to_string(), owner_evm.to_string());
    t.exec(&s, reg(&owner_evm, &o), &[]).unwrap();
    assert!(matches!(t.owner_does(set(vec![&owner_evm], 2)).unwrap_err(), ContractError::Invalid { .. }));
    // nor are a co-approver's two addresses two people
    let cfo = t.deps.api.addr_make("cfo");
    let cfo_evm = t.deps.api.addr_make("cfo-evm");
    t.dids.borrow_mut().insert(cfo.to_string(), ("1000000000007".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
    t.subs.borrow_mut().insert(cfo.to_string(), cfo_evm.to_string());
    t.exec(&s, reg(&cfo_evm, &cfo), &[]).unwrap();
    assert!(matches!(t.owner_does(set(vec![&cfo, &cfo_evm], 3)).unwrap_err(), ContractError::Invalid { .. }));

    // an address that only later becomes another signer's sub-account: its signature stops counting separately
    let aud = t.deps.api.addr_make("auditor");
    let aud_evm = t.deps.api.addr_make("auditor-evm");
    t.dids.borrow_mut().insert(aud.to_string(), ("1000000000008".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
    t.dids.borrow_mut().insert(aud_evm.to_string(), ("1000000000009".into(), "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO"));
    t.owner_does(set(vec![&aud, &aud_evm], 3)).unwrap();
    t.dids.borrow_mut().remove(aud_evm.as_str());
    t.subs.borrow_mut().insert(aud.to_string(), aud_evm.to_string());
    t.exec(&s, reg(&aud_evm, &aud), &[]).unwrap();
    t.pay(&v, 180, "BIG-1").unwrap();
    t.owner_does(ExecuteMsg::Approve { id: 1 }).unwrap();
    t.exec(&aud, ExecuteMsg::Approve { id: 1 }, &[]).unwrap();
    assert!(matches!(t.exec(&aud_evm, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(), ContractError::AlreadyApproved { have: 2, need: 3, .. }));
    assert_eq!(t.payment(1).status, PayStatus::Pending);

    // a signer can't approve a payment to their own sub-account
    t.owner_does(set(vec![&cfo], 2)).unwrap();
    let mut p = t.policy();
    p.vendors.push(cfo_evm.to_string());
    t.set_policy(p);
    t.later(DAY);
    t.pay(&cfo_evm, 170, "TO-CFO").unwrap();
    let held: PaymentsResponse = t.q(QueryMsg::Pending { owner: o.to_string(), start_after: None, limit: None });
    let to_cfo = held.payments.iter().find(|p| p.vendor == cfo_evm).unwrap().id;
    t.owner_does(ExecuteMsg::Approve { id: to_cfo }).unwrap();
    assert!(matches!(t.exec(&cfo, ExecuteMsg::Approve { id: to_cfo }, &[]).unwrap_err(), ContractError::SelfDealing { .. }));
}
