//! Walk-away lock: lock this computer when the paired phone that was linked
//! to it goes away and stays away. It is off until the user turns it on.
//!
//! A phone that leaves Wi-Fi range and one whose connection merely dropped
//! look the same from here, so the lock waits out a grace period first and
//! only counts a phone that had been connected: a computer that never saw a
//! phone does not lock itself.

use super::*;

const CHECK_EVERY: Duration = Duration::from_secs(2);
/// How long the phone must be gone before the computer locks.
const GRACE: Duration = Duration::from_secs(30);

/// What the watcher should do after looking at the phones, as a pure step so
/// it can be tested without a clock.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    Nothing,
    Lock,
}

#[derive(Default)]
pub(super) struct Watch {
    had_phone: bool,
    gone_for: Duration,
}

impl Watch {
    /// `phone_here`: a paired phone is connected now. `network_up`: this
    /// computer is on a network, so a missing phone isn't just our own
    /// connection being down. `elapsed`: time since the last step.
    pub(super) fn step(&mut self, phone_here: bool, network_up: bool, elapsed: Duration) -> Step {
        if phone_here {
            self.had_phone = true;
            self.gone_for = Duration::ZERO;
            return Step::Nothing;
        }
        if !self.had_phone || !network_up {
            self.gone_for = Duration::ZERO;
            return Step::Nothing;
        }
        self.gone_for += elapsed;
        if self.gone_for >= GRACE {
            // Lock once; it arms again when the phone comes back.
            self.had_phone = false;
            self.gone_for = Duration::ZERO;
            return Step::Lock;
        }
        Step::Nothing
    }

    /// The setting was turned off: forget what was seen.
    pub(super) fn reset(&mut self) {
        *self = Watch::default();
    }
}

impl Engine {
    pub(super) fn spawn_walk_away_lock(&self) {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return;
        }
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let mut watch = Watch::default();
            let mut interval = tokio::time::interval(CHECK_EVERY);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if !shared.settings.lock().unwrap().lock_when_phone_leaves {
                    watch.reset();
                    continue;
                }
                let phone_here = shared.peer_manager.list().iter().any(|p| {
                    p.trusted
                        && p.status == PeerConnectionState::Connected
                        && p.platform.as_deref() == Some("Android")
                });
                let step = watch.step(
                    phone_here,
                    network_manager::has_local_network(),
                    CHECK_EVERY,
                );
                if step == Step::Lock {
                    tracing::info!("the paired phone has been gone, locking the screen");
                    let _ = tokio::task::spawn_blocking(|| {
                        crate::remote_control::perform(crate::remote_control::Action::LockScreen)
                    })
                    .await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_secs(2);

    fn run(watch: &mut Watch, phone: bool, net: bool, ticks: u32) -> Vec<Step> {
        (0..ticks).map(|_| watch.step(phone, net, TICK)).collect()
    }

    #[test]
    fn locks_once_after_a_phone_that_was_here_stays_away() {
        let mut w = Watch::default();
        run(&mut w, true, true, 3);
        let steps = run(&mut w, false, true, 20);
        assert_eq!(steps.iter().filter(|s| **s == Step::Lock).count(), 1);
        // Not before the grace period.
        assert!(steps[..14].iter().all(|s| *s == Step::Nothing));
    }

    #[test]
    fn a_computer_that_never_saw_a_phone_does_not_lock() {
        let mut w = Watch::default();
        assert!(run(&mut w, false, true, 60)
            .iter()
            .all(|s| *s == Step::Nothing));
    }

    #[test]
    fn a_short_absence_does_not_lock() {
        let mut w = Watch::default();
        run(&mut w, true, true, 1);
        run(&mut w, false, true, 5);
        assert!(run(&mut w, true, true, 40)
            .iter()
            .all(|s| *s == Step::Nothing));
    }

    #[test]
    fn no_network_here_is_not_the_phone_leaving() {
        let mut w = Watch::default();
        run(&mut w, true, true, 1);
        assert!(run(&mut w, false, false, 60)
            .iter()
            .all(|s| *s == Step::Nothing));
    }

    #[test]
    fn it_arms_again_when_the_phone_returns() {
        let mut w = Watch::default();
        run(&mut w, true, true, 1);
        run(&mut w, false, true, 20);
        run(&mut w, true, true, 1);
        let steps = run(&mut w, false, true, 20);
        assert_eq!(steps.iter().filter(|s| **s == Step::Lock).count(), 1);
    }
}
