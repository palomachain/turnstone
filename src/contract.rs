use crate::helpers::de::KeyDeserialize;
use crate::msg::{ConsensusMsg, ExecuteMsg, InstantiateMsg, JobId, JobInfo, QueryMsg, QueryResult};
use crate::state::{BALANCES, BALANCES_BY_JOB_ID};
use crate::validation::{validate_json, Validator, TRUSTED_ADDRESSES, VALIDATORS};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Order,
    Response, Uint128,
};
use cw2::set_contract_version;
use eyre::{ensure, Result};
use std::collections::HashMap;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response> {
    const CONTRACT_NAME: &str = "crates.io:turnstone";
    const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let (mut validators, mut addresses): (Vec<_>, Vec<_>) = msg
        .validators
        .into_iter()
        .map(|val| {
            (
                Validator {
                    pubkey: val.public_key,
                    stake: val.stake,
                },
                val.address,
            )
        })
        .unzip();
    addresses.sort();
    validators.sort_by(|v1, v2| v1.pubkey.cmp(&v2.pubkey));
    TRUSTED_ADDRESSES.save(deps.storage, &addresses)?;
    VALIDATORS.save(deps.storage, &validators)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("from", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(deps: DepsMut, _env: Env, info: MessageInfo, msg: ExecuteMsg) -> Result<Response> {
    match msg {
        ExecuteMsg::Deposit { job_id } => execute_deposit(deps, info, job_id),
        ExecuteMsg::Withdraw { withdraw_info } => execute_withdraw(deps, info, withdraw_info),
        ExecuteMsg::WithConsensus {
            raw_json,
            signatures,
        } => match validate_json(deps.as_ref(), &info, &raw_json, &signatures)? {
            ConsensusMsg::None => {
                // TODO: update_valset
                // TODO: execute_external_contract https://github.com/palomachain/paloma/issues/109
                Ok(Response::new())
            }
        },
    }
}

fn execute_deposit(deps: DepsMut, info: MessageInfo, job_id: JobId) -> Result<Response> {
    let MessageInfo { sender, funds } = info;
    let mut res = Response::new().add_attribute("method", "deposit");
    let mut nonzero_funds = false;
    for coin in funds.into_iter() {
        nonzero_funds = nonzero_funds || !coin.amount.is_zero();
        BALANCES.update(
            deps.storage,
            (&sender, &job_id, &coin.denom),
            |balance| -> Result<Uint128> {
                Ok(match balance {
                    Some(balance) => balance + coin.amount,
                    None => coin.amount,
                })
            },
        )?;
        BALANCES_BY_JOB_ID.save(deps.storage, (&job_id, &sender, &coin.denom), &())?;
        res = res
            .add_attribute("job_id", &job_id.0)
            .add_attribute("denom", &coin.denom)
            .add_attribute("amount", coin.amount);
    }
    ensure!(nonzero_funds, "attempting to deposit 0 funds");

    Ok(res)
}

fn execute_withdraw(deps: DepsMut, info: MessageInfo, withdraws: Vec<JobInfo>) -> Result<Response> {
    ensure!(!withdraws.is_empty(), "must execute some withdrawal");
    let mut res = Response::new().add_attribute("method", "withdraw");
    let mut coins = Vec::with_capacity(withdraws.len());
    for withdraw in withdraws {
        let amount = BALANCES
            .may_load(
                deps.storage,
                (&info.sender, &withdraw.job_id, &withdraw.coin.denom),
            )?
            .unwrap_or_default();
        let amount = amount.checked_sub(withdraw.coin.amount)?;
        if amount.is_zero() {
            BALANCES.remove(
                deps.storage,
                (&info.sender, &withdraw.job_id, &withdraw.coin.denom),
            );
            BALANCES_BY_JOB_ID.remove(
                deps.storage,
                (&withdraw.job_id, &info.sender, &withdraw.coin.denom),
            );
        } else {
            BALANCES.save(
                deps.storage,
                (&info.sender, &withdraw.job_id, &withdraw.coin.denom),
                &amount,
            )?;
        }
        res = res
            .add_attribute("job_id", withdraw.job_id.0)
            .add_attribute("denom", &withdraw.coin.denom)
            .add_attribute("amount", amount);
        coins.push(Coin {
            amount,
            denom: withdraw.coin.denom.clone(),
        });
    }
    Ok(res.add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: coins,
    })))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary> {
    Ok(to_binary(&match msg {
        QueryMsg::GetDepositInfo { address } => {
            QueryResult::Jobs(query_address_info(deps, &address)?)
        }
        QueryMsg::GetJobInfo { job_id } => QueryResult::Balance(query_job_info(deps, &job_id)?),
    })?)
}

/// Fetch the coins associated to every job under the given address.
fn query_address_info(deps: Deps, address: &Addr) -> Result<Vec<JobInfo>> {
    BALANCES
        .sub_prefix(address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (key, amount) = item?;
            let (job_id, denom) = <(JobId, String)>::from_slice(&key)?;
            Ok(JobInfo {
                coin: Coin { denom, amount },
                job_id,
            })
        })
        .collect()
}

/// Fetch the funds associated with a given `JobId`, summed by denomination.
fn query_job_info(deps: Deps, job_id: &JobId) -> Result<Vec<Coin>> {
    let mut balance: HashMap<String, Uint128> = HashMap::new();
    for key in
        BALANCES_BY_JOB_ID
            .sub_prefix(job_id)
            .keys(deps.storage, None, None, Order::Ascending)
    {
        let (address, denom) = <(Addr, String)>::from_vec(key)?;
        let amount = BALANCES.load(deps.storage, (&address, job_id, &denom))?;
        *balance.entry(denom).or_default() += amount;
    }
    Ok(balance
        .into_iter()
        .map(|(denom, amount)| Coin { amount, denom })
        .collect())
}
