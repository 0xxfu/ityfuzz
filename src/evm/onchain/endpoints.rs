use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    env,
    fmt::Debug,
    hash::{Hash, Hasher},
    panic,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use itertools::Itertools;
use reqwest::header::HeaderMap;
use retry::{delay::Fixed, retry_with_index, OperationResult};
use revm_interpreter::analysis::to_analysed;
use revm_primitives::{Bytecode, B160};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::{
    cache::{Cache, FileSystemCache},
    evm::{
        tokens::TokenContext,
        types::{EVMAddress, EVMU256},
    },
};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Copy)]
pub enum Chain {
    ETH,
    GOERLI,
    SEPOLIA,
    BSC,
    CHAPEL,
    POLYGON,
    MUMBAI,
    FANTOM,
    AVALANCHE,
    OPTIMISM,
    ARBITRUM,
    GNOSIS,
    BASE,
    CELO,
    ZKEVM,
    ZkevmTestnet,
    LOCAL,
}

pub trait PriceOracle: Debug {
    // ret0: price = int(original_price x 10^5)
    // ret1: decimals of the token
    fn fetch_token_price(&mut self, token_address: EVMAddress) -> Option<(u32, u32)>;
}

impl FromStr for Chain {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ETH" | "eth" => Ok(Self::ETH),
            "GOERLI" | "goerli" => Ok(Self::GOERLI),
            "SEPOLIA" | "sepolia" => Ok(Self::SEPOLIA),
            "BSC" | "bsc" => Ok(Self::BSC),
            "CHAPEL" | "chapel" => Ok(Self::CHAPEL),
            "POLYGON" | "polygon" => Ok(Self::POLYGON),
            "MUMBAI" | "mumbai" => Ok(Self::MUMBAI),
            "FANTOM" | "fantom" => Ok(Self::FANTOM),
            "AVALANCHE" | "avalanche" => Ok(Self::AVALANCHE),
            "OPTIMISM" | "optimism" => Ok(Self::OPTIMISM),
            "ARBITRUM" | "arbitrum" => Ok(Self::ARBITRUM),
            "GNOSIS" | "gnosis" => Ok(Self::GNOSIS),
            "BASE" | "base" => Ok(Self::BASE),
            "CELO" | "celo" => Ok(Self::CELO),
            "ZKEVM" | "zkevm" => Ok(Self::ZKEVM),
            "ZKEVM_TESTNET" | "zkevm_testnet" => Ok(Self::ZkevmTestnet),
            "LOCAL" | "local" => Ok(Self::LOCAL),
            _ => Err(()),
        }
    }
}

impl Chain {
    pub fn get_chain_id(&self) -> u32 {
        match self {
            Chain::ETH => 1,
            Chain::GOERLI => 5,
            Chain::SEPOLIA => 11155111,
            Chain::BSC => 56,
            Chain::CHAPEL => 97,
            Chain::POLYGON => 137,
            Chain::MUMBAI => 80001,
            Chain::FANTOM => 250,
            Chain::AVALANCHE => 43114,
            Chain::OPTIMISM => 10,
            Chain::ARBITRUM => 42161,
            Chain::GNOSIS => 100,
            Chain::BASE => 8453,
            Chain::CELO => 42220,
            Chain::ZKEVM => 1101,
            Chain::ZkevmTestnet => 1442,
            Chain::LOCAL => 31337,
        }
    }

    pub fn to_lowercase(&self) -> String {
        match self {
            Chain::ETH => "eth",
            Chain::GOERLI => "goerli",
            Chain::SEPOLIA => "sepolia",
            Chain::BSC => "bsc",
            Chain::CHAPEL => "chapel",
            Chain::POLYGON => "polygon",
            Chain::MUMBAI => "mumbai",
            Chain::FANTOM => "fantom",
            Chain::AVALANCHE => "avalanche",
            Chain::OPTIMISM => "optimism",
            Chain::ARBITRUM => "arbitrum",
            Chain::GNOSIS => "gnosis",
            Chain::BASE => "base",
            Chain::CELO => "celo",
            Chain::ZKEVM => "zkevm",
            Chain::ZkevmTestnet => "zkevm_testnet",
            Chain::LOCAL => "local",
        }
        .to_string()
    }

    pub fn get_chain_rpc(&self) -> String {
        if let Ok(url) = env::var("ETH_RPC_URL") {
            return url;
        }
        match self {
            Chain::ETH => "https://eth.merkle.io",
            Chain::GOERLI => "https://rpc.ankr.com/eth_goerli",
            Chain::SEPOLIA => "https://rpc.ankr.com/eth_sepolia",
            Chain::BSC => "https://rpc.ankr.com/bsc",
            Chain::CHAPEL => "https://rpc.ankr.com/bsc_testnet_chapel",
            Chain::POLYGON => "https://polygon.llamarpc.com",
            Chain::MUMBAI => "https://rpc-mumbai.maticvigil.com/",
            Chain::FANTOM => "https://rpc.ankr.com/fantom",
            Chain::AVALANCHE => "https://rpc.ankr.com/avalanche",
            Chain::OPTIMISM => "https://rpc.ankr.com/optimism",
            Chain::ARBITRUM => "https://rpc.ankr.com/arbitrum",
            Chain::GNOSIS => "https://rpc.ankr.com/gnosis",
            Chain::BASE => "https://developer-access-mainnet.base.org",
            Chain::CELO => "https://rpc.ankr.com/celo",
            Chain::ZKEVM => "https://rpc.ankr.com/polygon_zkevm",
            Chain::ZkevmTestnet => "https://rpc.ankr.com/polygon_zkevm_testnet",
            Chain::LOCAL => "http://localhost:8545",
        }
        .to_string()
    }

    pub fn get_chain_etherscan_base(&self) -> String {
        match self {
            Chain::ETH => "https://api.etherscan.io/api",
            Chain::GOERLI => "https://api-goerli.etherscan.io/api",
            Chain::SEPOLIA => "https://api-sepolia.etherscan.io/api",
            Chain::BSC => "https://api.bscscan.com/api",
            Chain::CHAPEL => "https://api-testnet.bscscan.com/api",
            Chain::POLYGON => "https://api.polygonscan.com/api",
            Chain::MUMBAI => "https://mumbai.polygonscan.com/api",
            Chain::FANTOM => "https://api.ftmscan.com/api",
            Chain::AVALANCHE => "https://api.snowtrace.io/api",
            Chain::OPTIMISM => "https://api-optimistic.etherscan.io/api",
            Chain::ARBITRUM => "https://api.arbiscan.io/api",
            Chain::GNOSIS => "https://api.gnosisscan.io/api",
            Chain::BASE => "https://api.basescan.org/api",
            Chain::CELO => "https://api.celoscan.io/api",
            Chain::ZKEVM => "https://api-zkevm.polygonscan.com/api",
            Chain::ZkevmTestnet => "https://api-testnet-zkevm.polygonscan.com/api",
            Chain::LOCAL => "http://localhost:8080/abi/",
        }
        .to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub struct PairData {
    pub src: String,
    pub in_: i32,
    pub pair: String,
    pub in_token: String,
    pub next: String,
    pub src_exact: String,
    pub rate: u32,
    pub initial_reserves_0: String,
    pub initial_reserves_1: String,
    pub decimals_0: u32,
    pub decimals_1: u32,
}

#[derive(Deserialize)]
pub struct GetPairResponse {
    pub data: GetPairResponseData,
}

#[derive(Deserialize)]
pub struct GetPairResponseData {
    pub p0: Vec<GetPairResponseDataPair>,
    pub p1: Vec<GetPairResponseDataPair>,
}

#[derive(Deserialize)]
pub struct GetPairResponseDataPair {
    pub id: String,
    pub token0: GetPairResponseDataPairToken,
    pub token1: GetPairResponseDataPairToken,
}

#[derive(Deserialize)]
pub struct GetPairResponseDataPairToken {
    pub decimals: String,
    pub id: String,
}

#[derive(Clone, Default)]
pub struct OnChainConfig {
    pub endpoint_url: String,
    pub client: reqwest::blocking::Client,
    pub chain_id: u32,
    pub block_number: String,
    pub timestamp: Option<String>,
    pub coinbase: Option<String>,
    pub gaslimit: Option<String>,
    pub block_hash: Option<String>,

    pub etherscan_api_key: Vec<String>,
    pub etherscan_base: String,

    pub chain_name: String,

    balance_cache: HashMap<EVMAddress, EVMU256>,
    pair_cache: HashMap<EVMAddress, Vec<PairData>>,
    slot_cache: HashMap<(EVMAddress, EVMU256), EVMU256>,
    code_cache: HashMap<EVMAddress, String>,
    code_cache_analyzed: HashMap<EVMAddress, Bytecode>,
    price_cache: HashMap<EVMAddress, Option<(u32, u32)>>,
    abi_cache: HashMap<EVMAddress, Option<String>>,
    storage_dump_cache: HashMap<EVMAddress, Option<Arc<HashMap<EVMU256, EVMU256>>>>,
    uniswap_path_cache: HashMap<EVMAddress, TokenContext>,
    rpc_cache: FileSystemCache,
}

impl Debug for OnChainConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnChainConfig")
            .field("endpoint_url", &self.endpoint_url)
            .field("chain_id", &self.chain_id)
            .field("block_number", &self.block_number)
            .field("timestamp", &self.timestamp)
            .field("coinbase", &self.coinbase)
            .field("gaslimit", &self.gaslimit)
            .field("block_hash", &self.block_hash)
            .field("etherscan_api_key", &self.etherscan_api_key)
            .field("etherscan_base", &self.etherscan_base)
            .field("chain_name", &self.chain_name)
            .field("balance_cache", &self.balance_cache)
            .field("pair_cache", &self.pair_cache)
            .field("slot_cache", &self.slot_cache)
            .field("code_cache", &self.code_cache)
            .field("price_cache", &self.price_cache)
            .field("abi_cache", &self.abi_cache)
            .field("storage_dump_cache", &self.storage_dump_cache)
            .field("uniswap_path_cache", &self.uniswap_path_cache)
            .field("rpc_cache", &self.rpc_cache)
            .finish()
    }
}

impl OnChainConfig {
    pub fn new(chain: Chain, block_number: u64) -> Self {
        Self::new_raw(
            chain.get_chain_rpc(),
            chain.get_chain_id(),
            block_number,
            chain.get_chain_etherscan_base(),
            chain.to_lowercase(),
        )
    }

    pub fn new_raw(
        endpoint_url: String,
        chain_id: u32,
        block_number: u64,
        etherscan_base: String,
        chain_name: String,
    ) -> Self {
        let mut s = Self {
            endpoint_url,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("build client failed"),
            chain_id,
            block_number: format!("0x{:x}", block_number),
            timestamp: None,
            coinbase: None,
            gaslimit: None,
            block_hash: None,
            etherscan_api_key: vec![],
            etherscan_base,
            chain_name,
            rpc_cache: FileSystemCache::new("./cache"),
            ..Default::default()
        };
        if block_number == 0 {
            s.set_latest_block_number();
        }
        s
    }

    fn get(&self, url: String) -> Option<String> {
        let mut hasher = DefaultHasher::new();
        let key = format!("get_{}", url.as_str());
        key.hash(&mut hasher);
        let hash = hasher.finish().to_string();
        if let Ok(t) = self.rpc_cache.load(hash.as_str()) {
            return Some(t);
        }
        match retry_with_index(Fixed::from_millis(1000), |current_try| {
            if current_try > 5 {
                return OperationResult::Err("did not succeed within 3 tries".to_string());
            }
            match self.client.get(url.to_string()).headers(get_header()).send() {
                Ok(resp) => {
                    let text = resp.text();
                    match text {
                        Ok(t) => {
                            if t.contains("Max rate limit reached") {
                                debug!("Etherscan max rate limit reached, retrying...");
                                OperationResult::Retry("Rate limit reached".to_string())
                            } else {
                                OperationResult::Ok(t)
                            }
                        }
                        Err(e) => {
                            error!("{:?}", e);
                            OperationResult::Retry("failed to parse response".to_string())
                        }
                    }
                }
                Err(e) => {
                    error!("Error: {}", e);
                    OperationResult::Retry("failed to send request".to_string())
                }
            }
        }) {
            Ok(t) => {
                if !t.contains("error") {
                    self.rpc_cache.save(hash.as_str(), t.as_str()).unwrap();
                }

                Some(t)
            }
            Err(e) => {
                error!("Error: {}", e);
                None
            }
        }
    }

    fn post(&self, url: String, data: String) -> Option<String> {
        let mut hasher = DefaultHasher::new();
        let key = format!("post_{}_{}", url.as_str(), data.as_str());
        key.hash(&mut hasher);
        let hash = hasher.finish().to_string();
        if let Ok(t) = self.rpc_cache.load(hash.as_str()) {
            return Some(t);
        }
        match retry_with_index(Fixed::from_millis(100), |current_try| {
            if current_try > 3 {
                return OperationResult::Err("did not succeed within 3 tries".to_string());
            }
            match self
                .client
                .post(url.to_string())
                .header("Content-Type", "application/json")
                .headers(get_header())
                .body(data.to_string())
                .send()
            {
                Ok(resp) => {
                    let text = resp.text();
                    match text {
                        Ok(t) => OperationResult::Ok(t),
                        Err(e) => {
                            error!("{:?}", e);
                            OperationResult::Retry("failed to parse response".to_string())
                        }
                    }
                }
                Err(e) => {
                    error!("Error: {}", e);
                    OperationResult::Retry("failed to send request".to_string())
                }
            }
        }) {
            Ok(t) => {
                if !t.contains("error") {
                    self.rpc_cache.save(hash.as_str(), t.as_str()).unwrap();
                }
                Some(t)
            }
            Err(e) => {
                error!("Error: {}", e);
                None
            }
        }
    }

    pub fn set_latest_block_number(&mut self) {
        let resp = self._request("eth_blockNumber".to_string(), "[]".to_string());
        match resp {
            Some(resp) => {
                let block_number = resp.as_str().unwrap();
                self.block_number = block_number.to_string();
                let block_number = EVMU256::from_str_radix(block_number.trim_start_matches("0x"), 16)
                    .unwrap()
                    .to_string();
                debug!("latest block number is {}", block_number);
            }
            None => panic!("fail to get latest block number"),
        }
    }

    pub fn add_etherscan_api_key(&mut self, key: String) {
        self.etherscan_api_key.push(key);
    }

    pub fn fetch_blk_hash(&mut self) -> &String {
        if self.block_hash.is_none() {
            self.block_hash = {
                let mut params = String::from("[");
                params.push_str(&format!("\"{}\",false", self.block_number));
                params.push(']');
                let res = self._request("eth_getBlockByNumber".to_string(), params);
                match res {
                    Some(res) => {
                        let blk_hash = res["hash"].as_str().expect("fail to find block hash").to_string();
                        Some(blk_hash)
                    }
                    None => panic!("fail to get block hash"),
                }
            }
        }
        return self.block_hash.as_ref().unwrap();
    }

    pub fn fetch_storage_dump(&mut self, address: EVMAddress) -> Option<Arc<HashMap<EVMU256, EVMU256>>> {
        if let Some(storage) = self.storage_dump_cache.get(&address) {
            storage.clone()
        } else {
            let storage = self.fetch_storage_dump_uncached(address);
            self.storage_dump_cache.insert(address, storage.clone());
            storage
        }
    }

    pub fn fetch_storage_dump_uncached(&mut self, address: EVMAddress) -> Option<Arc<HashMap<EVMU256, EVMU256>>> {
        let resp = {
            let blk_hash = self.fetch_blk_hash();
            let mut params = String::from("[");
            params.push_str(&format!("\"{}\",", blk_hash));
            params.push_str("0,");
            params.push_str(&format!("\"0x{:x}\",", address));
            params.push_str("\"\",");
            params.push_str("1000000000000000");
            params.push(']');
            self._request("debug_storageRangeAt".to_string(), params)
        };

        match resp {
            Some(resp) => {
                let mut map = HashMap::new();
                let kvs = resp["storage"].as_object().expect("failed to convert resp to array");
                if kvs.is_empty() {
                    return None;
                }
                for (_, v) in kvs.iter() {
                    let key = v["key"].as_str().expect("fail to find key");
                    let value = v["value"].as_str().expect("fail to find value");

                    map.insert(
                        EVMU256::from_str_radix(key.trim_start_matches("0x"), 16).unwrap(),
                        EVMU256::from_str_radix(value.trim_start_matches("0x"), 16).unwrap(),
                    );
                }
                Some(Arc::new(map))
            }
            None => None,
        }
    }

    pub fn fetch_abi_uncached(&self, address: EVMAddress) -> Option<String> {
        #[cfg(feature = "no_etherscan")]
        {
            return None;
        }
        let endpoint = format!(
            "{}?module=contract&action=getabi&address={:?}&format=json&apikey={}",
            self.etherscan_base,
            address,
            if !self.etherscan_api_key.is_empty() {
                self.etherscan_api_key[rand::random::<usize>() % self.etherscan_api_key.len()].clone()
            } else {
                "".to_string()
            }
        );
        info!("fetching abi from {}", endpoint);
        match self.get(endpoint.clone()) {
            Some(resp) => {
                let json = serde_json::from_str::<Value>(&resp);
                match json {
                    Ok(json) => {
                        let result_parsed = json["result"].as_str();
                        match result_parsed {
                            Some(result) => {
                                if result == "Contract source code not verified" {
                                    None
                                } else {
                                    Some(result.to_string())
                                }
                            }
                            _ => None,
                        }
                    }
                    Err(_) => None,
                }
            }
            None => {
                error!("failed to fetch abi from {}", endpoint);
                None
            }
        }
    }

    pub fn fetch_abi(&mut self, address: EVMAddress) -> Option<String> {
        if self.abi_cache.contains_key(&address) {
            return self.abi_cache.get(&address).unwrap().clone();
        }
        let abi = self.fetch_abi_uncached(address);
        self.abi_cache.insert(address, abi.clone());
        abi
    }

    fn _request(&self, method: String, params: String) -> Option<Value> {
        let data = format!(
            "{{\"jsonrpc\":\"2.0\", \"method\": \"{}\", \"params\": {}, \"id\": {}}}",
            method, params, self.chain_id
        );
        self.post(self.endpoint_url.clone(), data)
            .and_then(|resp| serde_json::from_str(&resp).ok())
            .and_then(|json: Value| json.get("result").cloned())
            .or_else(|| {
                error!("failed to fetch from {}", self.endpoint_url);
                None
            })
    }

    fn _request_with_id(&self, method: String, params: String, id: u8) -> Option<Value> {
        let data = format!(
            "{{\"jsonrpc\":\"2.0\", \"method\": \"{}\", \"params\": {}, \"id\": {}}}",
            method, params, id
        );
        self.post(self.endpoint_url.clone(), data)
            .and_then(|resp| serde_json::from_str(&resp).ok())
            .and_then(|json: Value| json.get("result").cloned())
            .or_else(|| {
                error!("failed to fetch from {}", self.endpoint_url);
                None
            })
    }

    pub fn get_balance(&mut self, address: EVMAddress) -> EVMU256 {
        if self.balance_cache.contains_key(&address) {
            return self.balance_cache[&address];
        }

        let resp_string = {
            let mut params = String::from("[");
            params.push_str(&format!("\"0x{:x}\",", address));
            params.push_str(&format!("\"{}\"", self.block_number));
            params.push(']');
            let resp = self._request("eth_getBalance".to_string(), params);
            match resp {
                Some(resp) => {
                    let balance = resp.as_str().unwrap();
                    balance.to_string()
                }
                None => "".to_string(),
            }
        };
        let balance = EVMU256::from_str(&resp_string).unwrap();
        info!("balance of {address:?} at {} is {balance}", self.block_number);
        self.balance_cache.insert(address, balance);
        balance
    }

    pub fn fetch_blk_timestamp(&mut self) -> EVMU256 {
        if self.timestamp.is_none() {
            self.timestamp = {
                let mut params = String::from("[");
                params.push_str(&format!("\"{}\",false", self.block_number));
                params.push(']');
                let res = self._request("eth_getBlockByNumber".to_string(), params);
                match res {
                    Some(res) => {
                        let blk_timestamp = res["timestamp"]
                            .as_str()
                            .expect("fail to find block timestamp")
                            .to_string();
                        Some(blk_timestamp)
                    }
                    None => panic!("fail to get block timestamp"),
                }
            }
        }
        let timestamp = EVMU256::from_str(self.timestamp.as_ref().unwrap()).unwrap();
        timestamp
    }

    pub fn fetch_blk_coinbase(&mut self) -> EVMAddress {
        if self.coinbase.is_none() {
            self.coinbase = {
                let mut params = String::from("[");
                params.push_str(&format!("\"{}\",false", self.block_number));
                params.push(']');
                let res = self._request("eth_getBlockByNumber".to_string(), params);
                match res {
                    Some(res) => {
                        let blk_coinbase = res["miner"].as_str().expect("fail to find block coinbase").to_string();
                        Some(blk_coinbase)
                    }
                    None => panic!("fail to get block coinbase"),
                }
            }
        }
        let coinbase = EVMAddress::from_str(self.coinbase.as_ref().unwrap()).unwrap();
        coinbase
    }

    pub fn fetch_blk_gaslimit(&mut self) -> EVMU256 {
        if self.gaslimit.is_none() {
            self.gaslimit = {
                let mut params = String::from("[");
                params.push_str(&format!("\"{}\",false", self.block_number));
                params.push(']');
                let res = self._request("eth_getBlockByNumber".to_string(), params);
                match res {
                    Some(res) => {
                        let blk_gaslimit = res["gasLimit"]
                            .as_str()
                            .expect("fail to find block coinbase")
                            .to_string();
                        Some(blk_gaslimit)
                    }
                    None => panic!("fail to get block coinbase"),
                }
            }
        }
        let gaslimit = EVMU256::from_str(self.gaslimit.as_ref().unwrap()).unwrap();
        gaslimit
    }

    pub fn get_contract_code(&mut self, address: EVMAddress, force_cache: bool) -> String {
        if self.code_cache.contains_key(&address) {
            return self.code_cache[&address].clone();
        }
        if force_cache {
            return "".to_string();
        }

        info!("fetching code from {}", hex::encode(address));

        let resp_string = {
            let mut params = String::from("[");
            params.push_str(&format!("\"0x{:x}\",", address));
            params.push_str(&format!("\"{}\"", self.block_number));
            params.push(']');
            let resp = self._request("eth_getCode".to_string(), params);
            match resp {
                Some(resp) => {
                    let code = resp.as_str().unwrap();
                    code.to_string()
                }
                None => "".to_string(),
            }
        }
        .trim_start_matches("0x")
        .to_string();
        self.code_cache.insert(address, resp_string.clone());
        resp_string
    }

    pub fn get_contract_code_analyzed(&mut self, address: EVMAddress, force_cache: bool) -> Bytecode {
        if self.code_cache_analyzed.contains_key(&address) {
            return self.code_cache_analyzed[&address].clone();
        }

        let code = self.get_contract_code(address, force_cache);
        let contract_code = to_analysed(Bytecode::new_raw(Bytes::from(
            hex::decode(code).expect("fail to decode contract code"),
        )));
        let contract_code = to_analysed(contract_code);
        self.code_cache_analyzed.insert(address, contract_code.clone());
        contract_code
    }

    pub fn get_contract_slot(&mut self, address: EVMAddress, slot: EVMU256, force_cache: bool) -> EVMU256 {
        if self.slot_cache.contains_key(&(address, slot)) {
            return self.slot_cache[&(address, slot)];
        }
        if force_cache {
            return EVMU256::ZERO;
        }

        let resp_string = {
            let mut params = String::from("[");
            params.push_str(&format!("\"0x{:x}\",", address));
            params.push_str(&format!("\"0x{:x}\",", slot));
            params.push_str(&format!("\"{}\"", self.block_number));
            params.push(']');
            let resp = self._request("eth_getStorageAt".to_string(), params);
            match resp {
                Some(resp) => {
                    let slot_data = resp.as_str().unwrap();
                    slot_data.to_string()
                }
                None => "".to_string(),
            }
        };

        let slot_suffix = resp_string.trim_start_matches("0x");

        if slot_suffix.is_empty() {
            self.slot_cache.insert((address, slot), EVMU256::ZERO);
            return EVMU256::ZERO;
        }
        let slot_value = EVMU256::try_from_be_slice(&hex::decode(slot_suffix).unwrap()).unwrap();
        self.slot_cache.insert((address, slot), slot_value);
        slot_value
    }
}

impl OnChainConfig {
    pub fn get_pair(&mut self, token: &str, network: &str, is_pegged: bool, weth: String) -> Vec<PairData> {
        let token: String = token.to_lowercase();
        if self.pair_cache.contains_key(&EVMAddress::from_str(&token).unwrap()) {
            return self.pair_cache[&EVMAddress::from_str(&token).unwrap()].clone();
        }
        info!("fetching pairs for {token}");
        let url = if is_pegged {
            format!("https://pairs.infra.fuzz.land/single_pair/{network}/{token}/{weth}")
        } else {
            format!("https://pairs.infra.fuzz.land/pairs/{network}/{token}")
        };
        let resp: Value = reqwest::blocking::get(url).unwrap().json().unwrap();
        let mut pairs: Vec<PairData> = Vec::new();
        if let Some(resp_pairs) = resp.as_array() {
            for item in resp_pairs {
                let pair = item["pair"].as_str().unwrap().to_string();
                let code = self.get_contract_code(EVMAddress::from_str(&pair).unwrap(), false);
                if code.is_empty() {
                    continue;
                }
                let token0 = item["token0"].as_str().unwrap().to_string();
                let token1 = item["token1"].as_str().unwrap().to_string();

                let token0_decimals = item["token0_decimals"].as_i64().unwrap();
                let token1_decimals = item["token1_decimals"].as_i64().unwrap();
                let data = PairData {
                    src: if is_pegged { "pegged" } else { "v2" }.to_string(),
                    in_: if token == token0 { 0 } else { 1 },
                    pair,
                    next: if token == token0 { token1 } else { token0 },
                    in_token: token.clone(),
                    src_exact: item["interface"].as_str().unwrap().to_string(),
                    rate: 0,
                    initial_reserves_0: "".to_string(),
                    initial_reserves_1: "".to_string(),
                    decimals_0: if token0_decimals >= 0 {
                        token0_decimals as u32
                    } else {
                        0
                    },
                    decimals_1: if token1_decimals >= 0 {
                        token1_decimals as u32
                    } else {
                        0
                    },
                };
                pairs.push(data);
            }
        }
        self.pair_cache
            .insert(EVMAddress::from_str(&token).unwrap(), pairs.clone());
        pairs
    }

    pub fn fetch_reserve(&self, pair: &str) -> (String, String) {
        let result = {
            let params = json!([{
            "to": pair,
            "data": "0x0902f1ac",
            "id": 1
        }, self.block_number]);
            debug!("fetching reserve for {pair} {}", self.block_number);
            let resp = self._request_with_id("eth_call".to_string(), params.to_string(), 1);
            match resp {
                Some(resp) => resp.to_string(),
                None => "".to_string(),
            }
        };

        if result.len() != 196 {
            let rpc = &self.endpoint_url;
            let pair_code = self.clone().get_contract_code(B160::from_str(pair).unwrap(), true);
            warn!("rpc: {rpc}, result: {result}, pair: {pair}, pair code: {pair_code}");
            panic!("Unexpected RPC error, consider setting env <ETH_RPC_URL> ");
        }

        let reserve1 = &result[3..67];
        let reserve2 = &result[67..131];

        (reserve1.into(), reserve2.into())
    }
}

fn get_header() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authority", "etherscan.io".parse().unwrap());
    headers.insert("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.9".parse().unwrap());
    headers.insert("accept-language", "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
    headers.insert("cache-control", "max-age=0".parse().unwrap());
    headers.insert(
        "sec-ch-ua",
        "\"Not?A_Brand\";v=\"8\", \"Chromium\";v=\"108\", \"Google Chrome\";v=\"108\""
            .parse()
            .unwrap(),
    );
    headers.insert("sec-ch-ua-mobile", "?0".parse().unwrap());
    headers.insert("sec-ch-ua-platform", "\"macOS\"".parse().unwrap());
    headers.insert("sec-fetch-dest", "document".parse().unwrap());
    headers.insert("sec-fetch-mode", "navigate".parse().unwrap());
    headers.insert("sec-fetch-site", "none".parse().unwrap());
    headers.insert("sec-fetch-user", "?1".parse().unwrap());
    headers.insert("upgrade-insecure-requests", "1".parse().unwrap());
    headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers
}

#[cfg(test)]
mod tests {
    use tracing::debug;

    use super::*;
    use crate::evm::{
        onchain::endpoints::Chain::{BSC, ETH},
        types::EVMAddress,
    };

    #[test]
    fn test_onchain_config() {
        let config = OnChainConfig::new(BSC, 0);
        let v = config._request(
            "eth_getCode".to_string(),
            "[\"0x0000000000000000000000000000000000000000\", \"latest\"]".to_string(),
        );
        debug!("{:?}", v)
    }

    #[test]
    fn test_get_contract_slot() {
        let mut config = OnChainConfig::new(BSC, 0);
        let v = config.get_contract_slot(
            EVMAddress::from_str("0xb486857fac4254a7ffb3b1955ee0c0a2b2ca75ab").unwrap(),
            EVMU256::from(3),
            false,
        );
        debug!("{:?}", v)
    }

    #[test]
    fn test_fetch_abi() {
        let mut config = OnChainConfig::new(BSC, 0);
        let v = config.fetch_abi(EVMAddress::from_str("0xa0a2ee912caf7921eaabc866c6ef6fec8f7e90a4").unwrap());
        debug!("{:?}", v)
    }

    #[test]
    fn test_get_balance() {
        let mut config = OnChainConfig::new(ETH, 18168677);
        let v = config.get_balance(EVMAddress::from_str("0x1f9090aaE28b8a3dCeaDf281B0F12828e676c326").unwrap());
        debug!("{:?}", v);
        assert!(v == EVMU256::from(439351222497229612i64));
    }

    #[test]
    fn test_get_pair_pegged() {
        let mut config = OnChainConfig::new(BSC, 22055611);
        let v = config.get_pair(
            "0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82",
            "bsc",
            true,
            "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".to_string(),
        );
        assert!(!v.is_empty() && v.len() < 10);
    }

    // #[test]
    // fn test_fetch_token_price() {
    //     let mut config = OnChainConfig::new(BSC, 0);
    //     config.add_moralis_api_key(
    //         "ocJtTEZWOJZjYOMAQjRmWcHpvUdieMLJDAtUjycFNTdSxgFGofNJhdiRX0Kk1h1O".to_string(),
    //     );
    //     let v = config.fetch_token_price(
    //         EVMAddress::from_str("0xa0a2ee912caf7921eaabc866c6ef6fec8f7e90a4"
    // ).unwrap(),     );
    //     debug!("{:?}", v)
    // }
    //
    // #[test]
    // fn test_fetch_storage_all() {
    //     let mut config = OnChainConfig::new(BSC, 0);
    //     let v = config.fetch_storage_all(
    //         EVMAddress::from_str("0x2aB472b185787b665f334F12618254CaCA668e49"
    // ).unwrap(),     );
    //     debug!("{:?}", v)
    // }

    // #[test]
    // fn test_fetch_storage_dump() {
    //     let mut config = OnChainConfig::new(ETH, 0);
    //     let v = config
    //         .fetch_storage_dump(
    //
    // EVMAddress::from_str("0x3ea826a2724f3df727b64db552f3103192158c58").
    // unwrap(),         )
    //         .unwrap();

    //     let v0 = v.get(&EVMU256::from(0)).unwrap().clone();

    //     let slot_v = config.get_contract_slot(
    //         EVMAddress::from_str("0x3ea826a2724f3df727b64db552f3103192158c58"
    // ).unwrap(),         EVMU256::from(0),
    //         false,
    //     );

    //     assert_eq!(slot_v, v0);
    // }
}
