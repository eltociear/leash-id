//! ME Hub fixed deposits, read from inside the contract.
//!
//! `/metaearth.wstaking.Query/FixedDeposit` is one of the three Stargate queries ME Hub whitelists for
//! CosmWasm (app/keepers/wasm_me_query.go). A fixed deposit is MEC an ME ID holder has locked with their
//! region until a fixed date; Leash ID shows it as the owner's bond: money they can't move before then.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{from_json, to_json_vec, Binary, ContractResult, Deps, QueryRequest, SystemResult, Uint128};
use serde::Deserialize;

use crate::did::{fields, put_varint};

pub const FIXED_DEPOSIT_QUERY_PATH: &str = "/metaearth.wstaking.Query/FixedDeposit";

#[cw_serde]
pub struct Deposit {
    pub id: u64,
    pub account: String,
    pub denom: String,
    pub principal: Uint128,
    /// Unix seconds when the deposit unlocks.
    pub end_time: u64,
}

/// QueryGetFixedDepositRequest { string address = 1; uint64 id = 2; }
pub fn encode_query(address: &str, id: u64) -> Binary {
    let a = address.as_bytes();
    let mut out = Vec::with_capacity(a.len() + 12);
    out.push(0x0a);
    put_varint(&mut out, a.len() as u64);
    out.extend_from_slice(a);
    out.push(0x10);
    put_varint(&mut out, id);
    Binary::from(out)
}

pub fn fixed_deposit(deps: Deps, address: &str, id: u64) -> Option<Deposit> {
    let req: QueryRequest<cosmwasm_std::Empty> = QueryRequest::Stargate {
        path: FIXED_DEPOSIT_QUERY_PATH.to_string(),
        data: encode_query(address, id),
    };
    let bin = to_json_vec(&req).ok()?;
    match deps.querier.raw_query(&bin) {
        SystemResult::Ok(ContractResult::Ok(raw)) => parse_response(raw.as_slice()),
        _ => None,
    }
}

pub fn parse_response(raw: &[u8]) -> Option<Deposit> {
    parse_json(raw).or_else(|| parse_proto(raw))
}

#[derive(Deserialize)]
struct JsonResp {
    #[serde(rename = "FixedDeposit", alias = "fixedDeposit", alias = "fixed_deposit")]
    fd: JsonFd,
}

#[derive(Deserialize)]
struct JsonCoin {
    #[serde(default)]
    denom: String,
    #[serde(default)]
    amount: String,
}

#[derive(Deserialize)]
struct JsonFd {
    /// proto-JSON writes uint64 as a string. (No untagged enums here: they pull float code into the wasm,
    /// which ME Hub's wasmvm rejects.)
    #[serde(default)]
    id: String,
    #[serde(default)]
    account: String,
    principal: Option<JsonCoin>,
    #[serde(default, alias = "endTime")]
    end_time: String,
}

fn parse_json(raw: &[u8]) -> Option<Deposit> {
    let r: JsonResp = from_json(raw).ok()?;
    let p = r.fd.principal?;
    if r.fd.account.is_empty() {
        return None;
    }
    Some(Deposit {
        id: r.fd.id.parse().unwrap_or(0),
        account: r.fd.account,
        denom: p.denom,
        principal: p.amount.parse::<u128>().ok()?.into(),
        end_time: rfc3339_seconds(&r.fd.end_time)?,
    })
}

/// QueryGetFixedDepositResponse { FixedDeposit FixedDeposit = 1 }
/// FixedDeposit { uint64 id = 1; string account = 2; Coin principal = 3; ...; Timestamp end_time = 6; }
fn parse_proto(raw: &[u8]) -> Option<Deposit> {
    let fd = fields(raw)?.into_iter().find(|(f, w, _, _)| *f == 1 && *w == 2)?.2;
    let mut d = Deposit { id: 0, account: String::new(), denom: String::new(), principal: Uint128::zero(), end_time: 0 };
    for (f, w, s, v) in fields(fd)? {
        match (f, w) {
            (1, 0) => d.id = v,
            (2, 2) => d.account = String::from_utf8(s.to_vec()).ok()?,
            (3, 2) => {
                for (cf, cw, cs, _) in fields(s)? {
                    match (cf, cw) {
                        (1, 2) => d.denom = String::from_utf8(cs.to_vec()).ok()?,
                        (2, 2) => d.principal = String::from_utf8(cs.to_vec()).ok()?.parse::<u128>().ok()?.into(),
                        _ => {}
                    }
                }
            }
            (6, 2) => {
                for (tf, tw, _, tv) in fields(s)? {
                    if (tf, tw) == (1, 0) {
                        d.end_time = tv;
                    }
                }
            }
            _ => {}
        }
    }
    if d.account.is_empty() {
        None
    } else {
        Some(d)
    }
}

/// "2026-04-16T08:18:12.619173855Z" -> unix seconds. UTC ("Z") or a numeric offset; fractions ignored.
pub fn rfc3339_seconds(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { std::str::from_utf8(&b[r]).ok()?.parse().ok() };
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).map_or(false, |c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let offset = match b.get(i)? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let oh = num(i + 1..i + 3)?;
            let om = num(i + 4..i + 6)?;
            let o = oh * 3600 + om * 60;
            if *sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    // days from civil (Howard Hinnant's algorithm)
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let t = days * 86400 + h * 3600 + mi * 60 + se - offset;
    u64::try_from(t).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_rest_shape() {
        // From GET /metaearth/wstaking/fixed_deposit on mechain_400-1.
        let j = br#"{"FixedDeposit":{"id":"2","account":"me1wdywsn9emstjgz4tqe2vnmea82mjtalsxx42ns","principal":{"denom":"umec","amount":"6000000000"},"interest":{"denom":"umec","amount":"4931506"},"start_time":"2026-04-15T08:18:12.619173855Z","end_time":"2026-04-16T08:18:12.619173855Z","term":"1","rate":"0.300000000000000000"}}"#;
        let d = parse_response(j).unwrap();
        assert_eq!(d.id, 2);
        assert_eq!(d.account, "me1wdywsn9emstjgz4tqe2vnmea82mjtalsxx42ns");
        assert_eq!((d.denom.as_str(), d.principal.u128()), ("umec", 6_000_000_000));
        assert_eq!(d.end_time, 1_776_327_492); // 2026-04-16T08:18:12Z
    }

    #[test]
    fn parses_proto_shape() {
        let coin = [&[0x0a, 4][..], b"umec", &[0x12, 3], b"500"].concat();
        let ts = [0x08, 0x80, 0xa3, 0x05]; // seconds = 86400
        let fd = [&[0x08, 7, 0x12, 3][..], b"me1", &[0x1a, coin.len() as u8], &coin, &[0x32, ts.len() as u8], &ts].concat();
        let raw = [&[0x0a, fd.len() as u8][..], &fd].concat();
        let d = parse_response(&raw).unwrap();
        assert_eq!((d.id, d.account.as_str(), d.denom.as_str(), d.principal.u128(), d.end_time), (7, "me1", "umec", 500, 86400));
    }

    #[test]
    fn rfc3339() {
        assert_eq!(rfc3339_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_seconds("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(rfc3339_seconds("2026-04-16T17:18:12+09:00"), Some(1_776_327_492));
        assert_eq!(rfc3339_seconds("1969-12-31T23:59:59Z"), None);
        assert_eq!(rfc3339_seconds("2026-13-01T00:00:00Z"), None);
        assert_eq!(rfc3339_seconds("garbage"), None);
        assert!(parse_response(br#"{"FixedDeposit":{"account":"","principal":{"denom":"umec","amount":"1"},"end_time":"2026-01-01T00:00:00Z"}}"#).is_none());
    }
}
