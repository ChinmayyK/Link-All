use anyhow::{Context, Result};
use if_addrs::{get_if_addrs, IfAddr};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub ip: IpAddr,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub active_interface: Option<NetworkInterfaceInfo>,
    pub bind_addr: SocketAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkChangeKind {
    IpChanged,
    InterfaceChanged,
    NetworkLost,
    NetworkRestored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkChangeEvent {
    pub previous: NetworkSnapshot,
    pub current: NetworkSnapshot,
    pub kinds: Vec<NetworkChangeKind>,
}

pub fn list_interfaces() -> Result<Vec<NetworkInterfaceInfo>> {
    let primary_ip = detect_primary_outbound_ip().ok();
    let mut interfaces = Vec::new();

    for iface in get_if_addrs().context("enumerating network interfaces")? {
        let ip = match iface.addr {
            IfAddr::V4(v4) if is_candidate_v4(v4.ip) => IpAddr::V4(v4.ip),
            IfAddr::V6(v6) if is_candidate_v6(v6.ip) => IpAddr::V6(v6.ip),
            _ => continue,
        };

        interfaces.push(NetworkInterfaceInfo {
            name: iface.name,
            ip,
            is_primary: Some(ip) == primary_ip,
        });
    }

    interfaces.sort_by_cached_key(|iface| interface_rank(iface, primary_ip));

    Ok(interfaces)
}

pub fn get_active_interface() -> Result<NetworkInterfaceInfo> {
    list_interfaces()?
        .into_iter()
        .next()
        .context("no active LAN interface found")
}

pub fn get_local_ip() -> Result<IpAddr> {
    Ok(get_active_interface()?.ip)
}

pub fn bind_address(port: u16) -> Result<SocketAddr> {
    Ok(SocketAddr::new(get_local_ip()?, port))
}

pub fn detect_android_hotspot_gateway(iface: &NetworkInterfaceInfo) -> Option<IpAddr> {
    // 1. Try to ask the OS for the actual default gateway
    if let Ok(default_iface) = netdev::get_default_interface() {
        if let Some(gateway) = default_iface.gateway {
            if let Some(gw_ip) = gateway.ipv4.first() {
                return Some(std::net::IpAddr::V4(*gw_ip));
            }
        }
    }

    // 2. Fallback to guessing .1 on private subnets for hotspot/USB interfaces
    match iface.ip {
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => {
            let name = iface.name.to_lowercase();
            if is_android_hotspot_subnet(ip)
                || looks_like_wifi_or_ethernet(&name)
                || looks_like_usb_tether(&name)
            {
                let octets = ip.octets();
                Some(IpAddr::V4(Ipv4Addr::new(
                    octets[0], octets[1], octets[2], 1,
                )))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn resolve_snapshot(bind_ip: Option<IpAddr>, port: u16) -> Result<NetworkSnapshot> {
    if let Some(ip) = bind_ip {
        let active_interface = list_interfaces()
            .ok()
            .and_then(|interfaces| interfaces.into_iter().find(|iface| iface.ip == ip));
        return Ok(NetworkSnapshot {
            active_interface,
            bind_addr: SocketAddr::new(ip, port),
        });
    }

    match get_active_interface() {
        Ok(interface) => Ok(NetworkSnapshot {
            active_interface: Some(interface.clone()),
            bind_addr: SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), port),
        }),
        Err(err) => {
            warn!(error = %err, "no active LAN interface found, falling back to wildcard bind");
            Ok(NetworkSnapshot {
                active_interface: None,
                bind_addr: SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), port),
            })
        }
    }
}

pub fn detect_change(
    previous: &NetworkSnapshot,
    current: &NetworkSnapshot,
) -> Option<NetworkChangeEvent> {
    let mut kinds = Vec::new();

    let previous_name = previous
        .active_interface
        .as_ref()
        .map(|iface| iface.name.as_str());
    let current_name = current
        .active_interface
        .as_ref()
        .map(|iface| iface.name.as_str());
    if previous_name != current_name {
        kinds.push(NetworkChangeKind::InterfaceChanged);
    }

    if previous.bind_addr.ip() != current.bind_addr.ip() {
        kinds.push(NetworkChangeKind::IpChanged);
    }

    match (
        previous.active_interface.as_ref(),
        current.active_interface.as_ref(),
    ) {
        (Some(_), None) => kinds.push(NetworkChangeKind::NetworkLost),
        (None, Some(_)) => kinds.push(NetworkChangeKind::NetworkRestored),
        _ => {}
    }

    if kinds.is_empty() {
        None
    } else {
        Some(NetworkChangeEvent {
            previous: previous.clone(),
            current: current.clone(),
            kinds,
        })
    }
}

/// Watches the network and reports changes. Returns the change stream and
/// a hint sender: a host that hears about a change first (Android's
/// connectivity callback) sends on it to re-check now instead of at the
/// next poll.
pub fn spawn_network_monitor(
    bind_ip: Option<IpAddr>,
    port: u16,
    poll_interval: Duration,
) -> Result<(mpsc::Receiver<NetworkChangeEvent>, mpsc::Sender<()>)> {
    let mut previous = resolve_snapshot(bind_ip, port)?;
    let (change_tx, change_rx) = mpsc::channel(16);
    let (hint_tx, mut hint_rx) = mpsc::channel::<()>(16);

    spawn_platform_change_hints(hint_tx.clone());

    tokio::spawn(async move {
        let mut poll = tokio::time::interval(poll_interval);

        loop {
            tokio::select! {
                _ = poll.tick() => {}
                maybe_hint = hint_rx.recv() => {
                    if maybe_hint.is_none() {
                        break;
                    }
                }
            }

            match resolve_snapshot(bind_ip, port) {
                Ok(current) => {
                    if let Some(change) = detect_change(&previous, &current) {
                        previous = current;
                        if change_tx.send(change).await.is_err() {
                            break;
                        }
                    } else {
                        previous = current;
                    }
                }
                Err(err) => {
                    warn!(error = %err, "failed to refresh network snapshot");
                }
            }
        }
    });

    Ok((change_rx, hint_tx))
}

fn detect_primary_outbound_ip() -> Result<IpAddr> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .context("binding route-probe socket")?;
    socket
        .connect(SocketAddr::from(([8, 8, 8, 8], 80)))
        .context("probing primary outbound route")?;
    Ok(socket
        .local_addr()
        .context("reading probed local addr")?
        .ip())
}

fn is_candidate_v4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && (ip.is_private() || ip.is_link_local())
}

fn is_candidate_v6(ip: std::net::Ipv6Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && (ip.is_unique_local() || ip.is_unicast_link_local())
}

fn interface_rank(
    iface: &NetworkInterfaceInfo,
    primary_ip: Option<IpAddr>,
) -> (u8, u8, u8, String) {
    let name = iface.name.to_lowercase();
    let primary_rank = if Some(iface.ip) == primary_ip { 0 } else { 1 };
    let transport_rank = if looks_like_wifi_or_ethernet(&name) {
        0
    } else if looks_like_usb_tether(&name) {
        1
    } else {
        2
    };
    let ip_rank = match iface.ip {
        IpAddr::V4(v4) if v4.is_private() => 0,
        IpAddr::V6(v6) if v6.is_unique_local() => 1,
        IpAddr::V4(v4) if v4.is_link_local() => 2,
        IpAddr::V6(v6) if v6.is_unicast_link_local() => 3,
        _ => 4,
    };

    (transport_rank, ip_rank, primary_rank, name)
}

fn looks_like_wifi_or_ethernet(name: &str) -> bool {
    name.starts_with("en")
        || name.starts_with("eth")
        || name.starts_with("wl")
        || name.contains("wifi")
        || name.contains("wlan")
        || name.contains("ap")
        || name.contains("bridge")
}

fn looks_like_usb_tether(name: &str) -> bool {
    name.starts_with("usb")
        || name.contains("rndis")
        || name.contains("tether")
        || name.contains("bridge")
}

/// Whether this device is on a network other devices can share: Wi-Fi,
/// Ethernet, a hotspot it hosts or joins, USB tethering. Mobile data alone is
/// not one, and sending discovery or reconnects over it only wakes the
/// cellular radio (battery, and data on metered plans).
pub fn has_local_network() -> bool {
    list_interfaces()
        .map(|ifaces| any_local_network(&ifaces))
        .unwrap_or(true)
}

fn any_local_network(ifaces: &[NetworkInterfaceInfo]) -> bool {
    ifaces.iter().any(|iface| {
        // Every interface, even one that is down, can carry an IPv6
        // link-local address; it says nothing about a usable network.
        let routable = match iface.ip {
            IpAddr::V4(_) => true,
            IpAddr::V6(v6) => v6.is_unique_local(),
        };
        routable && !looks_like_cellular(&iface.name)
    })
}

/// Mobile-data interfaces as Android names them (Qualcomm `rmnet*`,
/// MediaTek `ccmni*`, 464XLAT `v4-rmnet*`, generic `pdp*`).
pub fn looks_like_cellular(name: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with("rmnet")
        || name.starts_with("ccmni")
        || name.starts_with("v4-")
        || name.starts_with("pdp")
}

fn is_android_hotspot_subnet(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    matches!(
        (o[0], o[1], o[2]),
        // Classic Android hotspot
        (192, 168, 43)
        // Android Wi-Fi Direct / newer hotspot
        | (192, 168, 49)
        // Some Samsung/OnePlus variants
        | (192, 168, 2) // Android 13+ may use randomized subnets in 192.168.x.x range
    )
}

/// Detect if the current local IP suggests we're connected to a mobile hotspot.
///
/// Covers:
/// - Android hotspot: 192.168.43.x, 192.168.49.x
/// - iPhone hotspot: 172.20.10.x
/// - USB tethering: 192.168.42.x (Android), various link-local
/// - Wi-Fi Direct: 192.168.49.x
pub fn is_hotspot_network(iface: &NetworkInterfaceInfo) -> bool {
    match iface.ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            let name = iface.name.to_lowercase();

            // Android hotspot subnets
            if is_android_hotspot_subnet(ip) {
                return true;
            }
            // iPhone personal hotspot: typically 172.20.10.x
            if o[0] == 172 && o[1] == 20 && o[2] == 10 {
                return true;
            }
            // Android USB tethering: typically 192.168.42.x
            if o[0] == 192 && o[1] == 168 && o[2] == 42 {
                return true;
            }
            // Interface name hints
            if looks_like_usb_tether(&name) {
                return true;
            }
            // Small subnets with very few peers (hotspots typically have < 10 clients)
            // detected by bridge/ap-style interface names
            if name.contains("bridge") && ip.is_private() {
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Detect the gateway IP for the hotspot network.
///
/// On mobile hotspot networks, the device acting as the hotspot is typically
/// the gateway (x.x.x.1). This function also tries to detect all common
/// gateway addresses for hotspot scenarios.
pub fn detect_hotspot_gateway_candidates(iface: &NetworkInterfaceInfo) -> Vec<IpAddr> {
    let mut candidates = Vec::new();

    // 1. Try the OS-reported default gateway
    if let Ok(default_iface) = netdev::get_default_interface() {
        tracing::debug!("Default interface found: {}", default_iface.name);
        for ip in default_iface.ipv4 {
            let addr = IpAddr::V4(ip.addr());
            // It could be a VPN interface or a virtual one
            if addr.is_loopback() {
                continue;
            }
            candidates.push(addr);
        }
    }

    // 2. Guess .1 on the same subnet (standard for hotspots)
    if let IpAddr::V4(ip) = iface.ip {
        if ip.is_private() || ip.is_link_local() {
            let o = ip.octets();
            let gateway_1 = IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], 1));
            if !candidates.contains(&gateway_1) && iface.ip != gateway_1 {
                candidates.push(gateway_1);
            }
        }
    }

    candidates
}

#[cfg(target_os = "linux")]
fn spawn_platform_change_hints(tx: mpsc::Sender<()>) {
    tokio::task::spawn_blocking(move || {
        if let Err(err) = linux_netlink_change_hints(tx) {
            warn!(
                error = %err,
                "linux netlink watcher stopped; continuing with polling fallback"
            );
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn spawn_platform_change_hints(_tx: mpsc::Sender<()>) {}

#[cfg(target_os = "linux")]
fn linux_netlink_change_hints(tx: mpsc::Sender<()>) -> Result<()> {
    use std::mem;

    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_ROUTE) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("opening netlink socket");
    }

    let result = (|| -> Result<()> {
        let mut addr: libc::sockaddr_nl = unsafe { mem::zeroed() };
        addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        addr.nl_groups =
            (libc::RTMGRP_LINK | libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV6_IFADDR) as u32;

        let bind_rc = unsafe {
            libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if bind_rc < 0 {
            return Err(std::io::Error::last_os_error()).context("binding netlink socket");
        }

        let timeout = libc::timeval {
            tv_sec: 1,
            tv_usec: 0,
        };
        let setopt_rc = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &timeout as *const _ as *const libc::c_void,
                mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if setopt_rc < 0 {
            return Err(std::io::Error::last_os_error()).context("setting SO_RCVTIMEO");
        }

        let mut buf = [0u8; 4096];
        loop {
            if tx.is_closed() {
                break;
            }
            let recv_len = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut _, buf.len(), 0) };
            if recv_len < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.raw_os_error() == Some(libc::EAGAIN)
                    || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                {
                    continue;
                }
                return Err(err).context("reading netlink change event");
            }
            if recv_len == 0 {
                continue;
            }
            if tx.blocking_send(()).is_err() {
                break;
            }
        }

        Ok(())
    })();

    unsafe {
        libc::close(fd);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, ip: &str) -> NetworkInterfaceInfo {
        NetworkInterfaceInfo {
            name: name.into(),
            ip: ip.parse().unwrap(),
            is_primary: false,
        }
    }

    #[test]
    fn mobile_data_alone_is_not_a_local_network() {
        assert!(!any_local_network(&[]));
        assert!(!any_local_network(&[
            iface("rmnet_data0", "10.45.2.7"),
            iface("v4-rmnet_data0", "192.0.0.4"),
            iface("wlan0", "fe80::1"),
        ]));
        assert!(any_local_network(&[
            iface("rmnet_data0", "10.45.2.7"),
            iface("wlan0", "192.168.29.21"),
        ]));
        // A hotspot this phone hosts, while it is itself on mobile data.
        assert!(any_local_network(&[
            iface("rmnet_data0", "10.45.2.7"),
            iface("ap0", "192.168.43.1"),
        ]));
    }

    #[test]
    fn candidate_ipv4_filter_rejects_loopback() {
        assert!(!is_candidate_v4(Ipv4Addr::LOCALHOST));
        assert!(is_candidate_v4(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn detects_ip_and_interface_change() {
        let previous = NetworkSnapshot {
            active_interface: Some(NetworkInterfaceInfo {
                name: "en0".into(),
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                is_primary: true,
            }),
            bind_addr: SocketAddr::from(([192, 168, 1, 10], 47823)),
        };
        let current = NetworkSnapshot {
            active_interface: Some(NetworkInterfaceInfo {
                name: "bridge0".into(),
                ip: IpAddr::V4(Ipv4Addr::new(172, 20, 10, 4)),
                is_primary: true,
            }),
            bind_addr: SocketAddr::from(([172, 20, 10, 4], 47823)),
        };

        let change = detect_change(&previous, &current).unwrap();
        assert!(change.kinds.contains(&NetworkChangeKind::InterfaceChanged));
        assert!(change.kinds.contains(&NetworkChangeKind::IpChanged));
    }

    #[test]
    fn detects_network_loss() {
        let previous = NetworkSnapshot {
            active_interface: Some(NetworkInterfaceInfo {
                name: "en0".into(),
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                is_primary: true,
            }),
            bind_addr: SocketAddr::from(([192, 168, 1, 10], 47823)),
        };
        let current = NetworkSnapshot {
            active_interface: None,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 47823)),
        };

        let change = detect_change(&previous, &current).unwrap();
        assert!(change.kinds.contains(&NetworkChangeKind::NetworkLost));
        assert!(change.kinds.contains(&NetworkChangeKind::InterfaceChanged));
        assert!(change.kinds.contains(&NetworkChangeKind::IpChanged));
    }

    #[test]
    fn detects_android_hotspot_gateway() {
        let iface = NetworkInterfaceInfo {
            name: "en7".into(),
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 43, 88)),
            is_primary: true,
        };
        // Depending on host OS, netdev might return the real default gateway.
        // We assert that it returns *some* valid gateway IP for this interface.
        assert!(detect_android_hotspot_gateway(&iface).is_some());
    }
}
