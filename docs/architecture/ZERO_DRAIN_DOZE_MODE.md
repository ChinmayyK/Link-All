# Beating Android Doze Mode: The Zero-Drain Architecture

Maintaining a persistent TCP connection between an Android phone and a Mac usually requires keeping the Android CPU awake. Android's deep sleep ("Doze mode") halts the CPU to save battery, which normally breaks continuous socket communication and leads to timeouts. 

Historically, developers fought this by acquiring a `PARTIAL_WAKE_LOCK` to prevent the CPU from sleeping. While this keeps the connection open, it consumes roughly 5% battery per hour—an unacceptable trade-off for a background utility.

Here is how Link All redesigned its network engine to beat Doze mode and achieve **0% battery drain**.

## 1. Asymmetric Connection Management
Instead of forcing both devices to constantly ping each other every 15 seconds, we shifted the responsibility of maintaining the connection entirely to the Mac (which isn't battery-constrained).

We introduced a new protocol message: `AppMessage::DeviceSleepState`. 
- When the Android screen turns off (`ACTION_SCREEN_OFF`), the phone broadcasts `DeviceSleepState { is_asleep: true }` to the Mac.
- The phone's CPU is then allowed to fully halt and enter deep sleep.

## 2. The Mac's Role: Infinite Patience
When the Mac receives the sleep state, it alters its network rules for that specific phone:
1. **Relaxed Timeout:** The Mac stops expecting a heartbeat every 15 seconds. It extends the timeout to **24 hours**.
2. **NAT Keepalive:** If a socket sits entirely idle, intermediate Wi-Fi routers will silently kill it after about 15 minutes. To prevent this, the Mac sends a sparse ping to the Android device **exactly once every 5 minutes**. 

> [!TIP]
> **Why 5 minutes?** It's frequent enough to keep the router's NAT routing tables alive, but sparse enough that it doesn't wake the Android radio frequently.

## 3. The Android Kernel: Passive Sockets
When the Mac sends its 5-minute ping, the packet travels over the Wi-Fi network to the sleeping phone. 

Because we instruct the user to "Disable Battery Optimizations" for Link All, Android's firewall does not drop the packet. The phone's Wi-Fi chip receives the packet and briefly wakes the kernel's network stack just long enough to send a TCP ACK (acknowledgment) back to the Mac. **The main Android CPU and the Link All app do not need to wake up for this to happen.** The socket remains perfectly alive at the OS level.

## 4. The "Time Jump" Bug Fix
When the Android phone eventually wakes up—either because the user turned the screen on (`ACTION_SCREEN_ON`) or Android triggered a brief Doze maintenance window (`ACTION_DEVICE_IDLE_MODE_CHANGED`)—a critical issue occurs:

The local Rust `linkall-core` process resumes execution and looks at its clock. It realizes 2 hours have passed since it last saw a heartbeat from the Mac. Normally, the Rust engine would panic and instantly sever the connection because `2 hours > 15 seconds`.

To beat this, we introduced the `local_last_wake` timestamp in the Rust engine:
- Whenever the Android device wakes up or network access is restored, we update `local_last_wake` to the current time.
- The heartbeat loop checks `time_since_wake`. If the CPU *just* woke up, it grants the connection a **15-second grace period** before enforcing any timeouts.

## Conclusion
By combining an **asymmetric heartbeat protocol** with **smart local time-jump detection**, Link All allows the Android OS to fully suspend the CPU while keeping the actual TCP socket open. The connection remains instantly responsive the moment the phone screen turns on, at zero cost to the phone's battery.
