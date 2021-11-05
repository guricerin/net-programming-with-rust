use anyhow::{anyhow, Context, Result};
use env_logger;
use log::info;
use pnet::{
    datalink::{self, Channel::Ethernet},
    packet::{
        ethernet::{EtherTypes, EthernetPacket},
        ip::IpNextHeaderProtocols,
        ipv4::Ipv4Packet,
        ipv6::Ipv6Packet,
        tcp::TcpPacket,
        udp::UdpPacket,
        Packet,
    },
};
use std::env;

mod packets;
use packets::GettableEndPoints;

fn main() -> Result<()> {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err(anyhow!("Please specify target interface name"));
    }

    let interface_name = &args[1];
    // ネットワークインターフェイス（NICや無線LANアダプタを抽象化したもの）の選択
    let interfaces = datalink::interfaces();
    let interface = interfaces
        .into_iter()
        .find(|iface| iface.name == *interface_name)
        .context("Failed to get interface")?;

    // データリンクのチャネルを取得
    let (_tx, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => return Err(anyhow!("Unhandled channel type")),
        Err(e) => {
            let msg = format!("Failed to create datalink channel {}", e);
            return Err(anyhow!(msg));
        }
    };

    loop {
        match rx.next() {
            Ok(frame) => {
                // 受信パケットからイーサネットフレームの構築
                let frame =
                    EthernetPacket::new(frame).context("Failed to make a EthernetPacket")?;
                match frame.get_ethertype() {
                    EtherTypes::Ipv4 => {
                        ipv4_handler(&frame);
                    }
                    EtherTypes::Ipv6 => {
                        ipv6_handler(&frame);
                    }
                    _ => {
                        info!("Not an IPv4 or IPv6");
                    }
                }
            }
            Err(e) => {
                eprintln!("{:?}", e);
            }
        }
    }
}

/**
 * IPv4パケットを構築し次のレイヤのハンドラを呼び出す
 */
fn ipv4_handler(frame: &EthernetPacket) {
    // フレームを剥いてパケットを取り出す
    if let Some(packet) = Ipv4Packet::new(frame.payload()) {
        match packet.get_next_level_protocol() {
            IpNextHeaderProtocols::Tcp => {
                tcp_handler(&packet);
            }
            IpNextHeaderProtocols::Udp => {
                udp_handler(&packet);
            }
            _ => {
                info!("Not a TCP or UDP packet");
            }
        }
    }
}

/**
 * IPv6パケットを構築し次のレイヤのハンドラを呼び出す
 */
fn ipv6_handler(frame: &EthernetPacket) {
    // フレームを剥いてパケットを取り出す
    if let Some(packet) = Ipv6Packet::new(frame.payload()) {
        match packet.get_next_header() {
            IpNextHeaderProtocols::Tcp => {
                tcp_handler(&packet);
            }
            IpNextHeaderProtocols::Udp => {
                udp_handler(&packet);
            }
            _ => {
                info!("Not a TCP or UDP packet");
            }
        }
    }
}

/**
 * TCPパケットを構築する
 */
fn tcp_handler<T: GettableEndPoints>(packet: &T) {
    let tcp = TcpPacket::new(packet.get_payload());
    if let Some(tcp) = tcp {
        print_packet_info(packet, &tcp, "TCP");
    }
}

/**
 * UDPパケットを構築する
 */
fn udp_handler<T: GettableEndPoints>(packet: &T) {
    let udp = UdpPacket::new(packet.get_payload());
    if let Some(udp) = udp {
        print_packet_info(packet, &udp, "UDP");
    }
}

const WIDTH: usize = 20;

/**
 * アプリケーション層のデータをバイナリで表示する
 */
fn print_packet_info<T: GettableEndPoints, S: GettableEndPoints>(l3: &T, l4: &S, proto: &str) {
    println!(
        "Captured a {} packet from {}|{} to {}|{}\n",
        proto,
        l3.get_source(),
        l4.get_source(),
        l3.get_destination(),
        l4.get_destination(),
    );

    let payload = l4.get_payload();
    let len = payload.len();
    // ペイロード部の表示
    for i in 0..len {
        // 16進数で表示
        print!("{:<02X} ", payload[i]);

        // asciiで表示
        if i % WIDTH == WIDTH - 1 || i == len - 1 {
            for _ in 0..WIDTH - 1 - (i % WIDTH) {
                print!("    ");
            }
            print!("|  ");
            for j in i - i % WIDTH..=i {
                if payload[j].is_ascii_alphabetic() {
                    print!("{}", payload[j] as char);
                } else {
                    // 非ascii文字は.で表示
                    print!(".");
                }
            }
            println!();
        }
    }
    println!("{}", "=".repeat(WIDTH * 3));
    println!();
}
