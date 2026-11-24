// GET /api/state -> the sandbox leash as the chain sees it, plus the canned attacks.
import { fail, json, presets, state } from "./_sandbox.mjs";

export default async function handler(req, res) {
  try {
    json(res, 200, { ...(await state()), presets: (await presets()).map(({ id, label, tool, args, expect }) => ({ id, label, tool, args, expect })) });
  } catch (e) {
    fail(res, e);
  }
}
