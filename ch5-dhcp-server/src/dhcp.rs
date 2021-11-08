use crate::{database, util};
use anyhow::{Context, Result};
use ipnetwork::Ipv4Network;
use log::info;
use pnet::{packet::PrimitiveValues, util::MacAddr};
use rusqlite::Connection;
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{Mutex, RwLock},
};

const OP: usize = 0;
const HTYPE: usize = 1;
const HLEN: usize = 2;
// const HOPS: usize = 3; 今回は使用しない
const XID: usize = 4;
const SECS: usize = 8;
const FLAGS: usize = 10;
const CIADDR: usize = 12;
const YIADDR: usize = 16;
const SIADDR: usize = 20;
const GIADDR: usize = 24;
const CHADDR: usize = 28;
const SNAME: usize = 44;
// const FILE:usize=108; 今回は使用しない
pub const OPTIONS: usize = 236;

/// フィールドだけのDHCPパケットのサイズ
const DHCP_MINIMUM_SIZE: usize = 237;
const OPTION_END: u8 = 255;

/**
 * DHCPのパケットを表現する
 */
pub struct DhcpPacket {
    buf: Vec<u8>,
}

impl DhcpPacket {
    pub fn new(buf: Vec<u8>) -> Option<Self> {
        if buf.len() > DHCP_MINIMUM_SIZE {
            let packet = Self { buf };
            Some(packet)
        } else {
            None
        }
    }

    pub fn get_buffer(&self) -> &[u8] {
        self.buf.as_ref()
    }

    pub fn get_op(&self) -> u8 {
        self.buf[OP]
    }

    pub fn get_options(&self) -> &[u8] {
        &self.buf[OPTIONS..]
    }

    pub fn get_xid(&self) -> &[u8] {
        &self.buf[XID..SECS]
    }

    pub fn get_flags(&self) -> &[u8] {
        &self.buf[FLAGS..CIADDR]
    }

    pub fn get_giaddr(&self) -> Ipv4Addr {
        let b = &self.buf[GIADDR..CHADDR];
        Ipv4Addr::new(b[0], b[1], b[2], b[3])
    }

    pub fn get_chaddr(&self) -> MacAddr {
        let b = &self.buf[CHADDR..SNAME];
        MacAddr::new(b[0], b[1], b[2], b[3], b[4], b[5])
    }

    pub fn get_ciaddr(&self) -> Ipv4Addr {
        let b = &self.buf[CIADDR..YIADDR];
        Ipv4Addr::new(b[0], b[1], b[2], b[3])
    }

    pub fn set_op(&mut self, op: u8) {
        self.buf[OP] = op;
    }

    pub fn set_htype(&mut self, htype: u8) {
        self.buf[HTYPE] = htype;
    }

    pub fn set_hlen(&mut self, hlen: u8) {
        self.buf[HLEN] = hlen;
    }

    pub fn set_xid(&mut self, xid: &[u8]) {
        self.buf[XID..SECS].copy_from_slice(xid);
    }

    pub fn set_ciaddr(&mut self, ciaddr: Ipv4Addr) {
        self.buf[CIADDR..YIADDR].copy_from_slice(&ciaddr.octets());
    }

    pub fn set_yiaddr(&mut self, yiaddr: Ipv4Addr) {
        self.buf[YIADDR..SIADDR].copy_from_slice(&yiaddr.octets());
    }

    pub fn set_flags(&mut self, flags: &[u8]) {
        self.buf[FLAGS..CIADDR].copy_from_slice(flags);
    }

    pub fn set_giaddr(&mut self, giaddr: Ipv4Addr) {
        self.buf[GIADDR..CHADDR].copy_from_slice(&giaddr.octets());
    }

    pub fn set_chaddr(&mut self, chaddr: MacAddr) {
        let t = chaddr.to_primitive_values();
        let macaddr_value = [t.0, t.1, t.2, t.3, t.4, t.5];
        // ここだけCHADDR..SNAMEでないのは、chaddrフィールドが16オクテット書く本されているため。
        // 今回はMACアドレスしかこのフィールドに入らないので、MACアドレスのサイズである6オクテット確保している。
        self.buf[CHADDR..CHADDR + 6].copy_from_slice(&macaddr_value);
    }

    pub fn set_option(
        &mut self,
        cursor: &mut usize,
        code: u8,
        len: usize,
        contents: Option<&[u8]>,
    ) {
        // オプションにはコード番号、サイズ、値の順番で入れる。
        self.buf[*cursor] = code;
        if code == OPTION_END {
            // 値が存在しないため。
            return;
        }
        *cursor += 1;
        self.buf[*cursor] = len as u8;
        *cursor += 1;
        if let Some(contents) = contents {
            self.buf[*cursor..*cursor + contents.len()].copy_from_slice(contents);
        }
        *cursor += len;
    }

    pub fn set_magic_cookie(&mut self, cursor: &mut usize) {
        self.buf[*cursor..*cursor + 4].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        *cursor += 4;
    }

    pub fn get_option(&self, option_code: u8) -> Option<Vec<u8>> {
        // 最初の4バイトはクッキーなのでスキップ。
        let mut index: usize = 4;
        let options = self.get_options();

        while options[index] != OPTION_END {
            if options[index] == option_code {
                // 目的のコードを発見したとき、データはコード、サイズ、データの順に並ぶ。
                let len = options[index + 1];
                let buf_index = index + 2;
                let v = options[buf_index..buf_index + len as usize].to_vec();
                return Some(v);
            } else if options[index] == 0 {
                // パディングなのでスキップ。
                index += 1;
            } else {
                // 目的でない他のオプション。
                index += 1;
                let len = options[index];
                index += 1;
                index += len as usize;
            }
        }
        None
    }
}

pub struct DhcpServer {
    /// 利用可能なアドレス
    address_pool: RwLock<Vec<Ipv4Addr>>,
    /// ConnextionはSyncを実装していないのでRwLockではだめ
    pub db_connection: Mutex<Connection>,
    pub network_addr: Ipv4Network,
    pub server_address: Ipv4Addr,
    pub default_gateway: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub dns_server: Ipv4Addr,
    pub lease_time: Vec<u8>,
}

impl DhcpServer {
    pub fn new() -> Result<Self> {
        let env = util::load_env()?;
        // DNSやゲートウェイなどのアドレス
        let static_addresses = util::obtain_static_addresses(&env)?;
        let network_addr_with_prefix: Ipv4Network = Ipv4Network::new(
            static_addresses["network_addr"],
            ipnetwork::ipv4_mask_to_prefix(static_addresses["subnet_mask"])?,
        )?;
        let conn = Connection::open("dhcp.db")?;
        let addr_pool =
            Self::init_address_pool(&conn, &static_addresses, network_addr_with_prefix)?;
        info!(
            "There are {} addresses in the address pool",
            addr_pool.len()
        );
        let lease_time = util::make_big_endian_vec_from_u32(
            env.get("LEASE_TIME")
                .with_context(|| "Missing lease_time")?
                .parse()?,
        )?;

        Ok(Self {
            address_pool: RwLock::new(addr_pool),
            db_connection: Mutex::new(conn),
            network_addr: network_addr_with_prefix,
            server_address: static_addresses["dhcp_server_addr"],
            default_gateway: static_addresses["default_gateway"],
            subnet_mask: static_addresses["subnet_mask"],
            dns_server: static_addresses["dns_addr"],
            lease_time: lease_time,
        })
    }

    /**
     * 新たなホストに割り当て可能なアドレスプールを初期化。
     */
    fn init_address_pool(
        conn: &Connection,
        static_addresses: &HashMap<String, Ipv4Addr>,
        network_addr_with_prefix: Ipv4Network,
    ) -> Result<Vec<Ipv4Addr>> {
        let network_addr = static_addresses.get("network_addr").context("")?;
        let default_gateway = static_addresses.get("default_gateway").context("")?;
        let dhcp_server_addr = static_addresses.get("dhcp_server_addr").context("")?;
        let dns_server_addr = static_addresses.get("dns_addr").context("")?;
        let broadcast = network_addr_with_prefix.broadcast();

        // すでに使用されていて解放もされていないIPアドレス
        let mut used_ip_addrs = database::select_addresses(conn, Some(0))?;
        used_ip_addrs.push(*network_addr);
        used_ip_addrs.push(*default_gateway);
        used_ip_addrs.push(*dhcp_server_addr);
        used_ip_addrs.push(*dns_server_addr);
        used_ip_addrs.push(broadcast);

        // ネットワークのすべてのIPアドレスから、使用されていなIPアドレスを除いたものをアドレスプールとする。
        let mut addr_pool: Vec<Ipv4Addr> = network_addr_with_prefix
            .iter()
            .filter(|addr| !used_ip_addrs.contains(addr))
            .collect();
        // IPアドレスの若いほうから割り当てたいので逆順にする
        // 取り出すときは末尾からpopする
        addr_pool.reverse();
        Ok(addr_pool)
    }

    /**
     * アドレスプールからIPアドレスを引き抜く。
     */
    pub fn pick_available_ip(&self) -> Option<Ipv4Addr> {
        let mut lock = self.address_pool.write().unwrap();
        lock.pop()
    }

    /**
     * アドレスプールから指定IPアドレスを引き抜く。
     */
    pub fn pick_specified_ip(&self, requested_ip: Ipv4Addr) -> Option<Ipv4Addr> {
        let mut lock = self.address_pool.write().unwrap();
        for i in 0..lock.len() {
            if lock[i] == requested_ip {
                return Some(lock.remove(i));
            }
        }
        None
    }

    /**
     * アドレスプールの先頭にアドレスを渡す。
     * 取り出しは後方から行われるため、返されたアドレスは当分他のホストに割り当てられない。
     */
    pub fn release_address(&self, released_ip: Ipv4Addr) {
        let mut lock = self.address_pool.write().unwrap();
        lock.insert(0, released_ip);
    }
}
