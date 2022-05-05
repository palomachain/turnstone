#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Attribute, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::error::ContractError::Invalidate;
use crate::msg::{DepositInfo, ExecuteMsg, InstantiateMsg, JobInfo, QueryMsg};
use crate::state::{DEPOSIT, DEPOSIT_REVERSE};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:turnstone";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("from", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Deposit { job_id } => execute_deposit(deps, info, job_id),
        ExecuteMsg::Withdraw { withdraw_info } => execute_withdraw(deps, info, withdraw_info),
    }
}

fn execute_deposit(
    deps: DepsMut,
    info: MessageInfo,
    job_id: String,
) -> Result<Response, ContractError> {
    if info.funds.len() != 1 || info.funds[0].amount.is_zero() {
        return Err(Invalidate {});
    }

    if DEPOSIT.has(deps.storage, (info.sender.as_bytes(), job_id.as_bytes())) {
        return Err(Invalidate {});
    }

    DEPOSIT.save(
        deps.storage,
        (info.sender.as_bytes(), job_id.as_bytes()),
        &info.funds[0],
    )?;
    DEPOSIT_REVERSE.save(
        deps.storage,
        (job_id.as_bytes(), info.sender.as_bytes()),
        &true,
    )?;

    Ok(Response::new()
        .add_attribute("method", "deposit")
        .add_attribute("job_id", job_id)
        .add_attribute("denom", info.funds[0].denom.to_string())
        .add_attribute("amount", info.funds[0].amount.to_string()))
}

fn execute_withdraw(
    deps: DepsMut,
    info: MessageInfo,
    withdraw_info: Vec<DepositInfo>,
) -> Result<Response, ContractError> {
    if withdraw_info.is_empty() {
        return Err(Invalidate {});
    }
    let mut attrs = Vec::new();
    let mut coins = Vec::new();
    for withdraw_info in withdraw_info {
        let coin = DEPOSIT
            .may_load(
                deps.storage,
                (info.sender.as_bytes(), withdraw_info.job_id.as_bytes()),
            )?
            .unwrap_or_default();
        if coin.denom != withdraw_info.coin.denom {
            return Err(Invalidate {});
        }
        if coin.amount == withdraw_info.coin.amount {
            DEPOSIT.remove(
                deps.storage,
                (info.sender.as_bytes(), withdraw_info.job_id.as_bytes()),
            );
            DEPOSIT_REVERSE.remove(
                deps.storage,
                (withdraw_info.job_id.as_bytes(), info.sender.as_bytes()),
            );
        } else {
            DEPOSIT.update(
                deps.storage,
                (info.sender.as_bytes(), withdraw_info.job_id.as_bytes()),
                |coin| -> StdResult<_> {
                    let mut coin = coin.unwrap();
                    coin.amount -= withdraw_info.coin.amount;
                    Ok(coin)
                },
            )?;
        }
        attrs.push(Attribute {
            key: "job_id".to_string(),
            value: withdraw_info.job_id,
        });
        attrs.push(Attribute {
            key: "denom".to_string(),
            value: coin.denom,
        });
        attrs.push(Attribute {
            key: "amount".to_string(),
            value: coin.amount.to_string(),
        });
        coins.push(withdraw_info.coin);
    }
    Ok(Response::new()
        .add_attribute("method", "withdraw")
        .add_attributes(attrs)
        .add_message(CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: coins,
        })))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetDepositInfo { address } => to_binary(&query_address_info(deps, address)?),
        QueryMsg::GetJobInfo { job_id } => to_binary(&query_job_info(deps, job_id)?),
    }
}

fn query_address_info(deps: Deps, address: String) -> StdResult<Vec<DepositInfo>> {
    let all: StdResult<Vec<_>> = DEPOSIT
        .prefix(address.as_bytes())
        .range(deps.storage, None, None, Order::Ascending)
        .collect();
    let all = all.unwrap();
    let mut result = Vec::new();
    for item in all {
        result.push(DepositInfo {
            coin: item.1,
            job_id: String::from_utf8(item.0).unwrap(),
        });
    }
    Ok(result)
}

fn query_job_info(deps: Deps, job_id: String) -> StdResult<Vec<JobInfo>> {
    let all: StdResult<Vec<_>> = DEPOSIT_REVERSE
        .prefix(job_id.as_bytes())
        .range(deps.storage, None, None, Order::Ascending)
        .collect();
    let all = all.unwrap();
    let mut result = Vec::new();
    for item in all {
        if item.1 {
            let coin = DEPOSIT
                .may_load(deps.storage, (item.0.as_slice(), job_id.as_bytes()))?
                .unwrap_or_default();
            result.push(JobInfo {
                coin,
                address: String::from_utf8(item.0)?,
            });
        }
    }
    Ok(result)
}
