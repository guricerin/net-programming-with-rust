use anyhow::Result;
use std::{
    io::{self, BufRead, BufReader, Write},
    net::TcpStream,
    str,
};

/**
 * 指定のIPアドレス、ポート番号にTCP接続する
 */
pub fn connect(address: &str) -> Result<()> {
    let mut stream = TcpStream::connect(address)?;
    loop {
        // 入力データをソケットから送信する
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        stream.write_all(input.as_bytes())?;

        // ソケットから受信したデータを表示する
        let mut reader = BufReader::new(&stream);
        let mut buffer = vec![];
        reader.read_until(b'\n', &mut buffer)?;
        print!("{}", str::from_utf8(&buffer)?);
    }
}
