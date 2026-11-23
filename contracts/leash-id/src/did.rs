//! ME ID lookup from inside the contract.
//!
//! ME Hub whitelists exactly one identity query for CosmWasm Stargate calls:
//! `/metaearth.did.Query/Did` (see me-hub app/keepers/wasm_me_query.go). The
//! request is the protobuf `QueryDid { string address = 1; }`. wasmd returns
//! the response as proto-JSON; we also accept raw protobuf bytes so the
//! contract keeps working if a node returns the binary form.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{from_json, to_json_vec, Binary, ContractResult, Deps, QueryRequest, SystemResult};
use serde::Deserialize;

pub const DID_QUERY_PATH: &str = "/metaearth.did.Query/Did";

/// The parts of `metaearth.did.DidInfo` the leash cares about.
#[cw_serde]
pub struct MeId {
    /// The ME ID number (e.g. "0000000000001").
    pub did: String,
    /// The main (KYC-bound) account.
    pub address: String,
    /// `DID_STATUS_ACTIVE` == 1.
    pub active: bool,
    /// `KYC_LEVEL_*` as a number (0 = none … 5).
    pub kyc_level: u8,
}

pub fn encode_query_did(address: &str) -> Binary {
    let a = address.as_bytes();
    let mut out = Vec::with_capacity(a.len() + 3);
    out.push(0x0a); // field 1, wire type 2
    put_varint(&mut out, a.len() as u64);
    out.extend_from_slice(a);
    Binary::from(out)
}

/// Live ME ID for `address`, or `None` when the chain has no DID for it.
pub fn lookup(deps: Deps, address: &str) -> Option<MeId> {
    let req: QueryRequest<cosmwasm_std::Empty> = QueryRequest::Stargate {
        path: DID_QUERY_PATH.to_string(),
        data: encode_query_did(address),
    };
    // Raw bytes on purpose: wasmd hands back the proto-JSON document itself, not a
    // JSON-encoded Binary, so `querier.query::<Binary>` would fail to decode it.
    let bin = to_json_vec(&req).ok()?;
    match deps.querier.raw_query(&bin) {
        SystemResult::Ok(ContractResult::Ok(raw)) => parse_response(raw.as_slice()),
        _ => None,
    }
}

pub fn parse_response(raw: &[u8]) -> Option<MeId> {
    if let Some(id) = parse_json(raw) {
        return Some(id);
    }
    parse_proto(raw)
}

#[derive(Deserialize)]
struct JsonResp {
    info: JsonInfo,
}

#[derive(Deserialize)]
struct JsonInfo {
    #[serde(default)]
    did: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "kycLevel")]
    kyc_level: Option<String>,
}

fn parse_json(raw: &[u8]) -> Option<MeId> {
    let r: JsonResp = from_json(raw).ok()?;
    if r.info.did.is_empty() {
        return None;
    }
    Some(MeId {
        did: r.info.did,
        address: r.info.address,
        active: status_from_str(r.info.status.as_deref().unwrap_or("")),
        kyc_level: level_from_str(r.info.kyc_level.as_deref().unwrap_or("")),
    })
}

fn status_from_str(s: &str) -> bool {
    matches!(s.trim(), "DID_STATUS_ACTIVE" | "1")
}

fn level_from_str(s: &str) -> u8 {
    match s.trim() {
        "KYC_LEVEL_ONE" | "1" => 1,
        "KYC_LEVEL_TWO" | "2" => 2,
        "KYC_LEVEL_THREE" | "3" => 3,
        "KYC_LEVEL_FOUR" | "4" => 4,
        "KYC_LEVEL_FIVE" | "5" => 5,
        _ => 0,
    }
}

// ---------- minimal protobuf reader (QueryDidResponse { DidInfo info = 1 }) ----------

fn get_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Iterates (field, wire, payload) over a protobuf message.
fn fields(b: &[u8]) -> Option<Vec<(u64, u64, &[u8], u64)>> {
    let mut i = 0;
    let mut out = vec![];
    while i < b.len() {
        let key = get_varint(b, &mut i)?;
        let (field, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                let v = get_varint(b, &mut i)?;
                out.push((field, wire, &b[0..0], v));
            }
            2 => {
                let len = get_varint(b, &mut i)? as usize;
                let end = i.checked_add(len)?;
                let s = b.get(i..end)?;
                i = end;
                out.push((field, wire, s, 0));
            }
            _ => return None,
        }
    }
    Some(out)
}

fn parse_proto(raw: &[u8]) -> Option<MeId> {
    let info = fields(raw)?
        .into_iter()
        .find(|(f, w, _, _)| *f == 1 && *w == 2)?
        .2;
    let mut id = MeId {
        did: String::new(),
        address: String::new(),
        active: false,
        kyc_level: 0,
    };
    for (f, w, s, v) in fields(info)? {
        match (f, w) {
            (1, 2) => id.did = String::from_utf8(s.to_vec()).ok()?,
            (2, 2) => id.address = String::from_utf8(s.to_vec()).ok()?,
            (4, 0) => id.active = v == 1,
            (6, 0) => id.kyc_level = v.min(5) as u8,
            _ => {}
        }
    }
    if id.did.is_empty() {
        None
    } else {
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query() {
        let b = encode_query_did("me1abc");
        assert_eq!(b.as_slice(), &[0x0a, 6, b'm', b'e', b'1', b'a', b'b', b'c']);
    }

    #[test]
    fn parses_live_rest_shape() {
        // Copied from GET /metaearth/did/did?address=… on mechain_400-1.
        let j = br#"{"info":{"did":"0000000000001","address":"me1q30g0n5fe3e2lgsxwsdgt8qn44ajjxseehxhzl","pubkey":"{}","status":"DID_STATUS_ACTIVE","regionId":"","kycLevel":"KYC_LEVEL_TWO","sub_account":""}}"#;
        let id = parse_response(j).unwrap();
        assert_eq!(id.did, "0000000000001");
        assert!(id.active);
        assert_eq!(id.kyc_level, 2);
    }

    #[test]
    fn parses_proto_shape() {
        let mut info = vec![0x0a, 2, b'4', b'2', 0x12, 3, b'm', b'e', b'1', 0x20, 1, 0x30, 3];
        let mut raw = vec![0x0a, info.len() as u8];
        raw.append(&mut info);
        let id = parse_response(&raw).unwrap();
        assert_eq!((id.did.as_str(), id.active, id.kyc_level), ("42", true, 3));
    }

    #[test]
    fn inactive_and_garbage() {
        let j = br#"{"info":{"did":"7","address":"me1x","status":"DID_STATUS_INACTIVE ","kycLevel":"KYC_LEVEL_NONE "}}"#;
        let id = parse_response(j).unwrap();
        assert!(!id.active);
        assert_eq!(id.kyc_level, 0);
        assert!(parse_response(b"\xff\xff").is_none());
        assert!(parse_response(br#"{"info":{"did":""}}"#).is_none());
    }
}
