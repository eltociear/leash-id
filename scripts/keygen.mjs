// Moved: `node leash.mjs keygen [role…]` (one key file per role). Kept so old instructions still work.
process.argv.splice(2, 0, "keygen");
await import("../leash.mjs");
