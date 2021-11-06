use anyhow::{anyhow, Context, Result};
use env_logger;
use pnet::{
    packet::{
        ip::IpNextHeaderProtocols,
        tcp::{self, MutableTcpPacket, TcpFlags},
    },
    transport::{
        self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
    },
};
use std::{
    collections::HashMap,
    env, fs,
    net::{IpAddr, Ipv4Addr},
    str, thread,
    time::Duration,
};

#[derive(Clone, Copy)]
enum ScanType {
    Syn = TcpFlags::SYN as isize,
    Fin = TcpFlags::FIN as isize,
    Xmax = (TcpFlags::FIN | TcpFlags::URG | TcpFlags::PSH) as isize,
    Null = 0,
}

struct PacketInfo {
    my_ipaddr: Ipv4Addr,
    target_ipaddr: Ipv4Addr,
    my_port: u16,
    maximum_port: u16,
    scan_type: ScanType,
}

impl PacketInfo {
    pub fn new(target_ipaddr: &str, scan_type: &str) -> Result<Self> {
        let contents = fs::read_to_string(".env").context("Failed to read env file")?;
        let lines: Vec<_> = contents.split('\n').collect();
        let mut map = HashMap::new();
        for line in lines {
            let elm: Vec<_> = line.split('=').map(str::trim).collect();
            if elm.len() == 2 {
                map.insert(elm[0], elm[1]);
            }
        }

        let res = Self {
            my_ipaddr: map["MY_IPADDR"].parse().context("invalid your ipaddr")?,
            target_ipaddr: target_ipaddr.parse().context("invalid target ipaddr")?,
            my_port: map["MY_PORT"].parse().context("invalid your port number")?,
            maximum_port: map["MAXIMUM_PORT_NUM"]
                .parse()
                .context("invalid maximum port num")?,
            scan_type: match scan_type {
                "sS" => ScanType::Syn,
                "sF" => ScanType::Fin,
                "sX" => ScanType::Xmax,
                "sN" => ScanType::Null,
                _ => return Err(anyhow!("Undefined scan method, only accept [sS|sF|sN|sX].")),
            },
        };
        Ok(res)
    }
}

fn main() -> Result<()> {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        return Err(anyhow!("Bad number of arguments. [ipaddr] [scantype]"));
    }

    let packet_info = PacketInfo::new(&args[1], &args[2])?;

    // トランスポート層のチャネルを開く
    // 内部的にはソケット
    let (mut ts, mut tr) = transport::transport_channel(
        1024,
        TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
    )
    .context("Failed to opne channel.")?;

    // パケットの送信と受信を並行に行う
    rayon::join(
        || send_packet(&mut ts, &packet_info).unwrap(),
        || receive_packets(&mut tr, &packet_info).unwrap(),
    );
    Ok(())
}

/**
 * 指定のレンジにパケットを送信する
 */
fn send_packet(ts: &mut TransportSender, packet_info: &PacketInfo) -> Result<()> {
    let mut packet = build_packet(packet_info)?;
    for i in 1..=packet_info.maximum_port {
        let mut tcp_header = MutableTcpPacket::new(&mut packet).context("invalid packet")?;
        register_destination_port(i, &mut tcp_header, packet_info);
        thread::sleep(Duration::from_millis(5));
        ts.send_to(tcp_header, IpAddr::V4(packet_info.target_ipaddr))?;
    }
    Ok(())
}

/**
 * TCPヘッダの宛先ポート情報を書き換える
 * チェックサムを計算し直す必要がある
 */
fn register_destination_port(
    target: u16,
    tcp_header: &mut MutableTcpPacket,
    packet_info: &PacketInfo,
) {
    tcp_header.set_destination(target);
    let checksum = tcp::ipv4_checksum(
        &tcp_header.to_immutable(),
        &packet_info.my_ipaddr,
        &packet_info.target_ipaddr,
    );
    tcp_header.set_checksum(checksum);
}

/**
 * パケットを受信してスキャン情報を出力する
 */
fn receive_packets(tr: &mut TransportReceiver, packet_info: &PacketInfo) -> Result<()> {
    let mut reply_ports = vec![];
    let mut packet_iter = transport::tcp_packet_iter(tr);

    loop {
        // ターゲットからの通信パケット
        let tcp_packet = match packet_iter.next() {
            Ok((tcp_packet, _)) => {
                if tcp_packet.get_destination() == packet_info.my_port {
                    tcp_packet
                } else {
                    continue;
                }
            }
            Err(_) => {
                continue;
            }
        };

        let target_port = tcp_packet.get_source();
        match packet_info.scan_type {
            ScanType::Syn => {
                if tcp_packet.get_flags() == TcpFlags::SYN | TcpFlags::ACK {
                    println!("port {} is open", target_port);
                }
            }
            // SYNスキャン以外はレスポンスが返ってきたポート（=閉じているポート）を記録
            ScanType::Fin | ScanType::Xmax | ScanType::Null => {
                reply_ports.push(target_port);
            }
        }

        // 手抜き：スキャン対象の最後のポートに対する返信が返ってこれば終了
        // フロー制御や再送制御などのTCPの機能を実装していないため
        if target_port != packet_info.maximum_port {
            continue;
        }
        match packet_info.scan_type {
            ScanType::Fin | ScanType::Xmax | ScanType::Null => {
                for i in 1..=packet_info.maximum_port {
                    // 返信のないポートを開いているものと判断
                    if reply_ports.iter().find(|&&x| x == i).is_none() {
                        println!("port {} is open", i);
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }
}

const TCP_SIZE: usize = 20;

/**
 * パケットを生成する
 */
fn build_packet(packet_info: &PacketInfo) -> Result<[u8; TCP_SIZE]> {
    // TCPヘッダの作成
    let mut tcp_buffer = [0u8; TCP_SIZE];
    let mut tcp_header =
        MutableTcpPacket::new(&mut tcp_buffer[..]).context("Failed to make MutableTcpPacket")?;

    tcp_header.set_source(packet_info.my_port);
    // オプションを含まないので、20オクテットまでがTCPヘッダ。4オクテット単位で指定する。
    tcp_header.set_data_offset(5);
    tcp_header.set_flags(packet_info.scan_type as u16);
    let checksum = tcp::ipv4_checksum(
        &tcp_header.to_immutable(),
        &packet_info.my_ipaddr,
        &packet_info.target_ipaddr,
    );
    tcp_header.set_checksum(checksum);

    Ok(tcp_buffer)
}
