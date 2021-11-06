use anyhow::{anyhow, Context, Result};
use env_logger;
use log::{debug, error};
use mio::{
    net::{TcpListener, TcpStream},
    Event, Events, Poll, PollOpt, Ready, Token,
};
use regex::Regex;
use std::{
    collections::HashMap,
    env,
    fs::File,
    io::{BufReader, Read, Write},
    str,
};

/// リスニングソケットのトークン
const SERVER: Token = Token(0);

/// ドキュメントルート
const WEBROOT: &str = "/webroot";

struct WebServer {
    /// ノンブロッキング
    listening_socket: TcpListener,
    /// サーバに接続されているクライアントを管理する
    connections: HashMap<usize, TcpStream>,
    next_connection_id: usize,
}

impl WebServer {
    fn new(addr: &str) -> Result<Self> {
        let address = addr.parse()?;
        let listening_socket = TcpListener::bind(&address)?;
        Ok(WebServer {
            listening_socket,
            connections: HashMap::new(),
            next_connection_id: 1,
        })
    }

    fn run(&mut self) -> Result<()> {
        let poll = Poll::new()?;
        // サーバソケットの状態を監視対象に登録する
        poll.register(
            &self.listening_socket,
            SERVER,
            Ready::readable(),
            PollOpt::level(),
        )?;

        // イベントキュー
        let mut events = Events::with_capacity(1024);
        // HTTPレスポンス用のバッファ
        let mut response = vec![];
        loop {
            // 現在のスレッドをブロックしてイベントを待つ
            match poll.poll(&mut events, None) {
                Ok(_) => {}
                Err(e) => {
                    error!("{}", e);
                    continue;
                }
            }
            for event in &events {
                match event.token() {
                    SERVER => {
                        // リスニングソケットの読み込み準備完了イベントが発生
                        let (stream, remote) = match self.listening_socket.accept() {
                            Ok(t) => t,
                            Err(e) => {
                                error!("{}", e);
                                continue;
                            }
                        };
                        debug!("Connection from {}", &remote);
                        // 接続済みソケットを監視対象に登録
                        self.register_connection(&poll, stream)
                            .unwrap_or_else(|e| error!("{}", e));
                    }

                    Token(conn_id) => {
                        // 接続済みソケットでイベントが発生
                        self.http_handler(conn_id, event, &poll, &mut response)
                            .unwrap_or_else(|e| error!("{}", e));
                    }
                }
            }
        }
    }

    /**
     * 接続済みソケットを監視対象に登録する
     */
    fn register_connection(&mut self, poll: &Poll, stream: TcpStream) -> Result<()> {
        let token = Token(self.next_connection_id);
        poll.register(&stream, token, Ready::readable(), PollOpt::edge())?;

        if self
            .connections
            .insert(self.next_connection_id, stream)
            .is_some()
        {
            // 既存のキーで値が更新されると更新前の値を返す
            error!("Connection ID is already exists.");
        }
        self.next_connection_id += 1;
        Ok(())
    }

    /**
     * 接続済みソケットで発生したイベントのハンドラ
     */
    fn http_handler(
        &mut self,
        conn_id: usize,
        event: Event,
        poll: &Poll,
        response: &mut Vec<u8>,
    ) -> Result<()> {
        let stream = self
            .connections
            .get_mut(&conn_id)
            .context("Failed to get connection.")?;

        if event.readiness().is_readable() {
            // ソケットから読み込み可能
            debug!("readable conn_id: {}", conn_id);
            let mut buf = [0u8; 1024];
            let nbytes = stream.read(&mut buf)?;

            if nbytes != 0 {
                *response = make_response(&buf[..nbytes])?;
                // 書き込み操作の可否を監視対象に登録する
                // ソケットの送信バッファが満杯の場合にブロックが発生するので、すぐには返信しない
                poll.reregister(stream, Token(conn_id), Ready::writable(), PollOpt::edge())?;
            } else {
                // 通信終了
                self.connections.remove(&conn_id);
            }
            Ok(())
        } else if event.readiness().is_writable() {
            // ソケットに書き込み可能
            debug!("writable conn_id: {}", conn_id);
            stream.write_all(response)?;
            self.connections.remove(&conn_id);
            Ok(())
        } else {
            Err(anyhow!("Undefined event."))
        }
    }
}

/**
 * レスポンスをバイト列で作成して返す
 */
fn make_response(buf: &[u8]) -> Result<Vec<u8>> {
    let http_pattern = Regex::new(r"(.*) (.*) HTTP/1.([0-1])\r\n.*")?;
    let captures = match http_pattern.captures(str::from_utf8(buf)?) {
        Some(cap) => cap,
        None => {
            // 不正なリクエスト
            return create_msg_from_code(400, None);
        }
    };
    let method = captures[1].to_string();
    let path = format!(
        "{}{}{}",
        env::current_dir()?.display(),
        WEBROOT,
        &captures[2]
    );
    let _version = captures[3].to_string();

    match method.as_str() {
        "GET" => {
            let file = match File::open(path) {
                Ok(file) => file,
                Err(_) => {
                    // パーミッションエラーなどもここに含まれるが簡略化のためnot foundにしている
                    return create_msg_from_code(404, None);
                }
            };
            let mut reader = BufReader::new(file);
            let mut buf = vec![];
            reader.read_to_end(&mut buf)?;
            create_msg_from_code(200, Some(buf))
        }
        _ => create_msg_from_code(501, None),
    }
}

fn create_msg_from_code(status_code: u16, msg: Option<Vec<u8>>) -> Result<Vec<u8>> {
    match status_code {
        200 => {
            let mut header = "HTTP/1.0 200 OK\r\nServer: mio webserver\r\n\r\n"
                .to_string()
                .into_bytes();
            if let Some(mut msg) = msg {
                header.append(&mut msg);
            }
            Ok(header)
        }
        400 => Ok("HTTP/1.0 400 Bad Request\r\nServer: mio webserver\r\n\r\n"
            .to_string()
            .into_bytes()),
        404 => Ok("HTTP/1.0 404 Not Found\r\nServer: mio webserver\r\n\r\n"
            .to_string()
            .into_bytes()),
        501 => Ok(
            "HTTP/1.0 501 Not Implemented\r\nServer: mio webserver\r\n\r\n"
                .to_string()
                .into_bytes(),
        ),
        _ => Err(anyhow!("Undefined status code.")),
    }
}
fn main() -> Result<()> {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err(anyhow!("Bad number of arguments."));
    }

    let mut server = WebServer::new(&args[1])?;
    server.run()?;

    Ok(())
}
