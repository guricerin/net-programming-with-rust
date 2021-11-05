use anyhow::{Context, Result};
use std::{io, net::UdpSocket, str};

pub fn communicate(address: &str) -> Result<()> {
    // 0番ポートを指定すると空いているポートをOSが選んでくれる
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        socket.send_to(input.as_bytes(), address)?;

        let mut buf = [0u8; 1024];
        socket
            .recv_from(&mut buf)
            .with_context(|| format!("failed to receive"))?;
        print!(
            "{}",
            str::from_utf8(&buf).with_context(|| format!("failed to convert to String"))?
        );
    }
}
