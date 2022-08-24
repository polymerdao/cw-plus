#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_binary, to_binary, Addr, Binary, Deps, DepsMut, Env, IbcMsg, IbcQuery, MessageInfo, Order,
    PortIdResponse, Response, StdError, StdResult,
};
use semver::Version;

use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::msg::{
    ChannelResponse, ConfigResponse, ExecuteMsg, InitMsg,
    ListChannelsResponse, MigrateMsg, PortResponse, QueryMsg, ICQQueryMsg,
};
use crate::state::{
    Config, ADMIN, CHANNEL_INFO, CONFIG,
};
use cw_utils::{maybe_addr, nonpayable, one_coin};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cw20-ics20";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InitMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let cfg = Config {
        default_timeout: msg.default_timeout,
        default_gas_limit: msg.default_gas_limit,
    };
    CONFIG.save(deps.storage, &cfg)?;

    let admin = deps.api.addr_validate(&msg.gov_contract)?;
    ADMIN.set(deps.branch(), Some(admin))?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Query(msg) => {
            execute_query(deps, env, msg)
        }
        ExecuteMsg::UpdateAdmin { admin } => {
            let admin = deps.api.addr_validate(&admin)?;
            Ok(ADMIN.execute_update_admin(deps, info, Some(admin))?)
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
	// prepare ibc message
	let msg = IbcMsg::Query {
		channel_id: msg.channel,
		requests: msg.requests,
		timeout: timeout.into(),
	};

	// send response
	let res = Response::new()
		.add_message(msg)
		.add_attribute("action", "query")
		.add_attribute("num_requests", msg.requests.len().to_string());
	Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(mut deps: DepsMut, env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Port {} => to_binary(&query_port(deps)?),
        QueryMsg::ListChannels {} => to_binary(&query_list(deps)?),
        QueryMsg::Channel { id } => to_binary(&query_channel(deps, id)?),
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::Admin {} => to_binary(&ADMIN.query_admin(deps)?),
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
    let admin = ADMIN.get(deps)?.unwrap_or_else(|| Addr::unchecked(""));
    let res = ConfigResponse {
        default_timeout: cfg.default_timeout,
        default_gas_limit: cfg.default_gas_limit,
        gov_contract: admin.into(),
    };
    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_helpers::*;

    use cosmwasm_std::testing::{mock_env, mock_info, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{coin, coins, CosmosMsg, IbcMsg, StdError, Uint128};

    use crate::state::ChannelState;
    use cw_utils::PaymentError;

    #[test]
    fn setup_and_query() {
        let deps = setup(&["channel-3", "channel-7"], &[]);

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
        assert_eq!(0, chan_res.total_sent.len());
        assert_eq!(0, chan_res.balances.len());

        let err = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Channel {
                id: "channel-10".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(err, StdError::not_found("cw20_ics20::state::ChannelInfo"));
    }
}
