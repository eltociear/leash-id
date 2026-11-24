// Bundled into docs/vendor/cosmjs.js (scripts/bundle-web.sh) so the pages never load code from a CDN.
export { SigningCosmWasmClient, CosmWasmClient } from "@cosmjs/cosmwasm-stargate";
export { GasPrice } from "@cosmjs/stargate";
export { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
// x/feegrant: the owner pays an agent's fees (contract calls only), so the agent key can hold no MEC.
export { BasicAllowance, AllowedMsgAllowance } from "cosmjs-types/cosmos/feegrant/v1beta1/feegrant";
