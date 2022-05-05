use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Coin};
use cw_storage_plus::{Item, Map, U32Key};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub count: i32,
    pub owner: Addr,
}

pub const STATE: Item<State> = Item::new("state");

pub const DEPOSIT: Map<(&[u8], &[u8]), Coin> = Map::new("deposit");

pub const DEPOSIT_REVERSE: Map<(&[u8], &[u8]), bool> = Map::new("deposit_reverse");

pub const JOB_ADDR: Map<(&[u8], U32Key), Addr> = Map::new("job_addr");
