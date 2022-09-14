use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::IbcEndpoint;
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("icq_config");

/// static info on one channel that doesn't change
pub const CHANNEL_INFO: Map<&str, ChannelInfo> = Map::new("channel_info");

pub const QUERY_RESULT_COUNTER: Item<u64> = Item::new("query_result_counter");

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct Config {
    pub default_timeout: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct ChannelInfo {
    /// id of this channel
    pub id: String,
    /// the remote channel/port we connect to
    pub counterparty_endpoint: IbcEndpoint,
    /// the connection this exists on (you can use to query client/consensus info)
    pub connection_id: String,
}
