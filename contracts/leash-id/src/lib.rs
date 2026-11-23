pub mod contract;
pub mod did;
pub mod error;
pub mod msg;
pub mod state;

#[cfg(test)]
mod tests;

#[cfg(not(feature = "library"))]
pub mod entry {
    use crate::contract::{self, Res};
    use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, StdResult};

    #[entry_point]
    pub fn instantiate(deps: DepsMut, env: Env, info: MessageInfo, msg: InstantiateMsg) -> Res {
        contract::instantiate(deps, env, info, msg)
    }

    #[entry_point]
    pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> Res {
        contract::execute(deps, env, info, msg)
    }

    #[entry_point]
    pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
        contract::query(deps, env, msg)
    }
}
