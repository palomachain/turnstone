///! Routines and storage associated with validating messages.
use cosmwasm_std::{Addr, Binary, Deps, MessageInfo, Uint128};
use cw_storage_plus::{Item, Map};
use eyre::{ensure, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A list of public keys and their associated stake in our chain.
pub const VALIDATORS: Item<Vec<ValKey>> = Item::new("validators");

/// Addresses associated with our validators. Only these addresses may issue
/// [`ExecuteMsg::WithConsensus`] messages.
pub const TRUSTED_ADDRESSES: Item<Vec<Addr>> = Item::new("trusted_addreses");

/// Messages may not be replayed with the same `id`.
pub const USED_MESSAGE_IDS: Map<&str, ()> = Map::new("used_message_ids");

#[derive(Serialize, Deserialize, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, JsonSchema)]
pub struct PubKey(pub Binary);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ValKey {
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

/// Concatenate `raw_json` with `message_id`, used as a nonce and hash them for signing.
pub(crate) fn hash(message_id: &str, raw_json: &str) -> Vec<u8> {
    // TODO: Delimit these messages.
    Sha256::new()
        .chain_update(raw_json.as_bytes())
        .chain_update(message_id)
        .finalize()
        .to_vec()
}

fn is_signed<'a>(
    deps: Deps,
    message_id: &str,
    raw_json: &'a str,
    signatures: &[Signature],
) -> Result<bool> {
    let message_hash = hash(message_id, raw_json);
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
    // Sort big weights first.
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
    message_id: &str,
    raw_json: &'a str,
    signatures: &[Signature],
) -> Result<T>
where
    T: Deserialize<'a>,
{
    ensure!(
        !USED_MESSAGE_IDS.has(deps.storage, message_id),
        "previously used message_id"
    );
    ensure!(is_trusted(deps, &info.sender)?, "forbidden");
    ensure!(
        is_signed(deps, message_id, raw_json, signatures)?,
        "unauthorized"
    );
    Ok(serde_json::from_str(raw_json)?)
}
