use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use cw20::{AllowanceResponse, Logo, MarketingInfoResponse};

use crate::ContractError;

// Mapping between connections and the counter on that connection.
pub const CONNECTION_COUNTS: Map<String, u32> = Map::new("connection_counts");
pub const IS_MAIN_CONTACT: Map<String, bool> = Map::new("is_main");
#[cw_serde]
pub struct TokenInfo {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: Uint128,
    pub mint: Option<MinterData>,
}

#[cw_serde]
pub struct MinterData {
    pub minter: Addr,
    /// cap is how many more tokens can be issued by the minter
    pub cap: Option<Uint128>,
}

impl TokenInfo {
    pub fn get_cap(&self) -> Option<Uint128> {
        self.mint.as_ref().and_then(|v| v.cap)
    }
}

#[cw_serde]
pub struct Chains {
    pub other_chains: Vec<String>,
}
impl Chains {
    pub fn is_allowed(&self, chain: String) -> Result<bool, ContractError> {
        let mut number: u32 = 0;
        let string = &chain;
        for i in &self.other_chains {
            if i == string {
                number += 1;
            }
        }
        Ok(number != 0)
    }
}

pub const CHAINS: Item<Chains> = Item::new("chains");

pub const TOKEN_INFO_CHAIN: Item<TokenInfo> = Item::new("token_infor_1");
pub const TOKEN_INFO: Map<String, TokenInfo> = Map::new("token_info");
pub const MARKETING_INFO: Item<MarketingInfoResponse> = Item::new("marketing_info");
pub const LOGO: Item<Logo> = Item::new("logo");
pub const BALANCES: Map<(String, &Addr), Uint128> = Map::new("balance");
pub const ALLOWANCES: Map<(String, &Addr, &Addr), AllowanceResponse> = Map::new("allowance");
// TODO: After https://github.com/CosmWasm/cw-plus/issues/670 is implemented, replace this with a `MultiIndex` over `ALLOWANCES`
pub const ALLOWANCES_SPENDER: Map<(String, &Addr, &Addr), AllowanceResponse> =
    Map::new("allowance_spender");
