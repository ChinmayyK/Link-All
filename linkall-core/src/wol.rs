//! Wake-on-LAN: waking a computer that has gone to sleep.
//!
//! A sleeping computer is not connected to Link All, so the wake does not go
//! through the app's connection. The computer tells its paired devices its
//! network card's MAC address while it is awake (in the handshake). To wake it,
//! a device broadcasts a "magic packet": six 0xFF bytes and then that MAC
//! address sixteen times, as a UDP datagram. The network card, which stays
//! powered while the computer sleeps, recognises its own address in the packet
//! and tells the computer to power up. Nothing here can tell whether it worked;
//! the caller watches for the computer to connect again.
//!
//! Whether a card listens while asleep is a setting on that computer (and often
//! in its BIOS); most do over a cable and few do over Wi-Fi.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// The two ports magic packets are conventionally sent to.
const PORTS: [u16; 2] = [9, 7];

/// A MAC address as six bytes.
pub type Mac = [u8; 6];

/// Reads `aa:bb:cc:dd:ee:ff`, `aa-bb-cc-dd-ee-ff` or `aabbccddeeff`.
/// None for anything else, and for the all-zero and multicast addresses,
/// which are never a real network card.
pub fn parse_mac(text: &str) -> Option<Mac> {
    let hex: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ':' | '-' | '.'))
        .collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    usable(mac).then_some(mac)
}

/// Whether `mac` could be a real network card's address.
fn usable(mac: Mac) -> bool {
    mac != [0; 6] && mac != [0xFF; 6] && mac[0] & 0x01 == 0
}

/// `aa:bb:cc:dd:ee:ff`, lower case.
pub fn format_mac(mac: Mac) -> String {
    mac.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The magic packet for `mac`: 6 x 0xFF, then the address 16 times.
pub fn magic_packet(mac: Mac) -> [u8; 102] {
    let mut packet = [0xFF; 102];
    for i in 0..16 {
        packet[6 + i * 6..12 + i * 6].copy_from_slice(&mac);
    }
    packet
}

/// This computer's own MAC address, as a string, for telling paired devices.
/// The address of the network card the computer is using on the LAN; None
/// when it cannot be told (a phone, a virtual adapter, no network).
pub fn local_mac() -> Option<String> {
    let active = crate::network_manager::get_active_interface().ok()?;
    let IpAddr::V4(ip) = active.ip else {
        return None;
    };
    netdev::get_interfaces()
        .into_iter()
        .find(|iface| iface.ipv4.iter().any(|net| net.addr() == ip))
        .and_then(|iface| iface.mac_addr)
        .map(|m| m.octets())
        .filter(|m| usable(*m))
        .map(format_mac)
}

/// Where a magic packet should be sent from and to: for each IPv4 network this
/// computer is on, that network's broadcast address, sent from that network's
/// own address so it leaves by the right card. The limited broadcast address is
/// added for each, which routers that drop directed broadcasts still pass on
/// the local segment.
fn routes() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut out = Vec::new();
    for iface in netdev::get_interfaces() {
        if !iface.is_up() || iface.is_loopback() {
            continue;
        }
        for net in &iface.ipv4 {
            let ip = net.addr();
            if ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() {
                continue;
            }
            for target in [net.broadcast(), Ipv4Addr::BROADCAST] {
                if !out.contains(&(ip, target)) {
                    out.push((ip, target));
                }
            }
        }
    }
    out
}

/// Broadcasts the magic packet for `mac` on every local network. Returns how
/// many datagrams went out. An error means none could be sent at all.
pub fn send_magic_packet(mac: Mac) -> Result<usize, String> {
    let packet = magic_packet(mac);
    let mut sent = 0;
    let mut last_error = None;
    for (from, to) in routes() {
        let socket = match UdpSocket::bind(SocketAddr::new(IpAddr::V4(from), 0)) {
            Ok(s) => s,
            Err(e) => {
                last_error = Some(e.to_string());
                continue;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            last_error = Some(e.to_string());
            continue;
        }
        for port in PORTS {
            match socket.send_to(&packet, SocketAddr::new(IpAddr::V4(to), port)) {
                Ok(_) => sent += 1,
                Err(e) => last_error = Some(e.to_string()),
            }
        }
    }
    if sent == 0 {
        Err(last_error.unwrap_or_else(|| "this device is not on a network".to_string()))
    } else {
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packet_is_six_ff_then_the_address_sixteen_times() {
        let mac = [0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E];
        let p = magic_packet(mac);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        for i in 0..16 {
            assert_eq!(&p[6 + i * 6..12 + i * 6], &mac, "repeat {i}");
        }
    }

    #[test]
    fn addresses_are_read_in_the_usual_spellings() {
        let want = Some([0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]);
        assert_eq!(parse_mac("00:1a:2b:3c:4d:5e"), want);
        assert_eq!(parse_mac("00-1A-2B-3C-4D-5E"), want);
        assert_eq!(parse_mac("001a2b3c4d5e"), want);
        assert_eq!(parse_mac("  00:1A:2B:3C:4D:5E "), want);
        assert_eq!(parse_mac("001a.2b3c.4d5e"), want);
    }

    #[test]
    fn things_that_are_not_a_card_are_refused() {
        assert_eq!(parse_mac(""), None);
        assert_eq!(parse_mac("00:1a:2b:3c:4d"), None, "too short");
        assert_eq!(parse_mac("00:1a:2b:3c:4d:5e:6f"), None, "too long");
        assert_eq!(parse_mac("zz:1a:2b:3c:4d:5e"), None, "not hex");
        assert_eq!(parse_mac("00:00:00:00:00:00"), None, "all zero");
        assert_eq!(parse_mac("ff:ff:ff:ff:ff:ff"), None, "broadcast");
        assert_eq!(parse_mac("01:00:5e:00:00:01"), None, "multicast");
    }

    #[test]
    fn an_address_survives_formatting() {
        let mac = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01];
        assert_eq!(format_mac(mac), "de:ad:be:ef:00:01");
        assert_eq!(parse_mac(&format_mac(mac)), Some(mac));
    }

    #[test]
    fn the_packet_reaches_a_listener_on_this_computer() {
        // A socket standing in for the sleeping card, on a spare port; this
        // checks the bytes that go on the wire, not the broadcast routing.
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to(&magic_packet(mac), listener.local_addr().unwrap())
            .unwrap();
        let mut buf = [0u8; 128];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &magic_packet(mac)[..]);
    }
}
