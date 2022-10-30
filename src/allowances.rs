use cosmwasm_std::{
    attr, Addr, Binary, BlockInfo, Deps, DepsMut, Env, IbcReceiveResponse, MessageInfo, Response,
    StdError, StdResult, Storage, Uint128,
};
use cw20::{AllowanceResponse, Cw20ReceiveMsg, Expiration};

use crate::error::ContractError;
use crate::state::{ALLOWANCES, ALLOWANCES_SPENDER, BALANCES, TOKEN_INFO};

pub fn execute_increase_allowance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    amount: Uint128,
    expires: Option<Expiration>,
    channel: String,
) -> Result<Response, ContractError> {
    let spender_addr = deps.api.addr_validate(&spender)?;
    if spender_addr == info.sender {
        return Err(ContractError::CannotSetOwnAccount {});
    }

    let update_fn = |allow: Option<AllowanceResponse>| -> Result<_, _> {
        let mut val = allow.unwrap_or_default();
        if let Some(exp) = expires {
            if exp.is_expired(&env.block) {
                return Err(ContractError::InvalidExpiration {});
            }
            val.expires = exp;
        }
        val.allowance += amount;
        Ok(val)
    };
    ALLOWANCES.update(
        deps.storage,
        (channel.clone(), &info.sender, &spender_addr),
        update_fn,
    )?;
    ALLOWANCES_SPENDER.update(
        deps.storage,
        (channel.clone(), &spender_addr, &info.sender),
        update_fn,
    )?;

    let res = Response::new().add_attributes(vec![
        attr("action", "increase_allowance"),
        attr("owner", info.sender),
        attr("spender", spender),
        attr("amount", amount),
    ]);
    Ok(res)
}

pub fn execute_decrease_allowance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    amount: Uint128,
    expires: Option<Expiration>,
    channel: String,
) -> Result<Response, ContractError> {
    let spender_addr = deps.api.addr_validate(&spender)?;
    if spender_addr == info.sender {
        return Err(ContractError::CannotSetOwnAccount {});
    }

    let key = (channel, &info.sender, &spender_addr);

    fn reverse<'a>(t: (String, &'a Addr, &'a Addr)) -> (String, &'a Addr, &'a Addr) {
        (t.0, t.2, t.1)
    }

    // load value and delete if it hits 0, or update otherwise
    let mut allowance = ALLOWANCES.load(deps.storage, key.clone())?;
    if amount < allowance.allowance {
        // update the new amount
        allowance.allowance = allowance
            .allowance
            .checked_sub(amount)
            .map_err(StdError::overflow)?;
        if let Some(exp) = expires {
            if exp.is_expired(&env.block) {
                return Err(ContractError::InvalidExpiration {});
            }
            allowance.expires = exp;
        }
        ALLOWANCES.save(deps.storage, key.clone(), &allowance)?;
        ALLOWANCES_SPENDER.save(deps.storage, reverse(key.clone()), &allowance)?;
    } else {
        ALLOWANCES.remove(deps.storage, key.clone());
        ALLOWANCES_SPENDER.remove(deps.storage, reverse(key.clone()));
    }

    let res = Response::new().add_attributes(vec![
        attr("action", "decrease_allowance"),
        attr("owner", info.sender),
        attr("spender", spender),
        attr("amount", amount),
    ]);
    Ok(res)
}

// this can be used to update a lower allowance - call bucket.update with proper keys
pub fn deduct_allowance(
    storage: &mut dyn Storage,
    owner: &Addr,
    spender: &Addr,
    block: &BlockInfo,
    amount: Uint128,
    channel: String,
) -> Result<AllowanceResponse, ContractError> {
    let update_fn = |current: Option<AllowanceResponse>| -> _ {
        match current {
            Some(mut a) => {
                if a.expires.is_expired(block) {
                    Err(ContractError::Expired {})
                } else {
                    // deduct the allowance if enough
                    a.allowance = a
                        .allowance
                        .checked_sub(amount)
                        .map_err(StdError::overflow)?;
                    Ok(a)
                }
            }
            None => Err(ContractError::NoAllowance {}),
        }
    };
    ALLOWANCES.update(storage, (channel.clone(), owner, spender), update_fn)?;
    ALLOWANCES_SPENDER.update(storage, (channel.clone(), spender, owner), update_fn)
}

pub fn execute_transfer_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    recipient: String,
    amount: Uint128,
    channel: String,
) -> Result<Response, ContractError> {
    let rcpt_addr = deps.api.addr_validate(&recipient)?;
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(
        deps.storage,
        &owner_addr,
        &info.sender,
        &env.block,
        amount,
        channel.clone(),
    )?;

    BALANCES.update(
        deps.storage,
        (channel.clone(), &owner_addr),
        |balance: Option<Uint128>| -> StdResult<_> {
            Ok(balance.unwrap_or_default().checked_sub(amount)?)
        },
    )?;
    BALANCES.update(
        deps.storage,
        (channel.clone(), &rcpt_addr),
        |balance: Option<Uint128>| -> StdResult<_> { Ok(balance.unwrap_or_default() + amount) },
    )?;

    let res = Response::new().add_attributes(vec![
        attr("action", "transfer_from"),
        attr("from", owner),
        attr("to", recipient),
        attr("by", info.sender),
        attr("amount", amount),
    ]);
    Ok(res)
}

pub fn execute_burn_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    amount: Uint128,
    channel: String,
) -> Result<Response, ContractError> {
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(
        deps.storage,
        &owner_addr,
        &info.sender,
        &env.block,
        amount,
        channel.clone(),
    )?;

    // lower balance
    BALANCES.update(
        deps.storage,
        (channel.clone(), &owner_addr),
        |balance: Option<Uint128>| -> StdResult<_> {
            Ok(balance.unwrap_or_default().checked_sub(amount)?)
        },
    )?;

    let mut token_info = TOKEN_INFO.load(deps.storage, channel.clone())?;
    token_info.total_supply = token_info.total_supply - amount;
    // reduce total_supply
    TOKEN_INFO.save(deps.storage, channel.clone(), &token_info)?;

    let res = Response::new().add_attributes(vec![
        attr("action", "burn_from"),
        attr("from", owner),
        attr("by", info.sender),
        attr("amount", amount),
    ]);
    Ok(res)
}

pub fn execute_send_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    contract: String,
    amount: Uint128,
    msg: Binary,
    channel: String,
) -> Result<Response, ContractError> {
    let rcpt_addr = deps.api.addr_validate(&contract)?;
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(
        deps.storage,
        &owner_addr,
        &info.sender,
        &env.block,
        amount,
        channel.clone(),
    )?;

    // move the tokens to the contract
    BALANCES.update(
        deps.storage,
        (channel.clone(), &owner_addr),
        |balance: Option<Uint128>| -> StdResult<_> {
            Ok(balance.unwrap_or_default().checked_sub(amount)?)
        },
    )?;
    BALANCES.update(
        deps.storage,
        (channel, &rcpt_addr),
        |balance: Option<Uint128>| -> StdResult<_> { Ok(balance.unwrap_or_default() + amount) },
    )?;

    let attrs = vec![
        attr("action", "send_from"),
        attr("from", &owner),
        attr("to", &contract),
        attr("by", &info.sender),
        attr("amount", amount),
    ];

    // create a send message
    let msg = Cw20ReceiveMsg {
        sender: info.sender.into(),
        amount,
        msg,
    }
    .into_cosmos_msg(contract)?;

    let res = Response::new().add_message(msg).add_attributes(attrs);
    Ok(res)
}

pub fn query_allowance(
    deps: Deps,
    owner: String,
    spender: String,
    channel: String,
) -> StdResult<AllowanceResponse> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let spender_addr = deps.api.addr_validate(&spender)?;
    let allowance = ALLOWANCES
        .may_load(deps.storage, (channel, &owner_addr, &spender_addr))?
        .unwrap_or_default();
    Ok(allowance)
}
