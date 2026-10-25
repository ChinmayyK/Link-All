//! Shortcuts that do the same thing but are pressed differently on a Mac and
//! on a PC, for a hand-off between the two.
//!
//! Keys travel as PS/2 scan codes of a PC keyboard, with the Mac's Command on
//! Control and its Control on the Windows key (see `scancode_to_mac`), so
//! Cmd+C on a Mac keyboard is Ctrl+C on a PC and the reverse. That covers
//! copy, paste, undo and most others. What it can't cover is a shortcut the
//! two systems spell differently: Cmd+Tab switches apps on a Mac, Alt+Tab on
//! a PC; Option+Left jumps a word on a Mac, Ctrl+Left on a PC; Home goes to
//! the start of the line on a PC, Cmd+Left (or Ctrl+A) on a Mac. The computer
//! being driven rewrites those here, so each one does what the person pressing
//! it expects.
//!
//! The rewrite only changes what the computer being driven is told is held.
//! The keys the other computer holds are tracked as they arrive, and every
//! release lets go of exactly what its press put down.

/// Which system a computer runs, as far as its shortcuts go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Mac,
    Windows,
    Linux,
}

impl System {
    /// From the platform a peer reported (`network::MY_PLATFORM`).
    pub fn from_platform(platform: &str) -> Option<System> {
        match platform {
            "macOS" => Some(System::Mac),
            "Windows" => Some(System::Windows),
            "Linux" => Some(System::Linux),
            _ => None,
        }
    }

    /// The system this build runs on.
    pub fn this() -> System {
        if cfg!(target_os = "macos") {
            System::Mac
        } else if cfg!(windows) {
            System::Windows
        } else {
            System::Linux
        }
    }

    fn is_mac(self) -> bool {
        self == System::Mac
    }
}

/// What the computer being driven should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Key(u16, bool),
    /// Lock the screen (Windows won't take Win+L as input).
    Lock,
}

// Modifiers, as bits. On the wire Control stands for the Mac's Command and the
// Windows key for the Mac's Control.
const CTRL: u8 = 1;
const SHIFT: u8 = 2;
const ALT: u8 = 4;
const WIN: u8 = 8;

const L_CTRL: u16 = 0x1D;
const R_CTRL: u16 = 0xE01D;
const L_SHIFT: u16 = 0x2A;
const R_SHIFT: u16 = 0x36;
const L_ALT: u16 = 0x38;
const R_ALT: u16 = 0xE038;
const L_WIN: u16 = 0xE05B;
const R_WIN: u16 = 0xE05C;

const ESC: u16 = 0x01;
const K3: u16 = 0x04;
const K4: u16 = 0x05;
const K5: u16 = 0x06;
const BACKSPACE: u16 = 0x0E;
const TAB: u16 = 0x0F;
const Q: u16 = 0x10;
const E: u16 = 0x12;
const Y: u16 = 0x15;
const L_BRACKET: u16 = 0x1A;
const R_BRACKET: u16 = 0x1B;
const A: u16 = 0x1E;
const S: u16 = 0x1F;
const D: u16 = 0x20;
const F: u16 = 0x21;
const L: u16 = 0x26;
const Z: u16 = 0x2C;
const M: u16 = 0x32;
const PERIOD: u16 = 0x34;
const SPACE: u16 = 0x39;
const F4: u16 = 0x3E;
const F11: u16 = 0x57;
const PRINT_SCREEN: u16 = 0xE037;
const HOME: u16 = 0xE047;
const UP: u16 = 0xE048;
const LEFT: u16 = 0xE04B;
const RIGHT: u16 = 0xE04D;
const END: u16 = 0xE04F;
const DOWN: u16 = 0xE050;
const DELETE: u16 = 0xE053;

/// The modifier bit a scan code is, if it is a modifier.
fn modifier(scancode: u16) -> Option<u8> {
    match scancode {
        L_CTRL | R_CTRL => Some(CTRL),
        L_SHIFT | R_SHIFT => Some(SHIFT),
        L_ALT | R_ALT => Some(ALT),
        L_WIN | R_WIN => Some(WIN),
        _ => None,
    }
}

fn left_key(bit: u8) -> u16 {
    match bit {
        CTRL => L_CTRL,
        SHIFT => L_SHIFT,
        ALT => L_ALT,
        _ => L_WIN,
    }
}

enum Out {
    /// Hold these modifiers and this key while the key is held.
    Chord(u8, u16),
    /// Press each of these in turn, once.
    Taps(&'static [(u8, u16)]),
    Lock,
}

struct Rule {
    mods: u8,
    key: u16,
    /// Shift held with the shortcut stays held with its rewrite
    /// (Shift+Cmd+Left selects to the start of the line, as Shift+Home).
    shift_passes: bool,
    out: Out,
    /// The rewritten modifiers stay down until the original ones are let go,
    /// so Cmd+Tab, Tab, Tab walks the app switcher as Alt+Tab, Tab, Tab does.
    sticky: bool,
    /// Only when the computer being driven runs this system.
    only: Option<System>,
}

const fn rule(mods: u8, key: u16, out: Out) -> Rule {
    Rule {
        mods,
        key,
        shift_passes: false,
        out,
        sticky: false,
        only: None,
    }
}

impl Rule {
    const fn shift(mut self) -> Rule {
        self.shift_passes = true;
        self
    }
    const fn sticky(mut self) -> Rule {
        self.sticky = true;
        self
    }
    const fn on(mut self, system: System) -> Rule {
        self.only = Some(system);
        self
    }
}

/// A Mac keyboard driving a PC.
const MAC_ON_PC: &[Rule] = &[
    rule(CTRL, TAB, Out::Chord(ALT, TAB)).shift().sticky(),
    rule(CTRL, Q, Out::Chord(ALT, F4)),
    rule(CTRL, LEFT, Out::Chord(0, HOME)).shift(),
    rule(CTRL, RIGHT, Out::Chord(0, END)).shift(),
    rule(CTRL, UP, Out::Chord(CTRL, HOME)).shift(),
    rule(CTRL, DOWN, Out::Chord(CTRL, END)).shift(),
    rule(ALT, LEFT, Out::Chord(CTRL, LEFT)).shift(),
    rule(ALT, RIGHT, Out::Chord(CTRL, RIGHT)).shift(),
    rule(ALT, BACKSPACE, Out::Chord(CTRL, BACKSPACE)),
    rule(ALT, DELETE, Out::Chord(CTRL, DELETE)),
    rule(CTRL, BACKSPACE, Out::Taps(&[(SHIFT, HOME), (0, BACKSPACE)])),
    // Spotlight: the Start menu, or the Activities overview.
    rule(CTRL, SPACE, Out::Chord(0, L_WIN)),
    rule(CTRL | WIN, F, Out::Chord(0, F11)),
    rule(CTRL | WIN, Q, Out::Lock).on(System::Windows),
    rule(CTRL | WIN, Q, Out::Chord(WIN, L)).on(System::Linux),
    rule(CTRL | SHIFT, K3, Out::Chord(WIN, PRINT_SCREEN)).on(System::Windows),
    rule(CTRL | SHIFT, K4, Out::Chord(WIN | SHIFT, S)).on(System::Windows),
    rule(CTRL | SHIFT, K5, Out::Chord(WIN | SHIFT, S)).on(System::Windows),
    rule(CTRL, M, Out::Chord(WIN, DOWN)).on(System::Windows),
    rule(CTRL | ALT, ESC, Out::Chord(CTRL | SHIFT, ESC)).on(System::Windows),
    // Control+arrows move between Spaces and open Mission Control.
    rule(WIN, LEFT, Out::Chord(WIN | CTRL, LEFT)).on(System::Windows),
    rule(WIN, RIGHT, Out::Chord(WIN | CTRL, RIGHT)).on(System::Windows),
    rule(WIN, UP, Out::Chord(WIN, TAB)).on(System::Windows),
];

/// A PC keyboard driving a Mac. On the Mac, CTRL is Command and WIN Control.
const PC_ON_MAC: &[Rule] = &[
    rule(ALT, TAB, Out::Chord(CTRL, TAB)).shift().sticky(),
    rule(ALT, F4, Out::Chord(CTRL, Q)),
    rule(CTRL, LEFT, Out::Chord(ALT, LEFT)).shift(),
    rule(CTRL, RIGHT, Out::Chord(ALT, RIGHT)).shift(),
    rule(CTRL, BACKSPACE, Out::Chord(ALT, BACKSPACE)),
    rule(CTRL, DELETE, Out::Chord(ALT, DELETE)),
    // Control+A and Control+E move to the ends of the line in every Mac text
    // field and do nothing elsewhere; Cmd+Left would go back in a browser.
    rule(0, HOME, Out::Chord(WIN, A)),
    rule(0, END, Out::Chord(WIN, E)),
    rule(SHIFT, HOME, Out::Chord(CTRL | SHIFT, LEFT)),
    rule(SHIFT, END, Out::Chord(CTRL | SHIFT, RIGHT)),
    rule(CTRL, HOME, Out::Chord(CTRL, UP)).shift(),
    rule(CTRL, END, Out::Chord(CTRL, DOWN)).shift(),
    rule(ALT, LEFT, Out::Chord(CTRL, L_BRACKET)),
    rule(ALT, RIGHT, Out::Chord(CTRL, R_BRACKET)),
    rule(CTRL, Y, Out::Chord(CTRL | SHIFT, Z)),
    rule(CTRL | SHIFT, ESC, Out::Chord(CTRL | ALT, ESC)),
    rule(WIN | SHIFT, S, Out::Chord(CTRL | WIN | SHIFT, K4)),
    rule(0, PRINT_SCREEN, Out::Chord(CTRL | SHIFT, K5)),
    rule(WIN, PRINT_SCREEN, Out::Chord(CTRL | SHIFT, K3)),
    rule(WIN, D, Out::Chord(0, F11)),
    rule(WIN, TAB, Out::Chord(WIN, UP)),
    rule(WIN | CTRL, LEFT, Out::Chord(WIN, LEFT)),
    rule(WIN | CTRL, RIGHT, Out::Chord(WIN, RIGHT)),
    rule(WIN, DOWN, Out::Chord(CTRL, M)),
    rule(WIN, PERIOD, Out::Chord(CTRL | WIN, SPACE)),
];

/// The Windows key pressed and let go on its own opens Spotlight on a Mac, as
/// it opens Start on a PC.
const PC_LONE_WIN_ON_MAC: (u8, u16) = (CTRL, SPACE);

/// Rewrites one computer's keys for another, one press or release at a time.
pub struct Translator {
    rules: &'static [Rule],
    target: System,
    /// The keys the other computer holds, as it sent them.
    host_down: Vec<u16>,
    /// The modifiers held down here.
    sent_mods: Vec<u16>,
    /// What each held key put down here (0 for nothing).
    active: Vec<(u16, u16)>,
    /// A sticky shortcut's modifiers (asked, given) while they stay swapped.
    sticky: Option<(u8, u8)>,
    /// A modifier pressed with nothing else since.
    lone: Option<u16>,
    /// Modifiers held here with no key pressed since they went down.
    clean: u8,
    /// Undo the Mac's Command/Control swap: the keys come from a PC keyboard
    /// on a Mac, whose Ctrl macOS reports as Control and Windows key as Command.
    as_labelled: bool,
}

impl Translator {
    /// Keys from a `host` keyboard for a computer that runs `target`. Between
    /// two of a kind (or an unknown host) nothing is rewritten.
    pub fn new(host: Option<System>, target: System) -> Translator {
        let rules: &'static [Rule] = match (host, target.is_mac()) {
            (Some(System::Mac), false) => MAC_ON_PC,
            (Some(System::Windows | System::Linux), true) => PC_ON_MAC,
            _ => &[],
        };
        Translator {
            rules,
            target,
            host_down: Vec::new(),
            sent_mods: Vec::new(),
            active: Vec::new(),
            sticky: None,
            lone: None,
            clean: 0,
            as_labelled: false,
        }
    }

    /// Keys from a PC keyboard (Ctrl, Windows, Alt) plugged into a `host` Mac,
    /// for a PC: each key does what its cap says, as if the keyboard were
    /// plugged in here, so Ctrl+W closes a tab and Ctrl+Left jumps a word.
    /// Nothing is rewritten. Any other pair is as `new`.
    pub fn pc_keyboard(host: Option<System>, target: System) -> Translator {
        let mut t = Translator::new(host, target);
        if host == Some(System::Mac) && !target.is_mac() {
            t.rules = &[];
            t.as_labelled = true;
        }
        t
    }

    /// The key as labelled on a PC keyboard plugged into a Mac.
    fn labelled(&self, scancode: u16) -> u16 {
        if !self.as_labelled {
            return scancode;
        }
        match scancode {
            L_CTRL => L_WIN,
            L_WIN => L_CTRL,
            R_CTRL => R_WIN,
            R_WIN => R_CTRL,
            other => other,
        }
    }

    /// Presses or releases a key that isn't a held modifier.
    fn press(&mut self, key: u16, down: bool, out: &mut Vec<Act>) {
        if down {
            self.clean = 0;
        }
        out.push(Act::Key(key, down));
    }

    fn tap(&mut self, key: u16, out: &mut Vec<Act>) {
        self.press(key, true, out);
        self.press(key, false, out);
    }

    fn host_mods(&self) -> u8 {
        self.host_down
            .iter()
            .filter_map(|k| modifier(*k))
            .fold(0, |a, b| a | b)
    }

    fn held_mods(&self) -> u8 {
        self.sent_mods
            .iter()
            .filter_map(|k| modifier(*k))
            .fold(0, |a, b| a | b)
    }

    /// Makes `want` the modifiers held here. New ones go down before old ones
    /// come up, so letting go of Alt or the Windows key never reads as
    /// tapping it alone (which opens a menu or Start on a PC); with nothing
    /// to press first, a tap of Control stands in.
    fn set_mods(&mut self, want: u8, out: &mut Vec<Act>) {
        let have = self.held_mods();
        let mut pressed = false;
        for bit in [CTRL, SHIFT, ALT, WIN] {
            if want & bit != 0 && have & bit == 0 {
                // The side the other computer holds, if it holds one.
                let key = self
                    .host_down
                    .iter()
                    .copied()
                    .find(|k| modifier(*k) == Some(bit))
                    .unwrap_or_else(|| left_key(bit));
                out.push(Act::Key(key, true));
                self.sent_mods.push(key);
                self.clean = bit;
                pressed = true;
            }
        }
        let releasing: Vec<u16> = self
            .sent_mods
            .iter()
            .copied()
            .filter(|k| modifier(*k).is_some_and(|bit| want & bit == 0))
            .collect();
        let bare = releasing
            .iter()
            .any(|k| modifier(*k).is_some_and(|bit| bit & self.clean & (ALT | WIN) != 0));
        if bare && !pressed && !self.target.is_mac() {
            let mask = if have & CTRL == 0 { L_CTRL } else { L_SHIFT };
            self.tap(mask, out);
        }
        for key in releasing {
            out.push(Act::Key(key, false));
            self.sent_mods.retain(|k| *k != key);
        }
    }

    fn find(&self, mods: u8, key: u16) -> Option<&'static Rule> {
        let applies = |r: &&Rule| r.key == key && r.only.is_none_or(|s| s == self.target);
        self.rules
            .iter()
            .filter(applies)
            .find(|r| r.mods == mods)
            .or_else(|| {
                self.rules
                    .iter()
                    .filter(applies)
                    .find(|r| r.shift_passes && r.mods == mods & !SHIFT)
            })
    }

    /// What a sticky shortcut's modifiers should be now, or None once one of
    /// the modifiers it was pressed with has been let go.
    fn sticky_mods(&self) -> Option<u8> {
        let (asked, given) = self.sticky?;
        let host = self.host_mods();
        (host & asked == asked).then_some(given | (host & SHIFT))
    }

    /// One key from the other computer, pressed (`down`) or released.
    pub fn key(&mut self, scancode: u16, down: bool) -> Vec<Act> {
        let scancode = self.labelled(scancode);
        if self.rules.is_empty() {
            return vec![Act::Key(scancode, down)];
        }
        let mut out = Vec::new();
        match (modifier(scancode), down) {
            (Some(_), true) => {
                // A held modifier repeating changes nothing.
                if self.host_down.contains(&scancode) {
                    return out;
                }
                self.host_down.push(scancode);
                self.lone = Some(scancode);
                match self.sticky_mods() {
                    Some(want) => self.set_mods(want, &mut out),
                    None => {
                        self.sticky = None;
                        if !self.sent_mods.contains(&scancode) {
                            out.push(Act::Key(scancode, true));
                            self.sent_mods.push(scancode);
                            self.clean = modifier(scancode).unwrap_or(0);
                        }
                    }
                }
            }
            (Some(bit), false) => {
                self.host_down.retain(|k| *k != scancode);
                let lone = self.lone.take() == Some(scancode);
                if self.sticky.is_some() {
                    match self.sticky_mods() {
                        Some(want) => self.set_mods(want, &mut out),
                        None => {
                            self.sticky = None;
                            let host = self.host_mods();
                            self.set_mods(host, &mut out);
                        }
                    }
                } else if self.sent_mods.contains(&scancode) {
                    out.push(Act::Key(scancode, false));
                    self.sent_mods.retain(|k| *k != scancode);
                }
                if lone && bit == WIN && self.target.is_mac() && self.host_mods() == 0 {
                    let (mods, key) = PC_LONE_WIN_ON_MAC;
                    self.set_mods(mods, &mut out);
                    self.tap(key, &mut out);
                    self.set_mods(0, &mut out);
                }
            }
            (None, true) => {
                self.lone = None;
                // A held key repeating repeats what it put down.
                if let Some(&(_, given)) = self.active.iter().find(|(k, _)| *k == scancode) {
                    if given != 0 {
                        self.press(given, true, &mut out);
                    }
                    return out;
                }
                self.host_down.push(scancode);
                let host = self.host_mods();
                match self.find(host, scancode) {
                    Some(rule) => match rule.out {
                        Out::Chord(mods, key) => {
                            let want = mods | if rule.shift_passes { host & SHIFT } else { 0 };
                            self.set_mods(want, &mut out);
                            self.press(key, true, &mut out);
                            self.active.push((scancode, key));
                            if rule.sticky {
                                self.sticky = Some((rule.mods, mods));
                            }
                        }
                        Out::Taps(taps) => {
                            for &(mods, key) in taps {
                                self.set_mods(mods, &mut out);
                                self.tap(key, &mut out);
                            }
                            self.active.push((scancode, 0));
                        }
                        Out::Lock => {
                            out.push(Act::Lock);
                            self.active.push((scancode, 0));
                        }
                    },
                    None => {
                        // Anything else is pressed with the modifiers as held.
                        self.sticky = None;
                        self.set_mods(host, &mut out);
                        self.press(scancode, true, &mut out);
                        self.active.push((scancode, scancode));
                    }
                }
            }
            (None, false) => {
                self.host_down.retain(|k| *k != scancode);
                match self.active.iter().position(|(k, _)| *k == scancode) {
                    Some(i) => {
                        let (_, given) = self.active.remove(i);
                        if given != 0 {
                            out.push(Act::Key(given, false));
                        }
                        // A rewrite's own modifiers go with it. The ones it
                        // lifted come back with the next key or click.
                        if self.sticky.is_none() {
                            let keep = self.held_mods() & self.host_mods();
                            self.set_mods(keep, &mut out);
                        }
                    }
                    None => out.push(Act::Key(scancode, false)),
                }
            }
        }
        out
    }

    /// Before a click or scroll: the modifiers held here become the ones the
    /// other computer holds again, so Cmd+click after Cmd+Left still clicks
    /// with Command.
    pub fn pointer(&mut self) -> Vec<Act> {
        let mut out = Vec::new();
        if !self.rules.is_empty() && self.sticky.is_none() {
            let host = self.host_mods();
            if self.held_mods() != host {
                self.set_mods(host, &mut out);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(t: &mut Translator, keys: &[(u16, bool)]) -> Vec<Act> {
        keys.iter().flat_map(|&(k, d)| t.key(k, d)).collect()
    }

    fn k(code: u16, down: bool) -> Act {
        Act::Key(code, down)
    }

    #[test]
    fn same_systems_pass_everything_through() {
        let mut t = Translator::new(Some(System::Windows), System::Linux);
        assert_eq!(t.key(L_ALT, true), vec![k(L_ALT, true)]);
        assert_eq!(t.key(TAB, true), vec![k(TAB, true)]);
        let mut t = Translator::new(None, System::Mac);
        assert_eq!(t.key(HOME, true), vec![k(HOME, true)]);
    }

    #[test]
    fn copy_needs_no_rewrite() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[(L_CTRL, true), (0x2E, true), (0x2E, false), (L_CTRL, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(0x2E, true),
                k(0x2E, false),
                k(L_CTRL, false)
            ]
        );
    }

    #[test]
    fn cmd_tab_walks_the_switcher_as_alt_tab() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[
                (L_CTRL, true),
                (TAB, true),
                (TAB, false),
                (TAB, true),
                (TAB, false),
            ],
        );
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_ALT, true),
                k(L_CTRL, false),
                k(TAB, true),
                k(TAB, false),
                k(TAB, true),
                k(TAB, false),
            ]
        );
        // Shift walks back; letting go of Command picks the app.
        assert_eq!(t.key(L_SHIFT, true), vec![k(L_SHIFT, true)]);
        assert_eq!(t.key(L_SHIFT, false), vec![k(L_SHIFT, false)]);
        assert_eq!(t.key(L_CTRL, false), vec![k(L_ALT, false)]);
        assert!(t.sent_mods.is_empty());
    }

    #[test]
    fn option_arrow_jumps_a_word_without_opening_a_menu() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[(L_ALT, true), (LEFT, true), (LEFT, false), (L_ALT, false)],
        );
        // Ctrl goes down before Alt comes up, and Alt never comes back up alone.
        assert_eq!(
            out,
            vec![
                k(L_ALT, true),
                k(L_CTRL, true),
                k(L_ALT, false),
                k(LEFT, true),
                k(LEFT, false),
                k(L_CTRL, false),
            ]
        );
        assert!(t.sent_mods.is_empty());
    }

    #[test]
    fn cmd_left_is_home_and_cmd_comes_back_for_the_next_key() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(&mut t, &[(L_CTRL, true), (LEFT, true), (LEFT, false)]);
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_CTRL, false),
                k(HOME, true),
                k(HOME, false)
            ]
        );
        // Cmd+C straight after still copies.
        assert_eq!(t.key(0x2E, true), vec![k(L_CTRL, true), k(0x2E, true)]);
    }

    #[test]
    fn shift_rides_along_where_it_should() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(&mut t, &[(L_SHIFT, true), (L_CTRL, true), (RIGHT, true)]);
        assert_eq!(
            out,
            vec![
                k(L_SHIFT, true),
                k(L_CTRL, true),
                k(L_CTRL, false),
                k(END, true)
            ]
        );
    }

    #[test]
    fn a_click_after_a_rewrite_has_the_real_modifiers() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        press(&mut t, &[(L_CTRL, true), (LEFT, true), (LEFT, false)]);
        assert_eq!(t.pointer(), vec![k(L_CTRL, true)]);
        assert_eq!(t.pointer(), vec![]);
    }

    #[test]
    fn a_held_key_repeats_its_rewrite() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        press(&mut t, &[(L_ALT, true), (BACKSPACE, true)]);
        assert_eq!(t.key(BACKSPACE, true), vec![k(BACKSPACE, true)]);
        assert_eq!(t.key(L_ALT, true), vec![]);
        let out = press(&mut t, &[(BACKSPACE, false), (L_ALT, false)]);
        assert_eq!(out, vec![k(BACKSPACE, false), k(L_CTRL, false)]);
    }

    #[test]
    fn cmd_space_opens_start() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[
                (L_CTRL, true),
                (SPACE, true),
                (SPACE, false),
                (L_CTRL, false),
            ],
        );
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_CTRL, false),
                k(L_WIN, true),
                k(L_WIN, false)
            ]
        );
    }

    #[test]
    fn locking_is_an_action_on_windows_and_super_l_on_linux() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(&mut t, &[(L_WIN, true), (L_CTRL, true), (Q, true)]);
        assert_eq!(out.last(), Some(&Act::Lock));
        let mut t = Translator::new(Some(System::Mac), System::Linux);
        let out = press(&mut t, &[(L_WIN, true), (L_CTRL, true), (Q, true)]);
        assert_eq!(out.last(), Some(&k(L, true)));
        assert!(out.contains(&k(L_CTRL, false)));
    }

    #[test]
    fn windows_only_rewrites_stay_off_linux() {
        let mut t = Translator::new(Some(System::Mac), System::Linux);
        let out = press(&mut t, &[(L_CTRL, true), (M, true)]);
        assert_eq!(out, vec![k(L_CTRL, true), k(M, true)]);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_a_mac_line() {
        let mut t = Translator::new(Some(System::Windows), System::Mac);
        let out = press(&mut t, &[(HOME, true), (HOME, false)]);
        assert_eq!(
            out,
            vec![k(L_WIN, true), k(A, true), k(A, false), k(L_WIN, false)]
        );
        let out = press(
            &mut t,
            &[(L_SHIFT, true), (END, true), (END, false), (L_SHIFT, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_SHIFT, true),
                k(L_CTRL, true),
                k(RIGHT, true),
                k(RIGHT, false),
                k(L_CTRL, false),
                k(L_SHIFT, false),
            ]
        );
    }

    #[test]
    fn alt_tab_switches_mac_apps() {
        let mut t = Translator::new(Some(System::Windows), System::Mac);
        let out = press(
            &mut t,
            &[(L_ALT, true), (TAB, true), (TAB, false), (L_ALT, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_ALT, true),
                k(L_CTRL, true),
                k(L_ALT, false),
                k(TAB, true),
                k(TAB, false),
                k(L_CTRL, false),
            ]
        );
    }

    #[test]
    fn ctrl_arrow_and_ctrl_backspace_work_on_words() {
        let mut t = Translator::new(Some(System::Linux), System::Mac);
        let out = press(&mut t, &[(R_CTRL, true), (BACKSPACE, true)]);
        assert_eq!(
            out,
            vec![
                k(R_CTRL, true),
                k(L_ALT, true),
                k(R_CTRL, false),
                k(BACKSPACE, true)
            ]
        );
    }

    #[test]
    fn the_windows_key_alone_opens_spotlight() {
        let mut t = Translator::new(Some(System::Windows), System::Mac);
        let out = press(&mut t, &[(L_WIN, true), (L_WIN, false)]);
        assert_eq!(
            out,
            vec![
                k(L_WIN, true),
                k(L_WIN, false),
                k(L_CTRL, true),
                k(SPACE, true),
                k(SPACE, false),
                k(L_CTRL, false),
            ]
        );
        // Not when it was part of a shortcut.
        let out = press(
            &mut t,
            &[(L_WIN, true), (0x13, true), (0x13, false), (L_WIN, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_WIN, true),
                k(0x13, true),
                k(0x13, false),
                k(L_WIN, false)
            ]
        );
    }

    #[test]
    fn ctrl_y_redoes_on_a_mac() {
        let mut t = Translator::new(Some(System::Windows), System::Mac);
        let out = press(
            &mut t,
            &[(L_CTRL, true), (Y, true), (Y, false), (L_CTRL, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_SHIFT, true),
                k(Z, true),
                k(Z, false),
                k(L_SHIFT, false),
                k(L_CTRL, false),
            ]
        );
    }

    #[test]
    fn cmd_backspace_deletes_to_the_start_of_the_line() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[(L_CTRL, true), (BACKSPACE, true), (BACKSPACE, false)],
        );
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_SHIFT, true),
                k(L_CTRL, false),
                k(HOME, true),
                k(HOME, false),
                k(L_SHIFT, false),
                k(BACKSPACE, true),
                k(BACKSPACE, false),
            ]
        );
    }

    #[test]
    fn letting_go_of_a_bare_windows_key_is_masked() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        // Ctrl+Cmd+F: the Windows key comes up with nothing pressed since.
        let out = press(&mut t, &[(L_CTRL, true), (L_WIN, true), (F, true)]);
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_WIN, true),
                k(L_SHIFT, true),
                k(L_SHIFT, false),
                k(L_CTRL, false),
                k(L_WIN, false),
                k(F11, true),
            ]
        );
    }

    #[test]
    fn cmd_option_esc_opens_task_manager() {
        let mut t = Translator::new(Some(System::Mac), System::Windows);
        let out = press(&mut t, &[(L_CTRL, true), (L_ALT, true), (ESC, true)]);
        assert_eq!(
            out,
            vec![
                k(L_CTRL, true),
                k(L_ALT, true),
                k(L_SHIFT, true),
                k(L_ALT, false),
                k(ESC, true),
            ]
        );
    }

    // A PC keyboard on a Mac: macOS reports its Ctrl as Control (sent as the
    // Windows key) and its Windows key as Command (sent as Ctrl).
    const W: u16 = 0x11;

    #[test]
    fn a_pc_keyboard_on_a_mac_types_ctrl_w_as_ctrl_w() {
        let mut t = Translator::pc_keyboard(Some(System::Mac), System::Windows);
        let out = press(
            &mut t,
            &[(L_WIN, true), (W, true), (W, false), (L_WIN, false)],
        );
        assert_eq!(
            out,
            vec![k(L_CTRL, true), k(W, true), k(W, false), k(L_CTRL, false)]
        );
    }

    #[test]
    fn a_pc_keyboard_on_a_mac_keeps_pc_shortcuts_as_they_are() {
        let mut t = Translator::pc_keyboard(Some(System::Mac), System::Windows);
        // Ctrl+Left jumps a word, Alt+Left goes back, Ctrl+Tab changes tab.
        for (m, key) in [(L_WIN, LEFT), (L_ALT, LEFT), (L_WIN, TAB)] {
            let out = press(&mut t, &[(m, true), (key, true), (key, false), (m, false)]);
            let want = t.labelled(m);
            assert_eq!(
                out,
                vec![k(want, true), k(key, true), k(key, false), k(want, false)]
            );
        }
        // Its Windows key alone opens Start; the right-hand keys swap too.
        assert_eq!(t.key(L_CTRL, true), vec![k(L_WIN, true)]);
        assert_eq!(t.key(L_CTRL, false), vec![k(L_WIN, false)]);
        assert_eq!(t.key(R_WIN, true), vec![k(R_CTRL, true)]);
        assert_eq!(t.key(R_CTRL, true), vec![k(R_WIN, true)]);
    }

    #[test]
    fn a_pc_keyboard_setting_changes_nothing_but_a_mac_driving_a_pc() {
        let mut t = Translator::pc_keyboard(Some(System::Windows), System::Mac);
        // A PC driving a Mac still gets the Mac's shortcuts (Alt+Tab is Cmd+Tab).
        assert_eq!(t.key(L_ALT, true), vec![k(L_ALT, true)]);
        assert_eq!(t.key(TAB, true)[0], k(L_CTRL, true));
        let mut t = Translator::pc_keyboard(Some(System::Mac), System::Mac);
        assert_eq!(t.key(L_WIN, true), vec![k(L_WIN, true)]);
    }
}
