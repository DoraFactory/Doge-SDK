use std::hash::Hash;
use std::path::PathBuf;

use axum::body::Body;
use axum::Router;
use clap::{arg, value_parser, Arg, ArgAction, ArgMatches, Command};
use database::RocksDB;
use proto_messages::cosmos::tx::v1beta1::Message;
use serde::de::DeserializeOwned;
use store_crate::StoreKey;
use strum::IntoEnumIterator;
use tendermint_abci::ServerBuilder;
use tracing::metadata::LevelFilter;
use tracing::{error, info};
use std::thread;
use crate::baseapp::BaseApp;
use crate::client::rest::run_rest_server;
//use crate::client::rest::run_rest_server;
use crate::utils::get_default_home_dir;
use crate::x::params::{Keeper, ParamsSubspaceKey};

use super::ante::{AuthKeeper, BankKeeper};
use super::Handler;
use network::reactor::Reactor;

pub fn run_run_command<
    SK: Hash + Eq + IntoEnumIterator + StoreKey + Clone + Send + Sync + 'static,
    PSK: ParamsSubspaceKey + Clone + Send + Sync + 'static,
    M: Message,
    BK: BankKeeper<SK> + Clone + Send + Sync + 'static,
    AK: AuthKeeper<SK> + Clone + Send + Sync + 'static,
    H: Handler<M, SK, G> + 'static,
    G: DeserializeOwned + Clone + Send + Sync + 'static,
>(
    matches: &ArgMatches,
    app_name: &'static str,
    app_version: &'static str,
    bank_keeper: BK,
    auth_keeper: AK,
    params_keeper: Keeper<SK, PSK>,
    params_subspace_key: PSK,
    handler: H,
    router: Router<BaseApp<SK, PSK, M, BK, AK, H, G>, Body>,
) {
    let host = matches
        .get_one::<String>("host")
        .expect("Host arg has a default value so this cannot be `None`");

    let port = matches
        .get_one::<u16>("port")
        .expect("Port arg has a default value so this cannot be `None`");

    let read_buf_size = matches
        .get_one::<usize>("read_buf_size")
        .expect("Read buf size arg has a default value so this cannot be `None`.");

    let rest_port = matches
        .get_one::<u16>("rest_port")
        .expect("REST port arg has a default value so this cannot be `None`")
        .to_owned();

    let verbose = matches.get_flag("verbose");
    let quiet = matches.get_flag("quiet");

    let log_level = if quiet {
        LevelFilter::OFF
    } else if verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let default_home_directory = get_default_home_dir(app_name);
    let home = matches
        .get_one::<PathBuf>("home")
        .or(default_home_directory.as_ref())
        .unwrap_or_else(|| {
            error!("Home argument not provided and OS does not provide a default home directory");
            std::process::exit(1)
        });
    info!("Using directory {} for config and data", home.display());

    let mut db_dir = home.clone();
    db_dir.push("data");
    db_dir.push("application.db");
    let db = RocksDB::new(db_dir).unwrap_or_else(|e| {
        error!("Could not open database: {}", e);
        std::process::exit(1)
    });

    let mut db_dir = home.clone();
    db_dir.push("data");
    db_dir.push("application2.db");

    let p2p_reactor = tokio::runtime::Runtime::new()
        .expect("unclear why this would ever fail")
        .block_on(
             Reactor::new()
        );

    let mut app: BaseApp<SK, PSK, M, BK, AK, H, G> = BaseApp::new(
        db,
        app_name,
        app_version,
        p2p_reactor,
        bank_keeper,
        auth_keeper,
        params_keeper,
        params_subspace_key,
        handler,
    );

    let mut app_clone = app.clone();

    // Run rest server
    info!("start the rest server");
    run_rest_server(app.clone(), rest_port, router);

    let host_clone = host.clone();
    let port_clone = port.clone();
    let read_buf_size_clone = *read_buf_size;
    let app_clone = app.clone();
    let app_clone_2 = app.clone();

    // Run abci server
    let server_thread = thread::spawn(move || {
        let server = ServerBuilder::new(read_buf_size_clone)
            .bind(format!("{}:{}", host_clone, port_clone), app_clone.clone())
            .unwrap_or_else(|e| {
                error!("Error binding to host: {}", e);
                std::process::exit(1);
            });
        info!("start the abci server");
        server.listen().unwrap_or_else(|e| {
            error!("Fatal server error: {}", e);
            std::process::exit(1);
        });
    });
    
    // Run p2p server
    tokio::runtime::Runtime::new()
        .expect("unclear why this would ever fail")
        .block_on(
            app.run_p2p_reactor()
        );

}

pub fn get_run_command(app_name: &str) -> Command {
    Command::new("run")
        .about("Run the full node application")
        .arg(
            arg!(--home)
                .help(format!(
                    "Directory for config and data [default: {}]",
                    get_default_home_dir(app_name)
                        .unwrap_or_default()
                        .display()
                        .to_string()
                ))
                .action(ArgAction::Set)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(--host)
                .help("Bind the TCP server to this host")
                .action(ArgAction::Set)
                .value_parser(value_parser!(String))
                .default_value("127.0.0.1"),
        )
        .arg(
            arg!(-p - -port)
                .help("Bind the TCP server to this port")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u16))
                .default_value("26658"),
        )
        .arg(
            arg!(--rest_port)
                .help("Bind the REST server to this port")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u16))
                .default_value("1317"),
        )
        .arg(
            arg!(-r - -read_buf_size)
                .help(
                    "The default server read buffer size, in bytes, for each incoming client
                connection",
                )
                .action(ArgAction::Set)
                .value_parser(value_parser!(usize))
                .default_value("1048576"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue)
                .help("Increase output logging verbosity to DEBUG level"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("Suppress all output logging (overrides --verbose)"),
        )
}
