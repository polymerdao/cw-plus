#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, IbcMsg, IbcQuery, MessageInfo, Order,
    PortIdResponse, Response, StdResult,
};

use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{
    ChannelResponse, ConfigResponse, ExecuteMsg, InitMsg,
    ListChannelsResponse, MigrateMsg, PortResponse, QueryMsg, ICQQueryMsg,
    InterchainQueryPacketData,
};
use crate::state::{
    Config, CHANNEL_INFO, CONFIG,
};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:icq";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InitMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let cfg = Config {
        default_timeout: msg.default_timeout,
    };
    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Query(msg) => {
            execute_query(deps, env, msg)
        }
    }
}

pub fn execute_query(
    deps: DepsMut,
    env: Env,
    msg: ICQQueryMsg,
) -> Result<Response, ContractError> {
    // ensure the requested channel is registered
    if !CHANNEL_INFO.has(deps.storage, &msg.channel) {
        return Err(ContractError::NoSuchChannel { id: msg.channel });
    }
    let config = CONFIG.load(deps.storage)?;
    // delta from user is in seconds
    let timeout_delta = match msg.timeout {
            Some(t) => t,
            None => config.default_timeout,
    };
    // timeout is in nanoseconds
    let timeout = env.block.time.plus_seconds(timeout_delta);
    let num_requests = msg.requests.len();
    
    let packet = InterchainQueryPacketData {
        data: to_binary(&msg.requests)?,
    };
    // prepare ibc message
    let msg = IbcMsg::SendPacket {
            channel_id: msg.channel,
            data: to_binary(&packet)?,
            timeout: timeout.into(),
    };

    // send response
    let res = Response::new()
            .add_message(msg)
            .add_attribute("action", "query")
            .add_attribute("num_requests", num_requests.to_string());
    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Port {} => to_binary(&query_port(deps)?),
        QueryMsg::ListChannels {} => to_binary(&query_list(deps)?),
        QueryMsg::Channel { id } => to_binary(&query_channel(deps, id)?),
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_port(deps: Deps) -> StdResult<PortResponse> {
    let query = IbcQuery::PortId {}.into();
    let PortIdResponse { port_id } = deps.querier.query(&query)?;
    Ok(PortResponse { port_id })
}

fn query_list(deps: Deps) -> StdResult<ListChannelsResponse> {
    let channels = CHANNEL_INFO
        .range_raw(deps.storage, None, None, Order::Ascending)
        .map(|r| r.map(|(_, v)| v))
        .collect::<StdResult<_>>()?;
    Ok(ListChannelsResponse { channels })
}

// make public for ibc tests
pub fn query_channel(deps: Deps, id: String) -> StdResult<ChannelResponse> {
    let info = CHANNEL_INFO.load(deps.storage, &id)?;
    Ok(ChannelResponse {
        info,
    })
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let res = ConfigResponse {
        default_timeout: cfg.default_timeout,
    };
    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_helpers::*;

    use cosmwasm_std::testing::{mock_env, mock_info, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{from_binary, coins, CosmosMsg, StdError, Uint128};
    use crate::msg::RequestQuery;

    #[test]
    fn setup_and_query() {
        let deps = setup(&["channel-3", "channel-7"]);

        let raw_list = query(deps.as_ref(), mock_env(), QueryMsg::ListChannels {}).unwrap();
        let list_res: ListChannelsResponse = from_binary(&raw_list).unwrap();
        assert_eq!(2, list_res.channels.len());
        assert_eq!(mock_channel_info("channel-3"), list_res.channels[0]);
        assert_eq!(mock_channel_info("channel-7"), list_res.channels[1]);

        let raw_channel = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Channel {
                id: "channel-3".to_string(),
            },
        )
        .unwrap();
        let chan_res: ChannelResponse = from_binary(&raw_channel).unwrap();
        assert_eq!(chan_res.info, mock_channel_info("channel-3"));

        let err = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Channel {
                id: "channel-10".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(err, StdError::not_found("icq::state::ChannelInfo"));
    }

    #[test]
    fn execute_query_success() {
        let send_channel = "channel-5";
        let mut deps = setup(&[send_channel, "channel-10"]);

        let requests = vec![RequestQuery {
            data: Binary::from([0, 1, 0, 1]),
            path: "/path".to_string(),
            height: None,
            prove: None,
        }];
        let q = ICQQueryMsg {
            channel: send_channel.to_string(),
            requests,
            timeout: None,
        };

        let msg = ExecuteMsg::Query(q.clone());
        let info = mock_info("foobar", &[]);
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
	if let CosmosMsg::Ibc(IbcMsg::SendPacket {
	    channel_id,
	    data,
	    timeout,
	}) = &res.messages[0].msg
	{
	    let expected_timeout = mock_env().block.time.plus_seconds(DEFAULT_TIMEOUT);
	    assert_eq!(timeout, &expected_timeout.into());
	    assert_eq!(channel_id.as_str(), send_channel);
	    let msg: InterchainQueryPacketData = from_binary(data).unwrap();
	} else {
	    panic!("Unexpected return message: {:?}", res.messages[0]);
	}
    }
}

