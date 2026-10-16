# Microsoft Store listing: Link All – Phone & Computer

Paste each field into Partner Center › Store listings (English). Keep claims to
what the Store build does; see the notes at the end.

## Product name
Link All – Phone & Computer

## Short description
Continuity for every device you own, whatever it runs. Your Windows PC, Mac, Android phone and Linux machine share one clipboard, pass files and folders, and show your phone's notifications and calls on your desktop. Over your own network, with no cloud and no account.

## Description
Link All brings continuity to every operating system. The kind of handoff that usually only works inside one company's devices now works across all of yours: Windows, Mac, Android and Linux, in any mix.

Copy text or an image on your phone and paste it on your PC. Drop a file or a whole folder on the window and it's on your Mac a moment later. When your phone rings in another room, your PC shows who is calling. Its notifications appear on your desktop, its battery and free space are a glance away, and the folders your other computers share are one click from your desk.

Everything travels straight over your Wi-Fi, LAN or phone hotspot, encrypted end to end. There is no cloud service, no account and no tracking. Your files and clipboard never leave the devices you paired.

ONE CLIPBOARD
• Copy on any device, paste on any other: text, links and images.
• A searchable history of everything you shared, pinned items included.
• Passwords, one-time codes, card numbers and API keys stay on the device they were copied on.

FILES AND FOLDERS
• Send files or whole folders, which arrive with their structure intact.
• Live speed and time left, and transfers resume where they stopped.
• Received files land in Downloads › Link All.

YOUR PHONE ON YOUR PC
• Phone notifications appear as Windows notifications.
• Incoming calls show who is calling in a banner.
• Battery level and free space for each phone.

YOUR OTHER COMPUTERS
• Browse, search and preview the files a paired computer shares, and fetch only what you need. Each computer chooses whether to allow it, and it is off until you turn it on.

MADE FOR WINDOWS
• Waits quietly in the system tray and can start with Windows.
• Quick Access panel to search your history and send to a device in two clicks.
• Light and dark themes that follow Windows.

PRIVATE BY DESIGN
• Devices pair once by confirming the same PIN on both screens, or by scanning a QR code.
• Every connection is encrypted end to end. Nothing goes through a server.
• Open source. The code is at github.com/ChinmayyK/Link-All.

Get Link All for your Mac, Android phone and Linux machines from linkall.chinmaykudalkar.com.

## Product features (one per line, up to 20)
Shared clipboard across your phone and computers: text, links and images
Searchable clipboard history with pinning
Sensitive text (passwords, one-time codes, cards, API keys) is never sent
Send files and whole folders, with live speed and resume after a dropped connection
Phone notifications as Windows notifications
See who is calling when your phone rings
Phone battery and free space at a glance
Browse and fetch files from your other computers, only when each one allows it
Continuity across Windows, Mac, Android and Linux, in any mix
Direct over Wi-Fi, LAN or a phone hotspot: no cloud, no account
End-to-end encrypted, paired with a PIN or QR code
System tray, start with Windows, and a Quick Access panel
Light and dark themes
Free and open source

## Search terms (7 max, 30 characters each)
continuity
universal clipboard
phone link
send files to phone
android pc
airdrop for windows
cross device

## What's new in this version
First release on the Microsoft Store.

## Category
Productivity (subcategory: none). Utilities & tools also fits.

## Privacy policy URL
https://linkall.chinmaykudalkar.com/privacy

## Website
https://linkall.chinmaykudalkar.com

## Support contact
https://linkall.chinmaykudalkar.com/support

## Screenshots
Upload docs/store/microsoft/*.png (1920×1080, in file-name order). Each has its caption built in.

## Notes for certification (Submission options › Notes for certification)
Link All needs a second device to show its main features: install Link All on an Android phone, Mac or another PC on the same network, then pair them by confirming the PIN shown on both. The app requests runFullTrust (a desktop app with a system tray helper). It needs no account and has no in-app purchases.

## Restricted capabilities justification
runFullTrust: Link All is a Win32/WinUI desktop app with a separate system tray helper process and named-pipe IPC.
(unvirtualizedResources was requested for the first submission and denied on 16 Oct 2026, policy 10.6.3. The Store build no longer asks for it: its data lives in the package's virtualized copy.)

## Claims to keep out (the Store build doesn't do these)
• "Send with Link All" in File Explorer's right-click menu: the Store build can't register it (the installer build does).
• Answering or declining calls from the PC, and SMS code forwarding: need the Android app from the website, not the Google Play one.
• Browsing a phone's files from the PC: needs the Android app from the website.
