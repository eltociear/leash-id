// POST /api/attack { preset: "<id>" } or { tool, args } -> what the chain did with it.
import { body, json, limited, presets, run } from "./_sandbox.mjs";

const TOOLS = new Set(["pay_invoice", "pay_batch", "approve_payment", "withdraw_budget", "raise_my_limit"]);

export default async function handler(req, res) {
  if (req.method !== "POST") return json(res, 405, { error: "POST only" });
  const why = limited(req);
  if (why) return json(res, 429, { error: why });
  try {
    const b = await body(req);
    let tool = b.tool, args = b.args ?? {}, label = "custom";
    if (b.preset) {
      const p = (await presets()).find((x) => x.id === b.preset);
      if (!p) return json(res, 400, { error: "unknown preset" });
      ({ tool, args, label } = p);
    }
    if (!TOOLS.has(tool)) return json(res, 400, { error: "unknown tool" });
    json(res, 200, { label, ...(await run(tool, args)) });
  } catch (e) {
    json(res, 503, { error: `couldn't reach ME Hub: ${e.message.slice(0, 200)}` });
  }
}
