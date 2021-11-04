//! This integration test tries to run and call the generated wasm.
//! It depends on a Wasm build being available, which you can create with `cargo wasm`.
//! Then running `cargo integration-test` will validate we can properly call into that generated Wasm.
//!
//! You can easily convert unit tests to integration tests as follows:
//! 1. Copy them over verbatim
//! 2. Then change
//!      let mut deps = mock_dependencies(20, &[]);
//!    to
//!      let mut deps = mock_instance(WASM, &[]);
//! 3. If you access raw storage, where ever you see something like:
//!      deps.storage.get(CONFIG_KEY).expect("no data stored");
//!    replace it with:
//!      deps.with_storage(|store| {
//!          let data = store.get(CONFIG_KEY).expect("no data stored");
//!          //...
//!      });
//! 4. Anywhere you see query(&deps, ...) you must replace it with query(&mut deps, ...)

use cosmwasm_std::testing::mock_info;
use cosmwasm_std::{
    attr, from_binary, to_binary, Addr, Coin, ContractResult, CosmosMsg, ReplyOn, Response, SubMsg,
    Uint128, WasmMsg,
};
use cosmwasm_vm::testing::{
    execute, instantiate, mock_backend_with_balances, mock_env, query, MockApi, MockQuerier,
    MockStorage, MOCK_CONTRACT_ADDR,
};
use cosmwasm_vm::{Instance, InstanceOptions};

use std::time::{SystemTime, UNIX_EPOCH};
use terraswap::asset::{AssetInfo, WeightedAssetInfo};
use terraswap::factory::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use terraswap::hook::InitHook;
use terraswap::pair::InstantiateMsg as PairInstantiateMsg;

// This line will test the output of cargo wasm
static WASM: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/release/terraswap_factory.wasm");
// You can uncomment this line instead to test productionified build from rust-optimizer
// static WASM: &[u8] = include_bytes!("../contract.wasm");

const DEFAULT_GAS_LIMIT: u64 = 500_000;

pub fn mock_instance(
    wasm: &[u8],
    contract_balance: &[(&str, &[Coin])],
) -> Instance<MockApi, MockStorage, MockQuerier> {
    // TODO: check_wasm is not exported from cosmwasm_vm
    // let terra_features = features_from_csv("staking,terra");
    // check_wasm(wasm, &terra_features).unwrap();
    let backend = mock_backend_with_balances(contract_balance);
    Instance::from_code(
        wasm,
        backend,
        InstanceOptions {
            gas_limit: DEFAULT_GAS_LIMIT,
            print_debug: false,
        },
        None,
    )
    .unwrap()
}

#[test]
fn proper_initialization() {
    let mut deps = mock_instance(WASM, &[]);

    let msg = InstantiateMsg {
        pair_code_id: 321u64,
        token_code_id: 123u64,
        owner: "owner0000".to_string(),
        init_hook: None,
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    // we can just call .unwrap() to assert this was a success
    let _res: Response = instantiate(&mut deps, env.clone(), info, msg).unwrap();

    let query_res = query(&mut deps, env, QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_binary(&query_res).unwrap();
    assert_eq!(123u64, config_res.token_code_id);
    assert_eq!(321u64, config_res.pair_code_id);
    assert_eq!(Addr::unchecked("owner0000"), config_res.owner);
}

#[test]
fn update_config() {
    let mut deps = mock_instance(WASM, &[]);
    let msg = InstantiateMsg {
        pair_code_id: 321u64,
        token_code_id: 123u64,
        owner: "owner0000".to_string(),
        init_hook: None,
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    // we can just call .unwrap() to assert this was a success
    let _res: Response = instantiate(&mut deps, env, info, msg).unwrap();

    // update owner
    let env = mock_env();
    let info = mock_info("owner0000", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        owner: Some(Addr::unchecked("addr0001")),
        pair_code_id: None,
        token_code_id: None,
    };

    let res: Response = execute(&mut deps, env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // it worked, let's query the state
    let query_res = query(&mut deps, env, QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_binary(&query_res).unwrap();
    assert_eq!(123u64, config_res.token_code_id);
    assert_eq!(321u64, config_res.pair_code_id);
    assert_eq!(Addr::unchecked("addr0001"), config_res.owner);

    // update left items
    let env = mock_env();
    let info = mock_info("addr0001", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        owner: None,
        pair_code_id: Some(100u64),
        token_code_id: Some(200u64),
    };

    let res: Response = execute(&mut deps, env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // it worked, let's query the state
    let query_res = query(&mut deps, env, QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_binary(&query_res).unwrap();
    assert_eq!(200u64, config_res.token_code_id);
    assert_eq!(100u64, config_res.pair_code_id);
    assert_eq!(Addr::unchecked("addr0001"), config_res.owner);

    // Unauthorzied err
    let env = mock_env();
    let info = mock_info("addr0000", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        owner: None,
        pair_code_id: None,
        token_code_id: None,
    };

    let res: ContractResult<Response> = execute(&mut deps, env, info, msg);
    assert_eq!(res.unwrap_err(), "Unauthorized");
}

#[test]
fn create_pair() {
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let end_time = start_time + 1000;
    let mut deps = mock_instance(WASM, &[]);

    let owner = "owner0000";

    let msg = InstantiateMsg {
        pair_code_id: 321u64,
        token_code_id: 123u64,
        owner: owner.to_string(),
        init_hook: None,
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    // we can just call .unwrap() to assert this was a success
    let _res: Response = instantiate(&mut deps, env, info, msg).unwrap();

    let asset_infos = [
        WeightedAssetInfo {
            info: AssetInfo::Token {
                contract_addr: Addr::unchecked("asset0000"),
            },
            start_weight: Uint128::new(1),
            end_weight: Uint128::new(1),
        },
        WeightedAssetInfo {
            info: AssetInfo::Token {
                contract_addr: Addr::unchecked("asset0001"),
            },
            start_weight: Uint128::new(1),
            end_weight: Uint128::new(1),
        },
    ];

    let msg = ExecuteMsg::CreatePair {
        asset_infos: asset_infos.clone(),
        start_time,
        end_time,
        init_hook: None,
        description: Some(String::from("description")),
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);
    let res: Response = execute(&mut deps, env.clone(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "create_pair"),
            attr("pair", "asset0000-asset0001")
        ]
    );
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Instantiate {
                msg: to_binary(&PairInstantiateMsg {
                    asset_infos: asset_infos.clone(),
                    token_code_id: 123u64,
                    init_hook: None,
                    start_time,
                    end_time,
                    description: Some(String::from("description")),
                })
                .unwrap(),
                code_id: 321u64,
                funds: vec![],
                admin: Some(owner.to_string()),
                label: String::from("terraswap pair"),
            })),
            SubMsg {
                id: 0,
                msg: WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_binary(&ExecuteMsg::Register {
                        asset_infos: asset_infos.clone()
                    })
                    .unwrap(),
                    funds: vec![],
                }
                .into(),
                gas_limit: None,
                reply_on: ReplyOn::Success,
            }
        ]
    );
}
