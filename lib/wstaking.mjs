// ME Hub x/wstaking fixed deposits: the MEC an owner can show as a bond (`set_bond`).
//
// cosmjs has no types for ME Hub's own modules, so MsgDoFixedDeposit is encoded here by hand from
// me-hub proto/metaearth/wstaking/tx.proto:
//   message MsgDoFixedDeposit { string account = 1; cosmos.base.v1beta1.Coin principal = 2; int64 term = 4; }
// `term` is in days. A deposit needs the account's ME ID, at least 0.01 MEC, and a term its KYC region offers
// (GET /metaearth/wstaking/fixed_deposit_cfg; on testnet only region me_earth, 1 day).
import { BinaryReader, BinaryWriter } from "cosmjs-types/binary";
import { Coin } from "cosmjs-types/cosmos/base/v1beta1/coin";
import { NET, retry } from "./chain.mjs";

const rest = (path) => retry(() => fetch(`${NET.rest}${path}`).then((r) => r.json()));

export const DO_FIXED_DEPOSIT = "/metaearth.wstaking.MsgDoFixedDeposit";

export const MsgDoFixedDeposit = {
  encode(m, w = BinaryWriter.create()) {
    if (m.account) w.uint32(10).string(m.account);
    if (m.principal) Coin.encode(m.principal, w.uint32(18).fork()).ldelim();
    if (m.term) w.uint32(32).int64(m.term);
    return w;
  },
  decode(input, length) {
    const r = input instanceof BinaryReader ? input : new BinaryReader(input);
    const end = length === undefined ? r.len : r.pos + length;
    const m = { account: "", principal: undefined, term: 0n };
    while (r.pos < end) {
      const tag = r.uint32();
      if (tag >>> 3 === 1) m.account = r.string();
      else if (tag >>> 3 === 2) m.principal = Coin.decode(r, r.uint32());
      else if (tag >>> 3 === 4) m.term = r.int64();
      else r.skipType(tag & 7);
    }
    return m;
  },
  fromPartial: (o) => ({ account: o.account ?? "", principal: o.principal ? Coin.fromPartial(o.principal) : undefined, term: BigInt(o.term ?? 0) }),
};

export const fixedDepositMsg = (account, umec, days) => ({
  typeUrl: DO_FIXED_DEPOSIT,
  value: MsgDoFixedDeposit.fromPartial({ account, principal: { denom: NET.denom, amount: String(umec) }, term: days }),
});

/** The new deposit's id, from the `fixed_deposit` event of the tx that made it. */
export function depositIdOf(events = []) {
  for (const e of events) {
    if (e.type !== "fixed_deposit") continue;
    const id = e.attributes.find((a) => a.key === "id")?.value;
    if (id) return Number(id);
  }
  return undefined;
}

/** Terms (days) and yearly rates the region offers, e.g. [{term: 1, rate: "0.3"}]. Empty when it offers none. */
export async function depositTerms(regionId) {
  const j = await rest("/metaearth/wstaking/fixed_deposit_cfg");
  const r = (j.RegionFixedDepositCfgs ?? []).find((x) => x.regionId === regionId);
  return (r?.RegionFixedDepositCfg ?? []).filter((c) => c.status === "FIXED_DEPOSIT_CFG_ACTIVE").map((c) => ({ term: Number(c.term), rate: c.rate }));
}

/** The KYC region of an ME ID (x/kyc stores it as the KYC record's data). */
export async function regionOf(did) {
  const j = await rest(`/metaearth/kyc/KYC?did=${encodeURIComponent(did)}`);
  return j?.kyc?.data ? Buffer.from(j.kyc.data, "base64").toString("utf8") : undefined;
}

/** An account's deposits that haven't unlocked yet. */
export async function openDeposits(account) {
  const j = await rest(`/metaearth/wstaking/fixed_deposit_by_acct/${account}/NOT_EXPIRED`);
  return j?.FixedDeposit ?? j?.fixedDeposit ?? j?.fixed_deposits ?? [];
}
