use std::f32::consts::E;

use anyhow::{anyhow, Result, Error};
use clap::{Arg, ArgMatches, Command};

use ibc_proto::protobuf::Protobuf;
use proto_messages::cosmos::bank::v1beta1::{QueryAllBalancesRequest, QueryAllBalancesResponse};
use proto_types::AccAddress;
use tendermint_rpc::{Client, HttpClient};
use tokio::runtime::Runtime;

pub fn get_bank_query_command() -> Command {
    Command::new("bank")
        .about("Querying commands for the bank module")
        .subcommand(
            Command::new("balances")
                .about("Query for account balances by address")
                .arg(
                    Arg::new("address")
                        .required(true)
                        .value_parser(clap::value_parser!(AccAddress)),
                ),
        )
        .subcommand_required(true)
}

pub fn run_bank_query_command(matches: &ArgMatches, node: &str) -> Result<String> {
    println!("开始进行bank query的查询了, 准备好了");
    let client = HttpClient::new(node)?;
    println!("开始启动子命令...");
    match matches.subcommand() {
        Some(("balances", sub_matches)) => {
            println!("查询余额中...");
            let address = sub_matches
                .get_one::<AccAddress>("address")
                .expect("address argument is required preventing `None`")
                .to_owned();
            println!("地址是多少{:?}", address);
            println!("client is {:?}", client);

            let result = Runtime::new()
                .expect("unclear why this would ever fail")
                .block_on(get_balances(client, address))
                .map_err(|err| anyhow!("Error: {}", err))?;
            println!("result is {:?}", result);

            Ok(result)
        }
        _ => unreachable!("exhausted list of subcommands and subcommand_required prevents `None`"),
    }
}

pub async fn get_balances(client: HttpClient, address: AccAddress) -> Result<String> {
    println!("开始get balance");
    let query = QueryAllBalancesRequest {
        address,
        pagination: None,
    };
    println!("开始获取账户余额.....");
    let a = "/cosmos.bank.v1beta1.Query/AllBalances"
                    .parse()
                    .expect("hard coded path will always succeed");
    println!("a is {:?}", a);
    let b = query.encode_vec();
    println!("b is {:?}", b);
    
    let res1 = client
        .abci_query(
            Some(
                a,
            ),
            b,
            None,
            false,
        )
        .await;

    match res1 {
            Ok(res) => {
                if res.code.is_err() {
                    return Err(anyhow!("node returned an error: {}", res.log));
                }
            
                let res = QueryAllBalancesResponse::decode(&*res.value)?;
                return Ok(serde_json::to_string_pretty(&res)?)
            }
            Err(err) => {
                println!("出现了错误：{:?}", err);
                Ok(String::from("nnnnnn"))
            }
        }
}
