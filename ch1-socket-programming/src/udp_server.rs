use anyhow::Result;
use log::debug;
use std::{net::UdpSocket, str};

pub fn serve(address: &str) -> Result<()> {
    let server_socket = UdpSocket::bind(address)?;
    loop {
        let mut buf = [0u8; 1024];
        // 1つのソケットがすべてのクライアントととの通信をさばく
        let (size, src) = server_socket.recv_from(&mut buf)?;
        debug!("Handling data from {}", src);
        print!("{}", str::from_utf8(&buf[..size])?);
        server_socket.send_to(&buf, src)?;
    }
}
