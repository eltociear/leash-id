# Leash ID：为 AI 代理加上由 ME ID 担保的支付“牵引绳”

[English](README.md) · **简体中文** · [日本語](README.ja.md)

**演示视频（4:48，英文字幕）：** https://www.youtube.com/watch?v=vySdFuWiGho · **查询代理（无需钱包）：** https://eltociear.github.io/leash-id/ ·
**所有者控制台（Keplr）：** https://eltociear.github.io/leash-id/owner.html ·
**[威胁模型](THREAT_MODEL.md)** · **[安全与部署](SECURITY.md)**

AI 代理已经开始替企业付账单、买服务。但有两件事还缺：

1. **对方不知道代理背后是谁。** 一个地址就只是一个地址。
2. **所有者无法确保代理不会超支。** 要么把私钥交出去，要么每一笔都人工批准。

**Leash ID** 是部署在 **ME Hub** 上的 CosmWasm 合约。它把每个代理绑定到一个**实时有效的 ME ID**
（ME Network 原生的 KYC 身份），并在链上强制执行支出规则。配套的 MCP 服务器让 Claude 或其他代理在这些规则内处理应付账款。

## 为什么现在需要

- **应付账款是资金被盗的重灾区。** 2024 年商业邮件欺诈（BEC）投诉 21,442 起，损失近 28 亿美元；2022–2024 年合计近 85 亿美元
  （FBI IC3，经 [Nacha](https://www.nacha.org/news/fbis-ic3-finds-almost-85-billion-lost-business-email-compromise-last-three-years)）。
  读发票的 AI 代理会继承同样的攻击：一封足够逼真的邮件就能让它转账。
- **卡组织正在建设“代理验证”。** Visa 的 Trusted Agent Protocol、Mastercard 的 Agent Pay 与 Verifiable Intent，都是卡网络内的凭证。
  在链上，没有人为代理担保。
- **链上的代理支付分不清是谁在付钱。** “定时脚本和代理留下的链上记录完全一样”
  （[TRM Labs，2026 年 9 月](https://www.trmlabs.com/trm-tech-blog/whos-actually-paying-measuring-ai-agent-payments-onchain)）。

## 每一笔支付，合约内完成的检查

1. 所有者的 ME ID 是否仍为 **ACTIVE**，KYC 等级是否 ≥ 要求（在合约内通过 Stargate 查询 `/metaearth.did.Query/Did`，无需预言机）。
2. 收款方在白名单内，或本身持有有效 ME ID（包括通过 x/did 绑定的 EVM 子账户）。
3. 单笔上限、**任意 24 小时**累计上限、所有者预算、有效期。
4. 同一张发票不会被重复支付（忽略大小写和分隔符，拒绝外观相似的 Unicode 字符）。
5. 超过阈值的支付进入待审批，由所有者批准；批准时会重新检查所有者与收款方。

**ME ID 即“紧急开关”：** 全局 DAO 一旦暂停所有者的 ME ID，其名下所有代理在下一个区块就无法付款，无需任何人手动撤销。
我们在由 `openmetaearth/me-hub` 源码构建的本地节点上实际演示了这一点，CI 在每次提交时都会重新验证。

## 充分利用 ME Network

ME Hub 只向 CosmWasm 开放三个 Stargate 查询和一个自定义消息。Leash ID 用到了其中所有有意义的部分：

- `Did`：每次付款、存款、要约/接受、批准时实时读取 ME ID。
- `FixedDeposit`：所有者可以把 ME Hub 定期存款展示为“保证金”，合约实时读取（到期或取出即失效）。
- `multi_send`：批量付款通过 ME Hub 自定义消息一次结算。
- `Region` 查询返回地区信息，但 x/did 的 `regionId` 在 me-hub 中标注为未使用，因此无法据此判断账户所属地区，我们没有使用。

## 面向卖家：只接受“有人担保”的代理付款（HTTP 402）

```js
import { paywall } from "leash-id/vendor";
http.createServer(paywall({ payTo: "me1你的地址…", priceUmec: "100000", minKyc: 2 }, (req, res, paid) => {
  res.end(JSON.stringify({ data: "…", served_to_owner_me_id: paid.owner_did }));
})).listen(8080);
```

代理先收到 `402 Payment Required` 和条款（每次请求签发一张带有效期和签名的新发票），通过 Leash ID 付款后，带上
`X-Leash-Payment: <交易哈希>` 和 `X-Leash-Proof`（代理私钥对"交易哈希 + 发票"的 ADR-36 签名）重试。
交易哈希是公开的，签名证明保证别人无法抢先兑现这笔付款。服务器从链上读取该笔付款，确认发票是自己签发且仍有效、
未被重复使用，并且付款代理背后有实时有效的 ME ID。我们的 MCP 工具 `pay_for_api` 负责代理一侧：只向持有 ME ID 的卖家付款，并设有价格上限。

## 面向所有者

- 浏览器控制台（Keplr）：审批或拒绝待审批付款、一键撤销代理、修改额度和收款方、导出 CSV。
- **多人审批**：可设置最多 5 名共同审批人（每人都需持有有效 ME ID），由 N 个签名（含所有者）放行一笔待审批付款，任何一人都可以拒绝；放行时重新核验每个签名人的 ME ID。
- `leash watch`：有付款进入待审批时，几秒内推送到 Slack 等 Webhook。

## 安全设计要点

- 合约**没有管理员**，任何人都无法替换代码；CI 会逐字节重建 wasm 并与链上代码比对（`npm run verify`）。
- MCP 服务器只加载代理私钥；如果其密钥目录中的其他 Leash ID 私钥文件对该进程可读，它会拒绝启动。请让代理以独立用户或容器运行。
- 每笔交易在广播前就计算好哈希：超时只会报告为“未知，请先查询”，不会导致重复付款。
- 所有拒绝都先在模拟中完成，不收手续费。

## 在 ME Hub 测试网上，使用真实的 ME ID

我们的所有者持有测试网 ME ID（`2611255912050`，KYC 等级 2，由测试网 KYC 签发方签发）。`scripts/testnet-pay.sh`
用它记录完整的支付流程（[`runs/testnet-payments-2026-11-25T16-54-25Z.txt`](runs/testnet-payments-2026-11-25T16-54-25Z.txt)），每一步都是可查询的交易：KYC 等级 1 的真实 ME ID 无法为代理注资（链上拒绝）；
限额内支付成功；重复发票、超额、无 ME ID 的收款方、KYC 等级 1 的收款方均被拒绝；向绑定到他人 KYC 2 ME ID 的 EVM
子账户付款，收据记录该 ME ID；超过阈值的付款由所有者批准；`multi_send` 批量支付；撤销后被拒、恢复后可付；从未持有
MEC 的代理密钥依靠所有者的手续费授权（x/feegrant）付款，且无法转出任何其他资金。

测试网上还记录了：Claude Code 作为代理（`runs/testnet-claude-*.jsonl`）；“服从一切”的客户端（[`runs/testnet-obey-2026-11-25T17-02-39-964Z.txt`](runs/testnet-obey-2026-11-25T17-02-39-964Z.txt)），
8 次攻击全部在链上被拒，拆分付款被 24 小时限额拦下（低于审批阈值时，24 小时限额是上限而不是阻止：之前一次限额为 0.2 MEC
的运行中，同样的拆分付款付给了所有者自己的收款方，见 THREAT_MODEL T6）；以及代理之间的交易
（`runs/testnet-commerce-*.txt`）。

## 快速开始

```bash
npm install -g github:eltociear/leash-id
leash keygen owner agent
leash deposit 0.5
leash offer agent --per-tx 0.02 --daily 0.05 --approve-above 0.015 --vendors me1供应商… --any-verified-vendor yes
leash accept me1所有者… --key-file /run/secrets/agent.mnemonic   # 在代理自己的主机上执行
leash whois me1代理…
```

更多细节（合约接口、MCP 工具、测试与 CI）请见英文 [README](README.md)。Apache-2.0 许可。
