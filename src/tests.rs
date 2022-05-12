use crate::contract::{execute, instantiate};
use crate::msg::{
    ExecuteMsg, InstantiateMsg, JobId, JobInfo, QueryMsg, QueryResult, ValidatorWithAddress,
};
use crate::validation::{PubKey, Signature};
use cosmwasm_std::testing::{mock_dependencies_with_balances, mock_env, mock_info};
use cosmwasm_std::{from_binary, Addr, Api, Binary, Coin, Deps, Env, Uint128};
use eyre::Result;
use secp256k1::rand::thread_rng;
use secp256k1::{generate_keypair, Message, SecretKey};
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

fn job_id(id: i32) -> JobId {
    JobId(id.to_string())
}

fn coin(amount: u128) -> Coin {
    cosmwasm_std::coin(amount, "¤")
}

fn coin2(amount: u128) -> Coin {
    cosmwasm_std::coin(amount, "🐥")
}

pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<QueryResult> {
    Ok(from_binary(&crate::contract::query(deps, env, msg)?)?)
}

#[test]
fn simple_deposit_query_withdraw() -> Result<()> {
    let mut deps = mock_dependencies_with_balances(&[]);

    let _ = instantiate(
        deps.as_mut(),
        mock_env(),
        mock_info("creator", &[]),
        InstantiateMsg { validators: vec![] },
    )?;

    let addr_a = deps.api.addr_validate("aaa")?;
    let addr_b = deps.api.addr_validate("bbb")?;
    let addr_c = deps.api.addr_validate("ccc")?;
    for (deposit, job_id) in [
        (mock_info(addr_a.as_str(), &[coin(600)]), job_id(1)),
        (mock_info(addr_a.as_str(), &[coin(400)]), job_id(1)),
        (mock_info(addr_a.as_str(), &[coin(500)]), job_id(2)),
        (mock_info(addr_b.as_str(), &[coin(6000)]), job_id(1)),
        (mock_info(addr_a.as_str(), &[coin2(777)]), job_id(1)),
        (
            mock_info(addr_c.as_str(), &[coin(4), coin(6), coin2(8), coin2(0)]),
            job_id(3),
        ),
    ] {
        execute(
            deps.as_mut(),
            mock_env(),
            deposit,
            ExecuteMsg::Deposit { job_id },
        )?;
    }

    let qr = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetDepositInfo {
            address: addr_a.clone(),
        },
    )?;
    assert_eq!(
        qr,
        QueryResult::Jobs(vec![
            JobInfo {
                coin: coin(1000),
                job_id: job_id(1)
            },
            JobInfo {
                coin: coin2(777),
                job_id: job_id(1),
            },
            JobInfo {
                coin: coin(500),
                job_id: job_id(2)
            }
        ])
    );

    let qr = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetJobInfo { job_id: job_id(1) },
    )?;
    match qr {
        QueryResult::Balance(mut coins) => {
            coins.sort_by_key(|coin| coin.denom.clone());
            assert_eq!(coins, vec![coin(7000), coin2(777)]);
        }
        _ => panic!("GetJobInfo must return a Balance"),
    };

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(addr_a.as_str(), &[]),
        ExecuteMsg::Withdraw {
            withdraw_info: vec![
                JobInfo {
                    coin: coin(14),
                    job_id: job_id(1),
                },
                JobInfo {
                    coin: coin(500),
                    job_id: job_id(2),
                },
            ],
        },
    )?;

    let qr = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetDepositInfo {
            address: addr_a.clone(),
        },
    )?;
    assert_eq!(
        qr,
        QueryResult::Jobs(vec![
            JobInfo {
                coin: coin(986),
                job_id: job_id(1)
            },
            JobInfo {
                coin: coin2(777),
                job_id: job_id(1),
            },
        ])
    );

    Ok(())
}

#[test]
fn deposit_withdraw_errors() -> Result<()> {
    let mut deps = mock_dependencies_with_balances(&[]);

    let _ = instantiate(
        deps.as_mut(),
        mock_env(),
        mock_info("creator", &[]),
        InstantiateMsg { validators: vec![] },
    )?;

    let addr_a = deps.api.addr_validate("aaa")?;
    for (deposit, job_id) in [
        // No empty deposits.
        (mock_info(addr_a.as_str(), &[]), job_id(1)),
        // Or zero deposits.
        (mock_info(addr_a.as_str(), &[coin(0)]), job_id(2)),
    ] {
        assert!(execute(
            deps.as_mut(),
            mock_env(),
            deposit,
            ExecuteMsg::Deposit { job_id },
        )
        .is_err());
    }
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(addr_a.as_str(), &[coin(1000)]),
        ExecuteMsg::Deposit { job_id: job_id(1) },
    )?;

    for withdraw_info in [
        // No withdrawing nothing.
        vec![],
        // Or too much.
        vec![JobInfo {
            coin: coin(2000),
            job_id: job_id(1),
        }],
        // Or too much but piecemeal.
        vec![
            JobInfo {
                coin: coin(501),
                job_id: job_id(1),
            },
            JobInfo {
                coin: coin(501),
                job_id: job_id(1),
            },
        ],
        // Or of the wrong denomination.
        vec![JobInfo {
            coin: coin2(1),
            job_id: job_id(1),
        }],
    ] {
        assert!(execute(
            deps.as_mut(),
            mock_env(),
            mock_info(addr_a.as_str(), &[]),
            ExecuteMsg::Withdraw { withdraw_info },
        )
        .is_err());
    }

    Ok(())
}

#[test]
fn simple_validation() -> Result<()> {
    let mut deps = mock_dependencies_with_balances(&[]);

    fn keys() -> (SecretKey, PubKey) {
        let (privkey, pubkey) = generate_keypair(&mut thread_rng());
        let pubkey = PubKey(Binary::from(pubkey.serialize()));
        (privkey, pubkey)
    }

    fn sign(privkey: &SecretKey, addr: &Addr, raw_json: &str) -> Result<Binary> {
        let hash = Sha256::new()
            .chain_update(raw_json.as_bytes())
            .chain_update(addr.as_bytes())
            .finalize();
        let sig = privkey.sign_ecdsa(Message::from_slice(&hash)?);
        Ok(Binary::from(sig.serialize_compact()))
    }

    let addresses: Vec<_> = [
        "aaa", "bbb", "ccc", "ddd", "eee", "fff", "ggg", "hhh", "iii", "jjj",
    ]
    .into_iter()
    .map(|addr| Ok(deps.api.addr_validate(addr)?))
    .collect::<Result<_>>()?;
    let non_validator = deps.api.addr_validate("eve")?;
    let keys: Vec<_> = addresses.iter().map(|_| keys()).collect();

    let _ = instantiate(
        deps.as_mut(),
        mock_env(),
        mock_info("creator", &[]),
        InstantiateMsg {
            validators: addresses
                .iter()
                .zip(&keys)
                .map(|(addr, (_, pubkey))| ValidatorWithAddress {
                    public_key: pubkey.clone(),
                    stake: Uint128::new(10),
                    address: addr.clone(),
                })
                .collect(),
        },
    )?;

    let mut t = |addr: &Addr, keys: &[(SecretKey, PubKey)]| -> Result<()> {
        let valid_json = json!({ "none": () }).to_string();
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(addr.as_str(), &[]),
            ExecuteMsg::WithConsensus {
                raw_json: valid_json.to_string(),
                signatures: keys
                    .iter()
                    .map(|(privkey, pubkey)| {
                        Ok(Signature {
                            pubkey: pubkey.clone(),
                            signature: sign(&privkey, &addr, &valid_json)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            },
        )?;
        Ok(())
    };

    // Validators can execute just fine.
    t(&addresses[0], &[])?;
    // Non validators can't. Even with exactly half.
    for n in 0..=5 {
        assert!(t(&non_validator, &keys[..n]).is_err());
    }
    // Unless they have enough signatures.
    for n in 6..10 {
        t(&non_validator, &keys[..n])?;
    }

    Ok(())
}
