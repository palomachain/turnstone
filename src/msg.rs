use crate::helpers::de::KeyDeserialize;
use cosmwasm_std::{Addr, Coin, StdResult};
use cw_storage_plus::{Prefixer, PrimaryKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct JobId(pub String);

// impl `PrimaryKey` and `Prefixer` for use as a key in `Map`s.
impl<'a> PrimaryKey<'a> for &'a JobId {
    type Prefix = ();
    type SubPrefix = ();

    fn key(&self) -> Vec<&[u8]> {
        vec![self.0.as_bytes()]
    }
}

impl<'a> Prefixer<'a> for &'a JobId {
    fn prefix(&self) -> Vec<&[u8]> {
        vec![self.0.as_bytes()]
    }
}

impl KeyDeserialize for JobId {
    type Output = JobId;

    fn from_vec(value: Vec<u8>) -> StdResult<Self::Output> {
        Ok(JobId(<String as KeyDeserialize>::from_vec(value)?))
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { job_id: JobId },
    Withdraw { withdraw_info: Vec<JobInfo> },
}

#[derive(Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetDepositInfo { address: Addr },
    GetJobInfo { job_id: JobId },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryResult {
    Jobs(Vec<JobInfo>),
    Balance(Vec<Coin>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct JobInfo {
    pub coin: Coin,
    pub job_id: JobId,
}
