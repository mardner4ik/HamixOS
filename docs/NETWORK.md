# Networking

HamixOS has a real TCP/IP stack in the kernel. It is built on
[smoltcp](https://github.com/smoltcp-rs/smoltcp) 0.12, vendored unchanged in
`libs/smoltcp` (0BSD licence) and compiled into the kernel with only the parts
that are needed: Ethernet medium, IPv4, ARP, DHCPv4, DNS, ICMP, UDP and TCP.

```
kernel/src/net/
  mod.rs          adapters, Ethernet / Wi-Fi / off mode, status text, hxinit units
  stack.rs        smoltcp interface, DHCP, DNS, sockets owned by processes
  dma.rs          DMA-able memory (contiguous frames below 4 GiB) and MAC helpers
  e1000.rs        Intel 8254x / 82574 (QEMU e1000 and e1000e)
  yukon.rs        Marvell Yukon 88E8040 (sky2 family) Fast Ethernet
  ar9285.rs       Qualcomm Atheros AR9285 802.11n (b/g) Wi-Fi
  ar9285_ini.rs   register initialisation tables from the Atheros HAL (ISC)
  wifi/           glue to libs/wlan
libs/wlan/        802.11 station: scanning, authentication, association,
                  WPA2-PSK (PBKDF2, PRF, 4-way handshake, AES key wrap, CCMP)
```

## How it runs

* `hxinit` probes PCI class 0x02 devices (`netdev` unit) and starts the stack
  (`netstack` unit). Ethernet is preferred when it exists, otherwise Wi-Fi.
* The `netd` kernel thread polls the active adapter and smoltcp: every tick
  while traffic flows, every third tick when idle.
* DHCP starts automatically; a static address can be set with `netconf` or in
  Settings → Network.
* Sockets belong to the process that opened them and are closed when it exits.

## Commands

| Command | What it does |
|---------|--------------|
| `ip` / `ifconfig` | adapters, MAC, link, address, gateway, DNS, packet counters |
| `netmode [ethernet \| wifi \| off]` | show or switch the active connection |
| `netconf dhcp` / `netconf static 192.168.1.20/24 192.168.1.1 1.1.1.1` | IP configuration (root) |
| `ping [-c N] host` | ICMP echo |
| `nslookup host` | DNS lookup |
| `wifi scan`, `wifi connect SSID [PASSWORD]`, `wifi disconnect` | Wi-Fi |
| `wget [-O file] http://host/path` | HTTP download |

Linux programs (curl, wget, python, git...) use the same stack through real
`AF_INET` TCP/UDP sockets, see `docs/LINUXULATOR.md`.

The Nook top bar shows the connection state (Ethernet, Wi-Fi signal strength or
offline). Clicking it opens a menu with the adapters, the Wi-Fi networks and a
switch between Ethernet and Wi-Fi.

## System calls

| Number | Name | `hamix_std::net` |
|--------|------|------------------|
| 9130 | net status (text) | `status()` |
| 9131 | set mode | `set_mode()` |
| 9132 | Wi-Fi scan results | `wifi_scan()` |
| 9133 / 9134 | Wi-Fi connect / disconnect | `wifi_connect()`, `wifi_disconnect()` |
| 9135 | configure DHCP / static | `configure()` |
| 9140–9144 | socket, connect, send, recv, close | `TcpStream` |
| 9145 | resolve | `resolve()` |
| 9146 | ping | `ping()` |
| 9147, 9148 | listen, socket state | |
| 9149, 9150 | sendto, recvfrom | `UdpSocket` |

## Testing in QEMU

```
qemu-system-x86_64 -M q35 -m 1G -smp 4 -enable-kvm -cdrom hamix_os.iso \
    -device e1000,netdev=n0 -netdev user,id=n0
```

Then `ip`, `ping 10.0.2.2`, `nslookup example.com`, `wget http://example.com/`.

## Hardware status

| Adapter | Status |
|---------|--------|
| Intel e1000 / 82574L (e1000e) | tested in QEMU |
| Marvell Yukon 88E8040 | written from the Marvell documentation and the FreeBSD `msk` driver as a reference; not yet tested on hardware |
| Atheros AR9285 | reset, PLL, EEPROM, initialisation tables, channel tuning, TX power, initial calibration, RX/TX DMA rings, scanning and the WPA2-PSK station; not yet tested on hardware |

### DHCP never finishing on real hardware

Three bugs kept the Samsung R428 (Yukon 88E8040 + AR9285) at "obtaining an
address" forever:

* smoltcp sent every DHCPREQUEST with a new transaction id instead of the
  one from the DHCPOFFER (RFC 2131 table 5). QEMU's DHCP server does not
  care, many home routers silently drop such requests. The vendored copy
  now keeps the offer's `xid`, and the stack waits 20 s instead of 6 s
  before restarting discovery so retransmitted requests are not cut off.
* Yukon: after the first received frame the driver moved the RX put index
  backwards, so the chip saw an empty ring and stopped receiving. It now
  keeps a separate put index and refills the slot behind the chip.
* AR9285: the hardware key cache was never cleared and hardware
  encryption/decryption was left on, so group-addressed (broadcast) frames
  could be flagged as decrypt errors and dropped. The driver clears all
  128 key cache entries, sets `AR_DIAG_ENCRYPT_DIS | AR_DIAG_DECRYPT_DIS`
  and leaves CCMP to `libs/wlan`.

Every DHCP message is now written to the kernel log (`dmesg`, serial):
`net: DHCP DISCOVER sent`, `net: DHCP OFFER received from ...`, and so on.
If a machine still does not get an address, that log shows which step is
missing.

The 802.11 and WPA2 logic in `libs/wlan` is tested on the host with the
standard test vectors (SHA-1, HMAC, PBKDF2, PRF, AES, key wrap, CCMP) and with
a simulated access point: `cargo test -p wlan`.

## Licences

smoltcp is 0BSD. The AR9285 initialisation values come from the Atheros HAL
under the ISC licence, see `docs/licenses/atheros-hal-ISC.txt`. The FreeBSD
`msk` driver was only used as a reference, see `docs/licenses/marvell-msk-BSD.txt`.
