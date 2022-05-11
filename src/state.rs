use crate::msg::JobId;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Map;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub count: i32,
    pub owner: Addr,
}

/// Deposits indexed by `(address, job_id, denomination)`.
pub const BALANCES: Map<(&Addr, &JobId, &str), Uint128> = Map::new("balances");

/// A reverse index on [`BALANCES`].
pub const BALANCES_BY_JOB_ID: Map<(&JobId, &Addr, &str), ()> = Map::new("balances_by_job_id");
