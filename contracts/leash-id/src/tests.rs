//! Unit tests with a fake ME Hub DID module behind the Stargate query.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
use cosmwasm_std::{
    coins, from_json, to_json_vec, Addr, BankMsg, Binary, ContractResult, CosmosMsg, Empty, Env,
    OwnedDeps, Querier, QuerierResult, QueryRequest, SystemError, SystemResult, Timestamp,
    Uint128,
};

use crate::contract::{execute, instantiate, query};
use crate::did::DID_QUERY_PATH;
use crate::error::ContractError;
use crate::msg::*;
use crate::state::{PayStatus, Payment};

type Dids = Rc<RefCell<HashMap<String, (String, &'static str, &'static str)>>>;

struct DidQuerier {
    base: MockQuerier,
    dids: Dids,
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
            assert_eq!(path, DID_QUERY_PATH, "only the whitelisted DID query is used");
            let d = data.as_slice();
            assert_eq!(d[0], 0x0a);
            let addr = String::from_utf8(d[2..2 + d[1] as usize].to_vec()).unwrap();
            return match self.dids.borrow().get(&addr) {
                Some((did, status, level)) => SystemResult::Ok(ContractResult::Ok(
                    Binary::from(
                        format!(r#"{{"info":{{"did":"{did}","address":"{addr}","pubkey":"","status":"{status}","regionId":"","kycLevel":"{level}","sub_account":""}}}}"#)
                            .into_bytes(),
                    ),
                )),
                None => SystemResult::Ok(ContractResult::Err("did not found".into())),
            };
        }
        self.base.raw_query(bin_request)
    }
}

struct T {
    deps: OwnedDeps<MockStorage, MockApi, DidQuerier>,
    dids: Dids,
    env: Env,
    owner: Addr,
    agent: Addr,
    vendor: Addr,
    shop: Addr,
    stranger: Addr,
}

const DAY: u64 = 86400;

fn setup() -> T {
    let api = MockApi::default().with_prefix("me");
    let dids: Dids = Rc::new(RefCell::new(HashMap::new()));
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
        },
        custom_query_type: std::marker::PhantomData,
    };
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(100 * DAY + 3600);
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
        env,
        owner,
        agent,
        vendor,
        shop,
        stranger,
    };
    t.exec(&t.owner.clone(), ExecuteMsg::Deposit {}, &coins(1_000, "umec"))
        .unwrap();
    let p = t.policy();
    t.exec(
        &t.owner.clone(),
        ExecuteMsg::RegisterAgent {
            agent: t.agent.to_string(),
            policy: p,
        },
        &[],
    )
    .unwrap();
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
    fn exec(
        &mut self,
        who: &Addr,
        msg: ExecuteMsg,
        funds: &[cosmwasm_std::Coin],
    ) -> Result<cosmwasm_std::Response<MeMsg>, ContractError> {
        let r = execute(
            self.deps.as_mut(),
            self.env.clone(),
            mock_info(who.as_str(), funds),
            msg,
        );
        // wasmd refuses the whole tx if any attribute key or value is empty (found live on ME Hub).
        if let Ok(res) = &r {
            for a in &res.attributes {
                assert!(!a.key.trim().is_empty() && !a.value.trim().is_empty(), "empty attribute {:?}", a);
            }
        }
        r
    }
    fn pay(&mut self, vendor: &Addr, amount: u128, inv: &str) -> Result<cosmwasm_std::Response<MeMsg>, ContractError> {
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
    fn q<R: serde::de::DeserializeOwned>(&self, msg: QueryMsg) -> R {
        from_json(query(self.deps.as_ref(), self.env.clone(), msg).unwrap()).unwrap()
    }
    fn whois(&self) -> WhoisResponse {
        self.q(QueryMsg::Whois {
            agent: self.agent.to_string(),
        })
    }
    fn check(&self, vendor: &Addr, amount: u128, inv: &str) -> CheckPayResponse {
        self.q(QueryMsg::CheckPay {
            agent: self.agent.to_string(),
            vendor: vendor.to_string(),
            amount: Uint128::new(amount),
            invoice: inv.into(),
        })
    }
    fn set_did(&self, addr: &Addr, status: &'static str, level: &'static str) {
        let mut m = self.dids.borrow_mut();
        let did = m.get(addr.as_str()).map(|x| x.0.clone()).unwrap_or("1000000000099".into());
        m.insert(addr.to_string(), (did, status, level));
    }
}

fn sent(res: &cosmwasm_std::Response<MeMsg>) -> Vec<(String, u128)> {
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
    assert!(w.verified);
    assert_eq!(w.owner, t.owner.to_string());
    assert_eq!(w.owner_me_id.unwrap().did, "1000000000001");
    assert_eq!(w.remaining_today, Uint128::new(300));
    assert_eq!(w.owner_balance, Uint128::new(1_000));
}

#[test]
fn pays_allowlisted_vendor_and_records_both_dids() {
    let mut t = setup();
    let v = t.vendor.clone();
    let res = t.pay(&v, 100, "INV-1").unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 100)]);
    let p: Payment = t.q(QueryMsg::Payment { id: 1 });
    assert_eq!(p.status, PayStatus::Paid);
    assert_eq!(p.owner_did, "1000000000001");
    assert_eq!(p.vendor_did, None);
    assert_eq!(t.whois().owner_balance, Uint128::new(900));
}

#[test]
fn owner_without_me_id_cannot_leash_anything() {
    let mut t = setup();
    let s = t.stranger.clone();
    let err = t
        .exec(&s, ExecuteMsg::Deposit {}, &coins(10, "umec"))
        .unwrap_err();
    assert!(matches!(err, ContractError::NoMeId { .. }));
    let p = t.policy();
    let err = t
        .exec(
            &s,
            ExecuteMsg::RegisterAgent {
                agent: t.shop.to_string(),
                policy: p,
            },
            &[],
        )
        .unwrap_err();
    assert!(matches!(err, ContractError::NoMeId { .. }));
}

#[test]
fn low_kyc_owner_is_refused() {
    let mut t = setup();
    let s = t.stranger.clone();
    t.set_did(&s, "DID_STATUS_ACTIVE", "KYC_LEVEL_ONE");
    let err = t
        .exec(&s, ExecuteMsg::Deposit {}, &coins(10, "umec"))
        .unwrap_err();
    assert!(matches!(err, ContractError::KycTooLow { have: 1, need: 2, .. }));
}

#[test]
fn suspending_the_owner_me_id_freezes_the_agent_immediately() {
    let mut t = setup();
    let (o, v) = (t.owner.clone(), t.vendor.clone());
    t.set_did(&o, "DID_STATUS_INACTIVE ", "KYC_LEVEL_TWO");
    assert!(!t.whois().verified);
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
    assert!(matches!(
        t.pay(&shop, 10, "A").unwrap_err(),
        ContractError::VendorNotAllowed { .. }
    ));
    let mut p = t.policy();
    p.any_verified_vendor = true;
    let o = t.owner.clone();
    t.exec(
        &o,
        ExecuteMsg::UpdatePolicy {
            agent: agent.to_string(),
            policy: p,
        },
        &[],
    )
    .unwrap();
    t.pay(&shop, 10, "A").unwrap();
    let pm: Payment = t.q(QueryMsg::Payment { id: 1 });
    assert_eq!(pm.vendor_did.as_deref(), Some("1000000000002"));
    // a vendor without ME ID is still refused
    assert!(matches!(
        t.pay(&stranger, 10, "B").unwrap_err(),
        ContractError::VendorNotAllowed { .. }
    ));
    // the agent cannot pay itself
    assert!(matches!(
        t.pay(&agent, 10, "C").unwrap_err(),
        ContractError::SelfDealing {}
    ));
}

#[test]
fn limits_and_utc_day_reset() {
    let mut t = setup();
    let v = t.vendor.clone();
    assert!(matches!(
        t.pay(&v, 201, "BIG").unwrap_err(),
        ContractError::OverPerTx { .. }
    ));
    t.pay(&v, 150, "1").unwrap();
    t.pay(&v, 150, "2").unwrap();
    assert!(matches!(
        t.pay(&v, 1, "3").unwrap_err(),
        ContractError::OverDaily { .. }
    ));
    t.env.block.time = Timestamp::from_seconds(101 * DAY);
    t.pay(&v, 1, "3").unwrap();
}

#[test]
fn duplicate_invoice_is_refused() {
    let mut t = setup();
    let v = t.vendor.clone();
    t.pay(&v, 10, "INV-7").unwrap();
    assert!(matches!(
        t.pay(&v, 10, "INV-7").unwrap_err(),
        ContractError::DuplicateInvoice { id: 1, .. }
    ));
    // a re-sent invoice is reported as a duplicate even once the daily limit is used up
    t.pay(&v, 140, "INV-8").unwrap();
    t.pay(&v, 140, "INV-9").unwrap();
    assert!(matches!(
        t.pay(&v, 140, "INV-8").unwrap_err(),
        ContractError::DuplicateInvoice { id: 2, .. }
    ));
}

#[test]
fn approval_flow_reserves_funds() {
    let mut t = setup();
    let (v, o) = (t.vendor.clone(), t.owner.clone());
    let res = t.pay(&v, 180, "BIG-1").unwrap();
    assert!(sent(&res).is_empty());
    assert_eq!(t.whois().owner_balance, Uint128::new(820));
    // owner cannot withdraw the reserved part
    assert!(t
        .exec(&o, ExecuteMsg::Withdraw { amount: Uint128::new(900) }, &[])
        .is_err());
    // agent cannot approve its own payment
    let a = t.agent.clone();
    assert_eq!(
        t.exec(&a, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(),
        ContractError::Unauthorized {}
    );
    let res = t.exec(&o, ExecuteMsg::Approve { id: 1 }, &[]).unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 180)]);
    assert!(matches!(
        t.exec(&o, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(),
        ContractError::NotPending { id: 1 }
    ));
    // reject path returns the reservation
    t.env.block.time = Timestamp::from_seconds(101 * DAY);
    t.pay(&v, 160, "BIG-2").unwrap();
    t.exec(&o, ExecuteMsg::Reject { id: 2 }, &[]).unwrap();
    assert_eq!(t.whois().owner_balance, Uint128::new(820));
}

#[test]
fn suspended_owner_can_reject_but_not_approve() {
    let mut t = setup();
    let (v, o) = (t.vendor.clone(), t.owner.clone());
    t.pay(&v, 180, "BIG-1").unwrap();
    t.pay(&v, 100, "BIG-2").unwrap(); // under approve_above: paid, not pending
    t.env.block.time = Timestamp::from_seconds(101 * DAY);
    t.pay(&v, 170, "BIG-3").unwrap();
    t.set_did(&o, "DID_STATUS_INACTIVE", "KYC_LEVEL_TWO");
    assert!(matches!(
        t.exec(&o, ExecuteMsg::Approve { id: 1 }, &[]).unwrap_err(),
        ContractError::MeIdInactive { .. }
    ));
    let res = t.exec(&o, ExecuteMsg::Reject { id: 3 }, &[]).unwrap();
    assert!(sent(&res).is_empty());
    t.set_did(&o, "DID_STATUS_ACTIVE", "KYC_LEVEL_TWO");
    let res = t.exec(&o, ExecuteMsg::Approve { id: 1 }, &[]).unwrap();
    assert_eq!(sent(&res), vec![(v.to_string(), 180)]);
}

#[test]
fn revoke_expiry_and_ownership() {
    let mut t = setup();
    let (v, o, s, a) = (
        t.vendor.clone(),
        t.owner.clone(),
        t.stranger.clone(),
        t.agent.clone(),
    );
    // someone else cannot revoke or steal the agent
    assert_eq!(
        t.exec(&s, ExecuteMsg::RevokeAgent { agent: a.to_string() }, &[])
            .unwrap_err(),
        ContractError::Unauthorized {}
    );
    t.set_did(&s, "DID_STATUS_ACTIVE", "KYC_LEVEL_THREE");
    let p = t.policy();
    assert!(matches!(
        t.exec(
            &s,
            ExecuteMsg::RegisterAgent {
                agent: a.to_string(),
                policy: p
            },
            &[]
        )
        .unwrap_err(),
        ContractError::AgentTaken { .. }
    ));
    t.exec(&o, ExecuteMsg::RevokeAgent { agent: a.to_string() }, &[])
        .unwrap();
    assert_eq!(t.pay(&v, 1, "R").unwrap_err(), ContractError::Revoked {});
    let p = t.policy();
    t.exec(
        &o,
        ExecuteMsg::UpdatePolicy {
            agent: a.to_string(),
            policy: p,
        },
        &[],
    )
    .unwrap();
    t.env.block.time = Timestamp::from_seconds(t.env.block.time.seconds() + 31 * DAY);
    assert_eq!(t.pay(&v, 1, "R").unwrap_err(), ContractError::Expired {});
}

#[test]
fn agent_cannot_attach_funds_or_drain_via_withdraw() {
    let mut t = setup();
    let (v, a) = (t.vendor.clone(), t.agent.clone());
    assert!(t
        .exec(
            &a,
            ExecuteMsg::Pay {
                vendor: v.to_string(),
                amount: Uint128::new(1),
                invoice: "X".into(),
                memo: None
            },
            &coins(5, "umec")
        )
        .is_err());
    // withdraw is for owners who deposited; the agent is refused outright
    assert!(matches!(
        t.exec(&a, ExecuteMsg::Withdraw { amount: Uint128::new(1) }, &[])
            .unwrap_err(),
        ContractError::Unauthorized {}
    ));
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
        (v.clone(), 10, "OK1"),
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
}

#[test]
fn batch_uses_me_multi_send() {
    let mut t = setup();
    let mut p = t.policy();
    p.any_verified_vendor = true;
    let (o, a, v, shop) = (
        t.owner.clone(),
        t.agent.clone(),
        t.vendor.clone(),
        t.shop.clone(),
    );
    t.exec(
        &o,
        ExecuteMsg::UpdatePolicy {
            agent: a.to_string(),
            policy: p,
        },
        &[],
    )
    .unwrap();
    let res = t
        .exec(
            &a,
            ExecuteMsg::PayBatch {
                payments: vec![
                    PayItem {
                        vendor: v.to_string(),
                        amount: Uint128::new(40),
                        invoice: "B1".into(),
                    },
                    PayItem {
                        vendor: shop.to_string(),
                        amount: Uint128::new(60),
                        invoice: "B2".into(),
                    },
                ],
            },
            &[],
        )
        .unwrap();
    let msg = &res.messages[0].msg;
    let json = String::from_utf8(to_json_vec(msg).unwrap()).unwrap();
    assert!(json.starts_with(r#"{"custom":{"multi_send":{"amount":[{"denom":"umec","amount":"100"}]"#), "{json}");
    assert_eq!(t.whois().spent_today, Uint128::new(100));
    // the same invoice twice inside one batch is refused
    let err = t
        .exec(
            &a,
            ExecuteMsg::PayBatch {
                payments: vec![
                    PayItem {
                        vendor: v.to_string(),
                        amount: Uint128::new(1),
                        invoice: "D".into(),
                    },
                    PayItem {
                        vendor: v.to_string(),
                        amount: Uint128::new(1),
                        invoice: "D".into(),
                    },
                ],
            },
            &[],
        )
        .unwrap_err();
    assert!(matches!(err, ContractError::DuplicateInvoice { .. }));
    // and a batch can never smuggle a payment that needs approval
    let err = t
        .exec(
            &a,
            ExecuteMsg::PayBatch {
                payments: vec![PayItem {
                    vendor: v.to_string(),
                    amount: Uint128::new(180),
                    invoice: "E".into(),
                }],
            },
            &[],
        )
        .unwrap_err();
    assert!(matches!(err, ContractError::Invalid { .. }));
}
