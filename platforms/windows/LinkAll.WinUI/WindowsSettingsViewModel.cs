using Microsoft.UI.Dispatching;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace LinkAll.WinUI
{
    public class WindowsSettingsViewModel : BaseViewModel
    {
        private bool _syncEnabled = true;
        public bool SyncEnabled
        {
            get => _syncEnabled;
            set => SetProperty(ref _syncEnabled, value);
        }

        private bool _showNotifications = true;
        public bool ShowNotifications
        {
            get => _showNotifications;
            set => SetProperty(ref _showNotifications, value);
        }

        private bool _requireTofu = true;
        public bool RequireTofu
        {
            get => _requireTofu;
            set => SetProperty(ref _requireTofu, value);
        }

        public bool OtpShieldEnabled
        {
            get => LinkAllStore.Shared.OtpShieldEnabled;
            set { LinkAllStore.Shared.OtpShieldEnabled = value; OnPropertyChanged(); }
        }

        // Add other core daemon settings here as needed
        // e.g., Device Name, Max Payload Size, etc.
        
        private string _deviceName = "";
        public string DeviceName
        {
            get => _deviceName;
            set => SetProperty(ref _deviceName, value);
        }
    }
}









