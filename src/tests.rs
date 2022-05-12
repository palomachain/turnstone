use crate::contract::{execute, instantiate};
use crate::msg::{
    ExecuteMsg, InstantiateMsg, JobId, JobInfo, QueryMsg, QueryResult, ValidatorWithAddresses,
};
use crate::validation;
use crate::validation::{PubKey, Signature};
use cosmwasm_std::testing::{mock_dependencies_with_balances, mock_env, mock_info};
use cosmwasm_std::{from_binary, Addr, Api, Binary, Coin, Deps, Env, Uint128};
use eyre::Result;
use rand::RngCore;
use secp256k1::rand::thread_rng;
use secp256k1::{generate_keypair, Message, SecretKey};
use serde_json::json;

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

    fn sign(privkey: &SecretKey, message_id: &str, raw_json: &str) -> Result<Binary> {
        let sig = privkey.sign_ecdsa(Message::from_slice(&validation::hash(
            message_id, raw_json,
        ))?);
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
                .map(|(addr, (_, pubkey))| ValidatorWithAddresses {
                    public_key: pubkey.clone(),
                    stake: Uint128::new(10),
                    address: vec![addr.clone()],
                })
                .collect(),
        },
    )?;

    let mut t =
        |addr: &Addr, message_id: Option<&str>, keys: &[(SecretKey, PubKey)]| -> Result<()> {
            let message_id = message_id
                .map(|s| s.to_string())
                .unwrap_or_else(|| rand::thread_rng().next_u64().to_string());
            let valid_json = json!({ "none": () }).to_string();
            execute(
                deps.as_mut(),
                mock_env(),
                mock_info(addr.as_str(), &[]),
                ExecuteMsg::WithConsensus {
                    message_id: message_id.clone(),
                    raw_json: valid_json.to_string(),
                    signatures: keys
                        .iter()
                        .map(|(privkey, pubkey)| {
                            Ok(Signature {
                                pubkey: pubkey.clone(),
                                signature: sign(&privkey, &message_id, &valid_json)?,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                },
            )?;
            Ok(())
        };

    // No messages validate with less than quorum. Exactly half is not quorum.
    for addr in [&addresses[0], &non_validator] {
        for n in 0..=5 {
            assert!(t(addr, None, &keys[..n]).is_err());
        }
    }
    // Non trusted addreses never work.
    for n in 0..=keys.len() {
        assert!(t(&non_validator, None, &keys[..n]).is_err());
    }
    // Trusted addreses do, if they have enough signatures.
    for addr in &addresses {
        for n in 6..10 {
            t(addr, None, &keys[..n])?;
        }
    }
    // But not if you try to reuse an id!
    t(&addresses[0], Some("😠"), &keys)?;
    assert!(t(&addresses[0], Some("😠"), &keys).is_err());

    Ok(())
}
