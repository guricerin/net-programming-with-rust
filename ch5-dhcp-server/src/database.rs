use std::net::Ipv4Addr;

use anyhow::{anyhow, Result};
use log::info;
use pnet::util::MacAddr;
use rusqlite::{params, Connection, Rows, Transaction};

/**
 * 利用されているIPアドレスを返す。
 * deletedが渡された場合は'deleted'カラムをその条件で絞り込む。
 */
pub fn select_addresses(conn: &Connection, deleted: Option<u8>) -> Result<Vec<Ipv4Addr>> {
    if let Some(deleted) = deleted {
        let mut statement = conn.prepare("select ip_addr from lease_entries where deleted = ?")?;
        let ip_addrs = statement.query(params![deleted.to_string()])?;
        get_addresses_from_row(ip_addrs)
    } else {
        let mut statement = conn.prepare("select ip_addr from lease_entries")?;
        let ip_addrs = statement.query([])?;
        get_addresses_from_row(ip_addrs)
    }
}

/**
 * 結果のレコードからIPアドレスのカラムを取り出し、そのベクタを返す。
 */
fn get_addresses_from_row(mut ip_addrs: Rows) -> Result<Vec<Ipv4Addr>> {
    let mut leased_addrs = vec![];
    while let Some(entry) = ip_addrs.next()? {
        let ip_addr = match entry.get(0) {
            Ok(ip) => {
                let ip: String = ip;
                ip.parse()?
            }
            Err(_) => continue,
        };
        leased_addrs.push(ip_addr);
    }
    Ok(leased_addrs)
}

/**
 * バインディングの追加
 */
pub fn insert_entry(tx: &Transaction, mac_addr: MacAddr, ip_addr: Ipv4Addr) -> Result<()> {
    tx.execute(
        "insert into lease_entries (mac_addr, ip_addr) values (?1, ?2)",
        params![mac_addr.to_string(), ip_addr.to_string()],
    )?;
    Ok(())
}

/**
 * 指定のMACアドレスをもつエントリ（論理削除されているものを含む）のIPアドレスを返す。
 */
pub fn select_entry(conn: &Connection, mac_addr: MacAddr) -> Result<Option<Ipv4Addr>> {
    let mut stmt = conn.prepare("select ip_addr from lease_entries where mac_addr = ?1")?;
    let mut row = stmt.query(params![mac_addr.to_string()])?;
    if let Some(entry) = row.next()? {
        let ip = entry.get(0)?;
        let ip: String = ip;
        Ok(Some(ip.parse()?))
    } else {
        info!("specified MAC addr was not found.");
        Ok(None)
    }
}

/**
 * バインディングの更新。
 */
pub fn update_entry(
    tx: &Transaction,
    mac_addr: MacAddr,
    ip_addr: Ipv4Addr,
    deleted: u8,
) -> Result<()> {
    tx.execute(
        "update lease_entries set ip_addr = ?2, deleted = ?3 where mac_addr = ?1",
        params![
            mac_addr.to_string(),
            ip_addr.to_string(),
            deleted.to_string()
        ],
    )?;
    Ok(())
}

/**
 * バインディングの論理削除
 */
pub fn delete_entry(tx: &Transaction, mac_addr: MacAddr) -> Result<()> {
    tx.execute(
        "update lease_entries set deleted = ?1 where mac_addr = ?2",
        params![1.to_string(), mac_addr.to_string()],
    )?;
    Ok(())
}

/**
 * 指定MACアドレスをもつレコードの件数を返す。
 */
pub fn count_records_by_macaddr(tx: &Transaction, mac_addr: MacAddr) -> Result<u8> {
    let mut stmt = tx.prepare("select count (*) from lease_entries where mac_addr = ?")?;
    let mut count_result = stmt.query(params![mac_addr.to_string()])?;

    let count: u8 = match count_result.next()? {
        Some(row) => row.get(0)?,
        None => {
            // countの結果なので基本的に起こり得ない。
            return Err(anyhow!("No query returned."));
        }
    };
    Ok(count)
}
