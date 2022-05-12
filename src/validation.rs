use cosmwasm_std::{Addr, Binary, Deps, MessageInfo, Uint128};
use cw_storage_plus::Item;
use eyre::{ensure, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VALIDATORS: Item<Vec<Validator>> = Item::new("validators");

pub const TRUSTED_ADDRESSES: Item<Vec<Addr>> = Item::new("trusted_addreses");

#[derive(Serialize, Deserialize, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, JsonSchema)]
pub struct PubKey(pub Binary);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Validator {
    pub pubkey: PubKey,
    pub stake: Uint128,
}

#[derive(Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Signature {
    pub pubkey: PubKey,
    pub signature: Binary,
}

fn is_trusted(deps: Deps, address: &Addr) -> Result<bool> {
    let trusted = TRUSTED_ADDRESSES.load(deps.storage)?;
    Ok(trusted.binary_search(address).is_ok())
}

fn is_signed<'a>(
    deps: Deps,
    info: &MessageInfo,
    raw_json: &'a str,
    signatures: &[Signature],
) -> Result<bool> {
    // XXX: This is almost the definition of asking for a chosen prefix attack.
    // @mm can we use a better scheme here?
    let message_hash = Sha256::new()
        .chain_update(raw_json.as_bytes())
        .chain_update(info.sender.as_bytes())
        .finalize();
    let validators = VALIDATORS.load(deps.storage)?;
    let total = validators.iter().map(|v| v.stake).sum::<Uint128>();

    // We only care about the signatures for pubkeys among our validators. We can also
    // sort larger stakes first and reach consensus faster.
    let mut signatures: Vec<_> = signatures
        .iter()
        .filter_map(|sig| {
            match validators.binary_search_by(|probe| probe.pubkey.cmp(&sig.pubkey)) {
                Ok(i) => Some((validators[i].stake, sig)),
                Err(_) => None,
            }
        })
        .collect();
    signatures.sort_by(|(w1, _), (w2, _)| w2.cmp(w1));
    let mut total_weight = Uint128::new(0);
    for (weight, sig) in signatures {
        if deps
            .api
            .secp256k1_verify(&message_hash, &sig.signature, &sig.pubkey.0)?
        {
            total_weight += weight;
            if total_weight * Uint128::new(2) > total {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub(crate) fn validate_json<'a, T>(
    deps: Deps,
    info: &MessageInfo,
    raw_json: &'a str,
    signatures: &[Signature],
) -> Result<T>
where
    T: Deserialize<'a>,
{
    ensure!(
        is_trusted(deps, &info.sender)? || is_signed(deps, info, raw_json, signatures)?,
        "forbidden"
    );
    Ok(serde_json::from_str(raw_json)?)
}
