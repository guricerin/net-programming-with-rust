## Usage

- ``.env``ファイルを用意する

例：

```
NETWORK_ADDR = 192.168.0.0
SUBNET_MASK = 255.255.255.0
SERVER_IDENTIFIER = 192.168.0.2
DEFAULT_GATEWAY = 192.168.0.1
DNS_SERVER = 192.168.0.1
LEASE_TIME = 300
```

- DB初期化

```bash
$ sqlite3 dhcp.db < setup.sql
```

- 実行

```bash
$ cargo build
$ sudo ./target/debug/ch5-dhcp-server
```
