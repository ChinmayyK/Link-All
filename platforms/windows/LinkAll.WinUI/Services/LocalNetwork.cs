using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;

namespace LinkAll.WinUI
{
    internal static class LocalNetwork
    {
        // IPv4 addresses of connected, non-loopback, non-tunnel adapters, with
        // adapters that have a default gateway (the real LAN) listed first.
        // Virtual adapters (Hyper-V, WSL, VPN) usually have none, so a phone
        // given the first address reaches this PC rather than a vEthernet.
        public static List<string> IPv4Addresses()
        {
            try
            {
                return NetworkInterface.GetAllNetworkInterfaces()
                    .Where(n => n.OperationalStatus == OperationalStatus.Up
                                && n.NetworkInterfaceType != NetworkInterfaceType.Loopback
                                && n.NetworkInterfaceType != NetworkInterfaceType.Tunnel)
                    .Select(n => (Props: n.GetIPProperties(), Nic: n))
                    .OrderByDescending(x => x.Props.GatewayAddresses.Any(g => g.Address.AddressFamily == AddressFamily.InterNetwork))
                    .SelectMany(x => x.Props.UnicastAddresses)
                    .Where(a => a.Address.AddressFamily == AddressFamily.InterNetwork
                                && !IPAddress.IsLoopback(a.Address)
                                && !a.Address.ToString().StartsWith("169.254."))
                    .Select(a => a.Address.ToString())
                    .Distinct()
                    .ToList();
            }
            catch (NetworkInformationException)
            {
                return new List<string>();
            }
        }
    }
}
