using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Threading.Tasks;
using Deskdrop.WinUI.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Deskdrop.WinUI
{
    // Direct IP connect for networks where mDNS discovery is blocked. The
    // engine only accepts IP literals, so hostnames are resolved here first.
    // A new, untrusted device then goes through the normal pairing prompt.
    public sealed partial class ConnectByIpDialog : ContentDialog
    {
        public const int DefaultPort = 47823;

        private const string RecentSettingKey = "RecentManualAddresses";
        private const int MaxRecent = 5;

        // The engine's own socket timeouts normally answer first; this bounds
        // the wait if the service never replies.
        private const int ConnectTimeoutMs = 20_000;

        private bool _connecting;

        public ConnectByIpDialog()
        {
            this.InitializeComponent();
            Title = Services.AppDialog.Header("\uE968", "Connect by IP address", "For networks where devices can't find each other");

            var ownAddresses = LocalIPv4Addresses();
            OwnAddressText.Text = ownAddresses.Count == 0
                ? "No network connection"
                : string.Join("\n", ownAddresses.Select(a => $"{a}:{DefaultPort}"));

            var recent = RecentAddresses();
            RecentList.ItemsSource = recent.Take(3).ToList();
            RecentPanel.Visibility = recent.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

            Opened += (_, _) => AddressBox.Focus(FocusState.Programmatic);
        }

        public readonly record struct ManualAddress(string Host, int Port)
        {
            public override string ToString()
            {
                var host = Host.Contains(':') ? $"[{Host}]" : Host;
                return Port == DefaultPort ? host : $"{host}:{Port}";
            }
        }

        // Parses "host", "host:port", "[v6]:port" or a bare IPv6 address.
        // Returns null and sets error when the input can't be used.
        public static ManualAddress? Parse(string raw, out string? error)
        {
            error = null;
            var input = raw.Trim();
            if (input.Length == 0) { error = "Enter the other device's IP address."; return null; }

            string host;
            string? portText = null;
            if (input.StartsWith('['))
            {
                var end = input.IndexOf(']');
                if (end < 0) { error = "Missing ] after the IPv6 address."; return null; }
                host = input[1..end];
                var rest = input[(end + 1)..];
                if (rest.Length > 0 && !rest.StartsWith(':')) { error = "Use [address]:port."; return null; }
                if (rest.Length > 1) portText = rest[1..];
            }
            else if (input.Count(ch => ch == ':') >= 2)
            {
                host = input;
            }
            else
            {
                var colon = input.IndexOf(':');
                host = colon < 0 ? input : input[..colon];
                if (colon >= 0) portText = input[(colon + 1)..];
            }

            var port = DefaultPort;
            if (portText != null && (!int.TryParse(portText, out port) || port < 1 || port > 65535))
            {
                error = "Port must be a number from 1 to 65535.";
                return null;
            }

            var looksLikeV4 = host.Length > 0 && host.All(ch => char.IsDigit(ch) || ch == '.');
            if (looksLikeV4)
            {
                var parts = host.Split('.');
                if (parts.Length != 4 || parts.Any(p => !int.TryParse(p, out var n) || n > 255))
                {
                    error = "That isn't a valid IP address. It should look like 192.168.1.50.";
                    return null;
                }
            }
            else if (!IPAddress.TryParse(host, out _) && Uri.CheckHostName(host) != UriHostNameType.Dns)
            {
                error = "That doesn't look like an IP address or hostname.";
                return null;
            }

            return new ManualAddress(host, port);
        }

        private async void OnConnectClicked(ContentDialog sender, ContentDialogButtonClickEventArgs args)
        {
            // Stay open: the result is shown in the dialog, and the dialog
            // closes itself on success.
            args.Cancel = true;
            await ConnectAsync(AddressBox.Text);
        }

        private async void OnRecentClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as Button)?.Content is string address)
            {
                AddressBox.Text = address;
                await ConnectAsync(address);
            }
        }

        private async void OnAddressKeyDown(object sender, KeyRoutedEventArgs e)
        {
            if (e.Key == Windows.System.VirtualKey.Enter)
            {
                e.Handled = true;
                await ConnectAsync(AddressBox.Text);
            }
        }

        private void OnAddressChanged(object sender, TextChangedEventArgs e)
        {
            if (_connecting) return;
            ErrorText.Visibility = Visibility.Collapsed;
            StatusPanel.Visibility = Visibility.Collapsed;
        }

        private async Task ConnectAsync(string text)
        {
            if (_connecting) return;
            if (Parse(text, out var parseError) is not ManualAddress address)
            {
                ShowError(parseError!);
                return;
            }

            SetConnecting(true, $"Connecting to {address}...");
            string? error;
            try
            {
                error = await ConnectCoreAsync(address);
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                error = "Something went wrong. Try again.";
            }
            SetConnecting(false, null);

            if (error != null)
            {
                ShowError(error);
                return;
            }

            RememberAddress(address.ToString());
            NotificationHelper.ShowToast("Connected", $"Connected to {address}");
            Hide();
        }

        // Returns null on success, or a message to show.
        private static async Task<string?> ConnectCoreAsync(ManualAddress address)
        {
            IPAddress? ip;
            if (!IPAddress.TryParse(address.Host, out ip))
            {
                try
                {
                    var resolved = await Dns.GetHostAddressesAsync(address.Host);
                    ip = resolved.FirstOrDefault(a => a.AddressFamily == AddressFamily.InterNetwork)
                         ?? resolved.FirstOrDefault();
                }
                catch (SocketException)
                {
                    ip = null;
                }
                if (ip == null) return $"Couldn't find \"{address.Host}\" on this network.";
            }

            if (LocalIPv4Addresses().Contains(ip.ToString()))
                return "That's this PC's own address. Enter the other device's IP.";

            var response = await DaemonClient.SendAsync(DaemonClient.Req("connect_manual", ("host", ip.ToString()), ("port", address.Port)),
                ConnectTimeoutMs);

            if (response == null)
                return $"No answer from {address}. Make sure Link All is open on that device.";
            var root = response.RootElement;
            if (root.TryGetProperty("status", out var status) && status.GetString() == "error")
            {
                var message = root.TryGetProperty("message", out var m) ? m.GetString() : null;
                return $"Couldn't reach {address}. Check both devices are on the same network and Link All is open on the other one."
                       + (string.IsNullOrEmpty(message) ? "" : $"\n({message})");
            }
            return null;
        }

        private void SetConnecting(bool connecting, string? status)
        {
            _connecting = connecting;
            AddressBox.IsEnabled = !connecting;
            RecentList.IsEnabled = !connecting;
            IsPrimaryButtonEnabled = !connecting;
            PrimaryButtonText = connecting ? "Connecting..." : "Connect";
            ErrorText.Visibility = Visibility.Collapsed;
            StatusPanel.Visibility = connecting ? Visibility.Visible : Visibility.Collapsed;
            ConnectingRing.IsActive = connecting;
            StatusText.Text = status ?? "";
            if (!connecting) AddressBox.Focus(FocusState.Programmatic);
        }

        private void ShowError(string message)
        {
            ErrorText.Text = message;
            ErrorText.Visibility = Visibility.Visible;
            PrimaryButtonText = "Try again";
        }

        private static List<string> RecentAddresses() =>
            (LocalSettingsStore.Get(RecentSettingKey) ?? "")
                .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .ToList();

        private static void RememberAddress(string address)
        {
            var updated = new[] { address }.Concat(RecentAddresses()).Distinct().Take(MaxRecent);
            LocalSettingsStore.Set(RecentSettingKey, string.Join("\n", updated));
        }

        // IPv4 addresses of connected, non-loopback, non-tunnel adapters, with
        // adapters that have a default gateway (the real LAN) listed first.
        private static List<string> LocalIPv4Addresses()
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
