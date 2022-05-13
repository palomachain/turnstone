use crate::contract::{execute, instantiate};
use crate::msg::{
    ConsensusMsg, ExecuteMsg, InstantiateMsg, JobId, JobInfo, QueryMsg, QueryResult, Validator,
};
use crate::validation;
use crate::validation::{PubKey, Signature};
use cosmwasm_std::testing::{mock_dependencies_with_balances, mock_env, mock_info};
use cosmwasm_std::{from_binary, Addr, Api, Binary, Coin, Deps, DepsMut, Env, Uint128};
use eyre::Result;
use secp256k1::rand::thread_rng;
use secp256k1::{generate_keypair, Message, SecretKey};

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
        InstantiateMsg { valset: vec![] },
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
        InstantiateMsg { valset: vec![] },
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

    fn gen_keys() -> (SecretKey, PubKey) {
        let (privkey, pubkey) = generate_keypair(&mut thread_rng());
        let pubkey = PubKey(Binary::from(pubkey.serialize()));
        (privkey, pubkey)
    }

    let mut base_message_id: u64 = 0xBA5EBA11 - 1;
    // Generate a unique message id.
    let mut mid = || -> String {
        base_message_id += 1;
        format!("{:x}", base_message_id)
    };

    fn sign(
        keys: &[(SecretKey, PubKey)],
        message_id: &str,
        raw_json: &str,
    ) -> Result<Vec<Signature>> {
        Ok(keys
            .iter()
            .map(|(privkey, pubkey)| {
                Ok(Signature {
                    pubkey: pubkey.clone(),
                    signature: Binary::from(
                        privkey
                            .sign_ecdsa(Message::from_slice(&validation::hash(
                                message_id, raw_json,
                            ))?)
                            .serialize_compact(),
                    ),
                })
            })
            .collect::<Result<Vec<_>>>()?)
    }

    let addresses: Vec<_> = [
        "aaa", "bbb", "ccc", "ddd", "eee", "fff", "ggg", "hhh", "iii", "jjj",
    ]
    .into_iter()
    .map(|addr| Ok(deps.api.addr_validate(addr)?))
    .collect::<Result<_>>()?;
    let non_validator = deps.api.addr_validate("eve")?;
    let keys: Vec<_> = addresses.iter().map(|_| gen_keys()).collect();

    let _ = instantiate(
        deps.as_mut(),
        mock_env(),
        mock_info("creator", &[]),
        InstantiateMsg {
            valset: addresses
                .iter()
                .zip(&keys)
                .map(|(addr, (_, pubkey))| Validator {
                    public_key: pubkey.clone(),
                    stake: Uint128::new(10),
                    address: vec![addr.clone()],
                })
                .collect(),
        },
    )?;

    let t = |deps: DepsMut,
             addr: &Addr,
             message_id: &str,
             keys: &[(SecretKey, PubKey)]|
     -> Result<()> {
        let valid_json = r#"{"stub": {}}"#;
        execute(
            deps,
            mock_env(),
            mock_info(addr.as_str(), &[]),
            ExecuteMsg::WithConsensus {
                message_id: message_id.to_string(),
                raw_json: valid_json.to_string(),
                signatures: sign(&keys, &message_id, &valid_json)?,
            },
        )?;
        Ok(())
    };

    // No messages validate with less than quorum. Exactly half is not quorum.
    for addr in [&addresses[0], &non_validator] {
        for n in 0..=5 {
            assert!(t(deps.as_mut(), addr, &mid(), &keys[..n]).is_err());
        }
    }
    // Non trusted addreses never work.
    for n in 0..=keys.len() {
        assert!(t(deps.as_mut(), &non_validator, &mid(), &keys[..n]).is_err());
    }
    // Trusted addreses do, if they have enough signatures.
    for addr in &addresses {
        for n in 6..10 {
            t(deps.as_mut(), addr, &mid(), &keys[..n])?;
        }
    }
    // But not if you try to reuse an id!
    t(deps.as_mut(), &addresses[0], "😠", &keys)?;
    assert!(t(deps.as_mut(), &addresses[0], "😠", &keys).is_err());

    // And if you change the valset...
    let new_addr = deps.api.addr_validate("new_hotness")?;
    let (privkey, pubkey) = gen_keys();
    let update_json = serde_json::to_string(&ConsensusMsg::UpdateValset {
        valset: vec![Validator {
            public_key: pubkey.clone(),
            stake: Uint128::new(100),
            address: vec![new_addr.clone()],
        }],
    })?;
    let message_id = mid();
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(&addresses[0].as_str(), &[]),
        ExecuteMsg::WithConsensus {
            message_id: message_id.clone(),
            raw_json: update_json.to_string(),
            signatures: sign(&keys, &message_id, &update_json.to_string())?,
        },
    )?;

    // Then they can sign new messages
    t(deps.as_mut(), &new_addr, &mid(), &[(privkey, pubkey)])?;
    // But others can't.
    assert!(t(deps.as_mut(), &addresses[0], &mid(), &keys).is_err());

    Ok(())
}
