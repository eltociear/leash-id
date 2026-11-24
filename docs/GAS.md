# Gas per operation

Measured on a local ME Hub node (me-hub 1e9eff8a, wasmd 0.43) with the canonical wasm, 2026-11-24.
Fee = gas × 0.1umec (testnet's price). The chain charges for the gas a tx *requests*; our senders request simulated gas × 1.4, so the fee actually paid is about 1.4× the column.

| Operation | Gas used | Fee |
|---|---:|---:|
| deposit | 196,308 | 0.00019631 MEC |
| update_policy | 210,854 | 0.00021086 MEC |
| pay (allow-listed vendor, paid) | 260,513 | 0.00026052 MEC |
| pay (any-verified vendor: 2nd ME ID lookup) | 276,294 | 0.0002763 MEC |
| pay (held for approval) | 250,662 | 0.00025067 MEC |
| approve (owner alone) | 225,980 | 0.00022598 MEC |
| pay (held, to reject) | 250,665 | 0.00025067 MEC |
| reject | 197,458 | 0.00019746 MEC |
| pay_batch (5 items, one multi_send) | 503,178 | 0.00050318 MEC |
| pay_batch (20 items, the maximum) | 1,411,060 | 0.00141106 MEC |
| register_sub_account | 195,690 | 0.00019569 MEC |
| set_approvers (1 co-approver) | 196,313 | 0.00019632 MEC |
| revoke_agent | 195,857 | 0.00019586 MEC |
| resume_agent | 188,435 | 0.00018844 MEC |

A refused payment costs nothing: it is caught in simulation and never broadcast.
The largest message, a 20-item batch, stays far below the block gas limit (300,000,000 on our node; unlimited, -1, on testnet).
