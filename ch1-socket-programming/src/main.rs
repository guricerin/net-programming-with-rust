use anyhow::{anyhow, Result};
use env_logger;
use std::env;

mod tcp_client;
mod tcp_server;

fn main() -> Result<()> {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err(anyhow!(
            "Please specify [tcp|udp] [server|client] [addr:port]."
        ));
    }

    let protocol: &str = &args[1];
    let role: &str = &args[2];
    let address = &args[3];
    match protocol {
        "tcp" => match role {
            "server" => {
                tcp_server::serve(address)?;
            }
            "client" => {
                tcp_client::connect(address)?;
            }
            _ => {
                missing_role()?;
            }
        },
        "udp" => match role {
            "server" => {
                // todo
            }
            "client" => {
                // todo
            }
            _ => {
                missing_role()?;
            }
        },
        _ => {
            return Err(anyhow!("Please specify tcp or udp on the 1st argument."));
        }
    }

    Ok(())
}

fn missing_role() -> Result<()> {
    Err(anyhow!(
        "Please specify server or client on the 2nd argument."
    ))
}
