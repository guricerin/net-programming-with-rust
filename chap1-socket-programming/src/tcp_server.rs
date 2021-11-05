use anyhow::Result;
use log::debug;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    str, thread,
};

/**
 * 指定のソケットアドレスで接続を待ち受ける
 */
pub fn serve(address: &str) -> Result<()> {
    // TCPコネクションを待ち受けるソケットを作成する
    let listener = TcpListener::bind(address)?;
    loop {
        let (stream, _) = listener.accept()?;
        // スレッドを立ち上げて接続に対処する
        // スレッドなのは、複数のリクエストを同時にさばくため
        thread::spawn(move || {
            handler(stream).unwrap_or_else(|error| eprintln!("{:?}", error));
        });
    }
}

/**
 * クライアントからの入力を待ち受け、受信したら同じものを返却
 */
fn handler(mut stream: TcpStream) -> Result<()> {
    debug!("Handling data from {}", stream.peer_addr()?);
    let mut buffer = [0u8; 1024];
    loop {
        let nbytes = stream.read(&mut buffer)?;
        if nbytes == 0 {
            debug!("Connection closed.");
            return Ok(());
        }
        print!("{}", str::from_utf8(&buffer[..nbytes])?);
        stream.write_all(&buffer[..nbytes])?;
    }
}
