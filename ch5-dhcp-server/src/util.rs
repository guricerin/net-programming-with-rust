use anyhow::{anyhow, Context, Result};
use byteorder::{BigEndian, WriteBytesExt};
use log::{debug, info};
use pnet::{
    packet::{
        icmp::{
            echo_request::{EchoRequestPacket, MutableEchoRequestPacket},
            IcmpTypes,
        },
        ip::IpNextHeaderProtocols,
        util::checksum,
        Packet,
    },
    transport::{self, icmp_packet_iter, TransportChannelType, TransportProtocol},
};
use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::mpsc,
    thread,
    time::Duration,
};

/**
 * .envから環境情報を読んでハッシュマップを返す
 */
pub fn load_env() -> Result<HashMap<String, String>> {
    let contents = fs::read_to_string(".env").with_context(|| "Failed to read env file.")?;
    let lines: Vec<_> = contents.split('\n').collect();
    let mut map = HashMap::new();
    for line in lines {
        let elm: Vec<_> = line.split('=').map(str::trim).collect();
        if elm.len() == 2 {
            map.insert(elm[0].to_string(), elm[1].to_string());
        }
    }
    Ok(map)
}

pub fn obtain_static_addresses(env: &HashMap<String, String>) -> Result<HashMap<String, Ipv4Addr>> {
    let network_addr = env
        .get("NETWORK_ADDR")
        .with_context(|| "Missing network_addr")?
        .parse()?;

    let subnet_mask = env
        .get("SUBNET_MASK")
        .with_context(|| "Missing subnet_mask")?
        .parse()?;

    let dhcp_server_address = env
        .get("SERVER_IDENTIFIER")
        .with_context(|| "Missing server_identifier")?
        .parse()?;

    let default_gateway = env
        .get("DEFAULT_GATEWAY")
        .with_context(|| "Missing default_gateway")?
        .parse()?;

    let dns_addr = env
        .get("DNS_SERVER")
        .with_context(|| "Missing dns_server")?
        .parse()?;

    let mut map = HashMap::new();
    map.insert("network_addr".to_string(), network_addr);
    map.insert("subnet_mask".to_string(), subnet_mask);
    map.insert("dhcp_server_addr".to_string(), dhcp_server_address);
    map.insert("default_gateway".to_string(), default_gateway);
    map.insert("dns_addr".to_string(), dns_addr);
    Ok(map)
}

/**
 * IPアドレスが使用可能か調べる。
 */
pub fn is_ipaddr_available(target_ip: Ipv4Addr) -> Result<()> {
    let icmp_buf = create_default_icmp_buffer();
    // ARPではなくICMPを利用する。別のセグメントにも送信可能なため。
    let icmp_packet = EchoRequestPacket::new(&icmp_buf).with_context(|| "")?;
    let (mut transport_sender, mut transport_receiver) = transport::transport_channel(
        1024,
        TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Icmp)),
    )?;
    transport_sender.send_to(icmp_packet, IpAddr::V4(target_ip))?;

    let (sender, receiver) = mpsc::channel();

    // ICMP echoリクエストのリプライに対してタイムアウトを設定するため、スレッドを起動する。
    // このスレッドはEchoリプライを受信するまで残り続ける。
    thread::spawn(move || {
        let mut iter = icmp_packet_iter(&mut transport_receiver);
        let (packet, _) = iter.next().unwrap();
        if packet.get_icmp_type() == IcmpTypes::EchoReply {
            match sender.send(true) {
                Err(_) => {
                    // 制御時間を超過してリプライが届いた場合。
                    info!("icmp timeout");
                }
                _ => {
                    // 送信できた場合はなにもせず終了。
                    return;
                }
            }
        }
    });

    if receiver.recv_timeout(Duration::from_millis(200)).is_ok() {
        // 制限時間内にEchoリプライが届いた場合、アドレスは使われている。
        Err(anyhow!("ip addr already in use:{}", target_ip))
    } else {
        // タイムアウトした場合、アドレスは使われていない。
        debug!("not received reply within timeout");
        Ok(())
    }
}

/**
 * スライスをIpv4アドレスに変換する。
 */
pub fn u8_to_ipv4addr(buf: &[u8]) -> Option<Ipv4Addr> {
    if buf.len() == 4 {
        Some(Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]))
    } else {
        None
    }
}

/**
 * ブロードキャストでDHCPクライアントにデータを送信する。
 */
pub fn send_dhcp_broadcast_response(soc: &UdpSocket, data: &[u8]) -> Result<()> {
    let destination: SocketAddr = "255.255.255.255:68".parse()?;
    soc.send_to(data, destination)?;
    Ok(())
}

/**
 * ICMP echoリクエストのバッファを作成する。
 * ICMP : IPの補佐的プロトコル。Pingなどネットワークの疎通確認に使われる。
 */
fn create_default_icmp_buffer() -> [u8; 8] {
    let mut buf = [0u8; 8];
    let mut icmp_packet = MutableEchoRequestPacket::new(&mut buf).unwrap();
    icmp_packet.set_icmp_type(IcmpTypes::EchoRequest);
    let checksum = checksum(icmp_packet.to_immutable().packet(), 16);
    icmp_packet.set_checksum(checksum);
    buf
}

pub fn make_big_endian_vec_from_u32(i: u32) -> Result<Vec<u8>> {
    let mut v = vec![];
    v.write_u32::<BigEndian>(i)?;
    Ok(v)
}
