use cosmwasm_std::{Api, StdError, StdResult};
use oraiswap::asset::AssetInfo;

pub fn get_prefix_decode_bech32(address: &str) -> StdResult<String> {
    let Ok((prefix, _, _)) = bech32::decode(address) else {
        return Err(StdError::generic_err(format!(
            "Cannot decode remote sender: {}",
            address
        )));
    };

    Ok(prefix)
}

pub fn parse_asset_info_denom(asset_info: &AssetInfo) -> String {
    match asset_info {
        AssetInfo::Token { contract_addr } => format!("cw20:{}", contract_addr.as_str()),
        AssetInfo::NativeToken { denom } => denom.to_string(),
    }
}

pub fn parse_ibc_wasm_port_id(contract_addr: &str) -> String {
    format!("wasm.{}", contract_addr)
}

pub fn denom_to_asset_info(api: &dyn Api, denom: &str) -> AssetInfo {
    if let Ok(contract_addr) = api.addr_validate(denom) {
        AssetInfo::Token { contract_addr }
    } else {
        AssetInfo::NativeToken {
            denom: denom.to_string(),
        }
    }
}

pub fn to_orai_bridge_address(address: &str) -> StdResult<String> {
    let Ok((_, data, _)) = bech32::decode(address) else {
        return Err(StdError::generic_err(format!(
            "Cannot decode sender address in to_orai_bridge_address: {}",
            address
        )));
    };

    bech32::encode("oraib", data, bech32::Variant::Bech32).map_err(|_| {
        StdError::generic_err(format!(
            "Cannot encode sender address to oraibridge address in to_orai_bridge_address: {}",
            address
        ))
    })
}

pub fn get_full_denom(factory_contract: String, subdenom: String) -> String {
    format!("factory/{}/{}", factory_contract, subdenom)
}

#[cfg(test)]
mod tests {
    use crate::helper::{get_prefix_decode_bech32, to_orai_bridge_address};

    #[test]
    fn test_get_prefix_decode_bech32() {
        let result =
            get_prefix_decode_bech32("cosmos1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejl67nlm").unwrap();
        assert_eq!(result, "cosmos".to_string());
    }

    #[test]
    fn test_to_orai_bridge_address() {
        let result = to_orai_bridge_address("orai1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejvfgs7g").unwrap();
        assert_eq!(
            result,
            "oraib1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejmgvu0t".to_string()
        );
    }
}
