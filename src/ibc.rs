use crate::{
    ack::{make_ack_fail, make_ack_success},
    allowances::{
        execute_burn_from, execute_decrease_allowance, execute_increase_allowance,
        execute_send_from, execute_transfer_from,
    },
    contract::{execute_burn, execute_mint, execute_send, execute_transfer, try_increment},
    error::Never,
    msg::IbcExecuteMsg,
    state::CONNECTION_COUNTS,
    ContractError,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_binary, Binary, DepsMut, Env, IbcBasicResponse, IbcChannel, IbcChannelCloseMsg,
    IbcChannelConnectMsg, IbcChannelOpenMsg, IbcOrder, IbcPacketAckMsg, IbcPacketReceiveMsg,
    IbcPacketTimeoutMsg, IbcReceiveResponse, MessageInfo, Uint128,
};
use cw_utils::Expiration;

pub const IBC_VERSION: &str = "counter-1";

/// Handles the `OpenInit` and `OpenTry` parts of the IBC handshake.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_channel_open(
    _deps: DepsMut,
    _env: Env,
    msg: IbcChannelOpenMsg,
) -> Result<(), ContractError> {
    validate_order_and_version(msg.channel(), msg.counterparty_version())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_channel_connect(
    deps: DepsMut,
    _env: Env,
    msg: IbcChannelConnectMsg,
) -> Result<IbcBasicResponse, ContractError> {
    validate_order_and_version(msg.channel(), msg.counterparty_version())?;

    // Initialize the count for this channel to zero.
    let channel = msg.channel().endpoint.channel_id.clone();
    CONNECTION_COUNTS.save(deps.storage, channel.clone(), &0)?;

    Ok(IbcBasicResponse::new()
        .add_attribute("method", "ibc_channel_connect")
        .add_attribute("channel_id", channel))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_channel_close(
    deps: DepsMut,
    _env: Env,
    msg: IbcChannelCloseMsg,
) -> Result<IbcBasicResponse, ContractError> {
    let channel = msg.channel().endpoint.channel_id.clone();
    // Reset the state for the channel.
    CONNECTION_COUNTS.remove(deps.storage, channel.clone());
    Ok(IbcBasicResponse::new()
        .add_attribute("method", "ibc_channel_close")
        .add_attribute("channel", channel))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_packet_receive(
    deps: DepsMut,
    env: Env,
    msg: IbcPacketReceiveMsg,
    info: MessageInfo,
) -> Result<IbcReceiveResponse, Never> {
    // Regardless of if our processing of this packet works we need to
    // commit an ACK to the chain. As such, we wrap all handling logic
    // in a seprate function and on error write out an error ack.
    match do_ibc_packet_receive(deps, info, env, msg) {
        Ok(response) => Ok(response),
        Err(error) => Ok(IbcReceiveResponse::new()
            .add_attribute("method", "ibc_packet_receive")
            .add_attribute("error", error.to_string())
            .set_ack(make_ack_fail(error.to_string()))),
    }
}

pub fn do_ibc_packet_receive(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    msg: IbcPacketReceiveMsg,
) -> Result<IbcReceiveResponse, ContractError> {
    // The channel this packet is being relayed along on this chain.
    let channel = msg.packet.dest.channel_id;
    let msg: IbcExecuteMsg = from_binary(&msg.packet.data)?;

    match msg {
        IbcExecuteMsg::Increment {} => execute_increment(deps, channel),
        IbcExecuteMsg::Transfer { receipient, amount } => {
            transfer(deps, env, info, receipient, amount, channel)
        }
        IbcExecuteMsg::Burn { amount } => burn(deps, env, info, amount, channel),
        IbcExecuteMsg::TransferFrom {
            owner,
            recipient,
            amount,
        } => transfer_from(deps, env, info, owner, recipient, amount, channel),
        IbcExecuteMsg::IncreaseAllowance {
            spender,
            amount,
            expires,
        } => increase_allowance(deps, env, info, spender, amount, expires, channel),
        IbcExecuteMsg::DecreaseAllowance {
            spender,
            amount,
            expires,
        } => decrease_allowance(deps, env, info, spender, amount, expires, channel),
        IbcExecuteMsg::Mint { receipient, amount } => {
            mint(deps, env, info, receipient, amount, channel)
        }
        IbcExecuteMsg::BurnFrom { owner, amount } => {
            burn_from(deps, env, info, owner, amount, channel)
        }

        IbcExecuteMsg::Send {
            contract,
            amount,
            msg,
        } => send(deps, env, info, contract, amount, msg, channel),
        IbcExecuteMsg::SendFrom {
            owner,
            contract,
            amount,
            msg,
        } => send_from(deps, env, info, owner, contract, amount, msg, channel),
    }
}

fn send_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    contract: String,
    amount: Uint128,
    msg: Binary,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_send_from(
        deps,
        env,
        info,
        owner.clone(),
        contract.clone(),
        amount,
        msg,
        channel,
    )?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "send_from")
        .add_attribute("owner", owner)
        .add_attribute("contract", contract.clone())
        .add_attribute("amount", amount.to_string()))
}
fn decrease_allowance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    amount: Uint128,
    expires: Option<Expiration>,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_increase_allowance(
        deps,
        env,
        info,
        spender.clone(),
        amount,
        expires,
        channel.clone(),
    )?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "decrease_allowance")
        .add_attribute("spender", spender)
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel))
}

fn increase_allowance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    amount: Uint128,
    expires: Option<Expiration>,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_increase_allowance(
        deps,
        env,
        info,
        spender.clone(),
        amount,
        expires,
        channel.clone(),
    )?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "increase_allowance")
        .add_attribute("spender", spender)
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel))
}

fn burn_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    amount: Uint128,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_burn_from(deps, env, info, owner.clone(), amount, channel.clone())?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("action", "burn_from")
        .add_attribute("from", owner)
        .add_attribute("amount", amount.to_string()))
}
fn mint(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: String,
    amount: Uint128,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_mint(deps, env, info, recipient.clone(), amount, channel.clone())?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "mint")
        .add_attribute("recipient", recipient.to_string())
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel.to_string())
        .set_ack(make_ack_success()))
}

fn send(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    contract: String,
    amount: Uint128,
    msg: Binary,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_send(
        deps,
        env,
        info,
        contract.clone(),
        amount,
        msg,
        channel.clone(),
    )?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "send")
        .add_attribute("contract", contract.to_string())
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel.to_string())
        .set_ack(make_ack_success()))
}

fn burn(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    let res = execute_burn(deps, env, info, amount, channel.clone())?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "execute_burn")
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel)
        .set_ack(make_ack_success()))
}
fn transfer_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    recipient: String,
    amount: Uint128,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    execute_transfer_from(
        deps,
        env,
        info,
        owner.clone(),
        recipient.clone(),
        amount,
        channel.clone(),
    )?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "transfer_from")
        .add_attribute("owner", owner)
        .add_attribute("recepient", recipient)
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel))
}
fn transfer(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: String,
    amount: Uint128,
    channel: String,
) -> Result<IbcReceiveResponse, ContractError> {
    let res = execute_transfer(deps, env, info, recipient.clone(), amount, channel.clone())?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "execute_transfer")
        .add_attribute("receipient", recipient.to_string())
        .add_attribute("amount", amount.to_string())
        .add_attribute("channel", channel.to_string())
        .set_ack(make_ack_success()))
}
fn execute_increment(deps: DepsMut, channel: String) -> Result<IbcReceiveResponse, ContractError> {
    let count = try_increment(deps, channel)?;
    Ok(IbcReceiveResponse::new()
        .add_attribute("method", "execute_increment")
        .add_attribute("count", count.to_string())
        .set_ack(make_ack_success()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_packet_ack(
    _deps: DepsMut,
    _env: Env,
    _ack: IbcPacketAckMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // Nothing to do here. We don't keep any state about the other
    // chain, just deliver messages so nothing to update.
    //
    // If we did care about how the other chain received our message
    // we could deserialize the data field into an `Ack` and inspect
    // it.
    Ok(IbcBasicResponse::new().add_attribute("method", "ibc_packet_ack"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_packet_timeout(
    _deps: DepsMut,
    _env: Env,
    _msg: IbcPacketTimeoutMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // As with ack above, nothing to do here. If we cared about
    // keeping track of state between the two chains then we'd want to
    // respond to this likely as it means that the packet in question
    // isn't going anywhere.
    Ok(IbcBasicResponse::new().add_attribute("method", "ibc_packet_timeout"))
}

pub fn validate_order_and_version(
    channel: &IbcChannel,
    counterparty_version: Option<&str>,
) -> Result<(), ContractError> {
    // We expect an unordered channel here. Ordered channels have the
    // property that if a message is lost the entire channel will stop
    // working until you start it again.
    if channel.order != IbcOrder::Unordered {
        return Err(ContractError::OrderedChannel {});
    }

    if channel.version != IBC_VERSION {
        return Err(ContractError::InvalidVersion {
            actual: channel.version.to_string(),
            expected: IBC_VERSION.to_string(),
        });
    }

    // Make sure that we're talking with a counterparty who speaks the
    // same "protocol" as us.
    //
    // For a connection between chain A and chain B being established
    // by chain A, chain B knows counterparty information during
    // `OpenTry` and chain A knows counterparty information during
    // `OpenAck`. We verify it when we have it but when we don't it's
    // alright.
    if let Some(counterparty_version) = counterparty_version {
        if counterparty_version != IBC_VERSION {
            return Err(ContractError::InvalidVersion {
                actual: counterparty_version.to_string(),
                expected: IBC_VERSION.to_string(),
            });
        }
    }

    Ok(())
}
