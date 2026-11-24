// POST /api/attack { preset: "<id>" } or { tool, args } -> what the chain does with it.
import { badRequest, body, fail, json, limited, presets, run } from "./_sandbox.mjs";

const TOOLS = new Set(["pay_invoice", "pay_batch", "approve_payment", "withdraw_budget", "raise_my_limit"]);

export default async function handler(req, res) {
  const bad = badRequest(req);
  if (bad) return json(res, 400, { error: bad });
  try {
    const why = await limited(req, 1);
    if (why) return json(res, 429, { error: why });
    const b = await body(req);
    let tool = b.tool, args = b.args ?? {}, label = "custom", broadcast = false;
    if (b.preset) {
      const p = (await presets()).find((x) => x.id === b.preset);
      if (!p) return json(res, 400, { error: "unknown preset" });
      ({ tool, args, label } = p);
      broadcast = !!p.broadcast; // only the control payment is ever broadcast
    }
    if (!TOOLS.has(tool)) return json(res, 400, { error: "unknown tool" });
    json(res, 200, { label, ...(await run(tool, args, { broadcast })) });
  } catch (e) {
    fail(res, e);
  }
}
