//! Optional USB-serial link to the ELEGOO / Arduino Mega 2560 status display (inherited
//! from Delve; bonus feature). See hardware/README.md for the protocol.
//!
//! The current kit build is OUTPUT-ONLY (LCD1602, 4-digit 7-segment, MAX7219, DS1307):
//! it sends `HELLO:CRYPT` and a `HB` heartbeat, never `J:` lines, so the keyboard is the
//! only control. The `J:` joystick path from Delve's Uno build is still parsed (an old
//! board keeps working), but nothing ever waits for it: with no `J:` line the joystick
//! simply reads as centred and unpressed.
//!
//! All port I/O runs on a background thread; the game only talks to it through
//! channels, so a slow or missing device can never stall the 60 Hz loop.
//! When the `hardware` feature is off, or on the web build, a do-nothing stub
//! with the same API is used instead.

// In builds without the serial link most of this module is unused.
// Campus Crypt only uses part of Delve's controller API (no settings screen), and in
// builds without the serial link most of this module is unused.
#![allow(dead_code)]

use macroquad::prelude::{vec2, Vec2};

/// Connection status shown in Settings > Hardware.
#[derive(Clone, Debug, PartialEq)]
pub enum HwStatus {
    Disabled,
    /// let go of the USB port while a new sketch is uploaded
    Held,
    Unsupported,
    Searching,
    Connected(String),
    NotFound,
    Busy(String),
}

impl HwStatus {
    /// Borrowed for the fixed states; only the port-name states allocate.
    pub fn label(&self) -> std::borrow::Cow<'static, str> {
        use std::borrow::Cow::{Borrowed, Owned};
        match self {
            HwStatus::Disabled => Borrowed("Off"),
            HwStatus::Held => Borrowed("Paused while a sketch uploads"),
            HwStatus::Unsupported => Borrowed("Not available in this build"),
            HwStatus::Searching => Borrowed("Searching..."),
            HwStatus::Connected(p) => Owned(format!("Connected on {p}")),
            HwStatus::NotFound => Borrowed("Not found"),
            HwStatus::Busy(p) => Owned(format!("{p} is busy (close Serial Monitor)")),
        }
    }
    pub fn connected(&self) -> bool {
        matches!(self, HwStatus::Connected(_))
    }
}

/// Raw joystick reading from the device: x/y in -1..1 (+y = up).
#[derive(Clone, Copy, Debug, Default)]
pub struct Joy {
    pub x: f32,
    pub y: f32,
    pub btn: bool,
}

/// Menu navigation edges derived from the joystick.
#[derive(Clone, Copy, Debug, Default)]
pub struct JoyNav {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub select: bool,
}

pub const DEADZONE: f32 = 0.25;

/// Shared, platform-independent joystick handling.
#[derive(Default)]
struct JoyState {
    joy: Joy,
    prev_btn: bool,
    btn_pressed: bool,
    nav: JoyNav,
    nav_dir: (i8, i8),
    nav_timer: f32,
}

impl JoyState {
    fn update(&mut self, joy: Joy, dt: f32) {
        self.joy = joy;
        self.btn_pressed = joy.btn && !self.prev_btn;
        self.prev_btn = joy.btn;
        let dir = |v: f32| -> i8 {
            if v > 0.55 {
                1
            } else if v < -0.55 {
                -1
            } else {
                0
            }
        };
        let d = (dir(joy.x), dir(joy.y));
        let mut fire = false;
        if d != self.nav_dir {
            self.nav_dir = d;
            self.nav_timer = 0.35; // first repeat delay
            fire = d != (0, 0);
        } else if d != (0, 0) {
            self.nav_timer -= dt;
            if self.nav_timer <= 0.0 {
                self.nav_timer = 0.14; // repeat rate
                fire = true;
            }
        }
        self.nav = JoyNav {
            up: fire && d.1 > 0,
            down: fire && d.1 < 0,
            left: fire && d.0 < 0,
            right: fire && d.0 > 0,
            select: self.btn_pressed,
        };
    }

    /// Movement vector in screen space (+y = down) with a radial deadzone.
    fn move_vec(&self) -> Vec2 {
        let v = vec2(self.joy.x, -self.joy.y);
        let len = v.length();
        if len < DEADZONE {
            return Vec2::ZERO;
        }
        let scaled = ((len - DEADZONE) / (1.0 - DEADZONE)).min(1.0);
        v / len * scaled
    }
}

/// Parse "J:x,y,btn" (x/y -100..100).
/// A line that identifies a controller during the handshake: the Mega build's
/// `HELLO:CRYPT` (or its `HB` heartbeat), Delve's `HELLO:DELVE`, or an old J: stream.
pub fn is_controller_line(text: &str) -> bool {
    matches!(text, "HELLO:CRYPT" | "HELLO:DELVE" | "HB") || parse_joy(text).is_some()
}

pub fn parse_joy(line: &str) -> Option<Joy> {
    let rest = line.strip_prefix("J:")?;
    let mut it = rest.split(',').map(|s| s.trim());
    let x: f32 = it.next()?.parse().ok()?;
    let y: f32 = it.next()?.parse().ok()?;
    let b: i32 = it.next()?.parse().ok()?;
    Some(Joy {
        x: (x / 100.0).clamp(-1.0, 1.0),
        y: (y / 100.0).clamp(-1.0, 1.0),
        btn: b != 0,
    })
}

/// What the game tells the controller (encoded as an `S:` line).
#[derive(Clone, Debug, PartialEq)]
pub struct HwState {
    pub hp: i32,
    pub max_hp: i32,
    pub level: u32,
    pub floor: u32,
    pub gold: u32,
    pub kills: u32,
    pub mode: char, // M menu, P playing, F Keeper fight, X paused, D dead
    pub show_kills: bool,
    /// floors in the run (the LCD's "F3/10"); sent as the 9th field
    pub floors: u32,
}

impl HwState {
    pub fn line(&self) -> String {
        format!(
            "S:{},{},{},{},{},{},{},{},{}",
            self.hp,
            self.max_hp,
            self.level,
            self.floor,
            self.gold,
            self.kills,
            self.mode,
            if self.show_kills { 'K' } else { 'F' },
            self.floors
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HwEvent {
    Hit,
    Die,
    LevelUp,
    Stairs,
    Menu,
}

impl HwEvent {
    pub fn line(self) -> &'static str {
        match self {
            HwEvent::Hit => "E:HIT",
            HwEvent::Die => "E:DIE",
            HwEvent::LevelUp => "E:LVL",
            HwEvent::Stairs => "E:STAIRS",
            HwEvent::Menu => "E:MENU",
        }
    }
}

// ---------------------------------------------------------------- real link
#[cfg(all(feature = "hardware", not(target_arch = "wasm32")))]
mod link {
    use super::*;
    use std::io::{ErrorKind, Read, Write};
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::thread;
    use std::time::{Duration, Instant};

    pub(super) enum Cmd {
        Line(String),
        /// live-layout lines (hardware/README.md), written a few ms apart so the
        /// board's 64-byte receive buffer never overflows
        Paced(Vec<String>),
        SetEnabled(bool),
        Rescan,
    }

    pub(super) enum Msg {
        Status(HwStatus),
        Joy(Joy),
    }

    pub struct Hardware {
        tx: Sender<Cmd>,
        rx: Receiver<Msg>,
        status: HwStatus,
        js: JoyState,
        last_joy: Joy,
        last_state: Option<HwState>,
        state_timer: f32,
        enabled: bool,
        held: bool,
    }

    impl Hardware {
        pub fn new(enabled: bool, port_override: Option<String>) -> Hardware {
            let (tx, cmd_rx) = channel();
            let (msg_tx, rx) = channel();
            thread::Builder::new()
                .name("crypt-serial".into())
                .spawn(move || worker(cmd_rx, msg_tx, enabled, port_override))
                .expect("spawn serial thread");
            Hardware {
                tx,
                rx,
                status: if enabled { HwStatus::Searching } else { HwStatus::Disabled },
                js: JoyState::default(),
                last_joy: Joy::default(),
                last_state: None,
                state_timer: 0.0,
                enabled,
                held: false,
            }
        }

        /// Call once per frame.
        pub fn poll(&mut self, dt: f32) {
            loop {
                match self.rx.try_recv() {
                    Ok(Msg::Status(s)) => {
                        // the worker reports Disabled while held; keep showing why
                        let s = if self.held && s == HwStatus::Disabled { HwStatus::Held } else { s };
                        if !s.connected() {
                            self.last_joy = Joy::default();
                        }
                        if s.connected() && !self.status.connected() {
                            self.last_state = None; // resend everything
                        }
                        self.status = s;
                    }
                    Ok(Msg::Joy(j)) => self.last_joy = j,
                    Err(_) => break,
                }
            }
            self.js.update(self.last_joy, dt);
            self.state_timer -= dt;
        }

        pub fn status(&self) -> &HwStatus {
            &self.status
        }
        pub fn set_enabled(&mut self, on: bool) {
            if on != self.enabled {
                self.enabled = on;
                self.apply();
            }
        }
        /// Let go of the port while a sketch uploads (true), then look for the board again.
        pub fn hold(&mut self, held: bool) {
            if held != self.held {
                self.held = held;
                self.apply();
            }
        }
        fn apply(&mut self) {
            let on = self.enabled && !self.held;
            self.status = if on { HwStatus::Searching } else if self.held { HwStatus::Held } else { HwStatus::Disabled };
            let _ = self.tx.send(Cmd::SetEnabled(on));
        }
        /// Send live-layout lines (LT:/LM:/LB:/LSAVE...); false if no board is connected.
        pub fn send_layout(&mut self, lines: Vec<String>) -> bool {
            if !self.status.connected() || lines.is_empty() {
                return false;
            }
            self.tx.send(Cmd::Paced(lines)).is_ok()
        }
        pub fn rescan(&mut self) {
            if self.enabled {
                self.status = HwStatus::Searching;
                let _ = self.tx.send(Cmd::Rescan);
            }
        }
        // The board is output-only: J: lines from an old Delve (joystick) sketch are
        // ignored, so a floating joystick pin can never move the player or press keys.
        pub fn move_vec(&self) -> Vec2 {
            Vec2::ZERO
        }
        pub fn button_held(&self) -> bool {
            false
        }
        pub fn nav(&self) -> JoyNav {
            JoyNav::default()
        }
        /// Send the state line when it changes (and at least every 0.5 s as a heartbeat).
        pub fn send_state(&mut self, s: HwState) {
            if !self.status.connected() {
                return;
            }
            let changed = self.last_state.as_ref() != Some(&s);
            // changes go out at most 10 times a second; otherwise a 0.5 s heartbeat
            let due = if changed { self.state_timer <= 0.0 } else { self.state_timer <= -0.4 };
            if due {
                let _ = self.tx.send(Cmd::Line(s.line()));
                self.last_state = Some(s);
                self.state_timer = 0.1;
            }
        }
        pub fn send_event(&mut self, e: HwEvent) {
            if self.status.connected() {
                let _ = self.tx.send(Cmd::Line(e.line().to_string()));
            }
        }
    }

    impl Drop for Hardware {
        fn drop(&mut self) {
            let _ = self.tx.send(Cmd::SetEnabled(false));
        }
    }

    /// Serial devices that look like an Arduino (or a CH340/CP210x clone).
    fn candidates() -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/dev") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let lower = name.to_lowercase();
                let mac = name.starts_with("cu.")
                    && (lower.contains("usbmodem") || lower.contains("usbserial") || lower.contains("wch") || lower.contains("slab"));
                let linux = name.starts_with("ttyACM") || name.starts_with("ttyUSB");
                if mac || linux {
                    out.push(format!("/dev/{name}"));
                }
            }
        }
        out.sort();
        out
    }

    enum Probe {
        Ok(Box<dyn serialport::SerialPort>, Vec<u8>),
        Busy,
        No,
        /// a command arrived that stops the scan
        Aborted(Abort),
    }

    #[derive(Clone, Copy)]
    enum Abort {
        Disable,
        Rescan,
        Quit,
    }

    /// Open a port and wait for "HELLO:CRYPT" (or HELLO:DELVE / HB / a J: line). Opening resets the board (DTR), and
    /// its bootloader takes ~1.5 s, so we wait up to 3.5 s and PING after 2 s.
    fn probe(path: &str, cmd_rx: &Receiver<Cmd>) -> Probe {
        let mut port = match serialport::new(path, 115_200).timeout(Duration::from_millis(50)).open() {
            Ok(p) => p,
            Err(e) => {
                let d = e.description.to_lowercase();
                let busy = d.contains("busy") || d.contains("resource") || d.contains("lock");
                return if busy { Probe::Busy } else { Probe::No };
            }
        };
        let start = Instant::now();
        let mut pinged = false;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        while start.elapsed() < Duration::from_millis(3500) {
            if let Some(a) = drain_cmds(cmd_rx) {
                return Probe::Aborted(a);
            }
            if !pinged && start.elapsed() > Duration::from_millis(2000) {
                let _ = port.write_all(b"PING\n");
                pinged = true;
            }
            match port.read(&mut chunk) {
                Ok(n) if n > 0 => {
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = buf.drain(..=pos).collect();
                        let text = String::from_utf8_lossy(&line);
                        let text = text.trim();
                        // a running controller streams J: lines, so those identify it too
                        if is_controller_line(text) {
                            return Probe::Ok(port, buf);
                        }
                    }
                    if buf.len() > 1024 {
                        buf.clear();
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => return Probe::No,
            }
        }
        Probe::No
    }

    /// Handle commands that arrive while we're not connected.
    fn drain_cmds(cmd_rx: &Receiver<Cmd>) -> Option<Abort> {
        loop {
            match cmd_rx.try_recv() {
                Ok(Cmd::Line(_)) | Ok(Cmd::Paced(_)) | Ok(Cmd::SetEnabled(true)) => {}
                Ok(Cmd::SetEnabled(false)) => return Some(Abort::Disable),
                Ok(Cmd::Rescan) => return Some(Abort::Rescan),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => return Some(Abort::Quit),
            }
        }
    }

    fn worker(cmd_rx: Receiver<Cmd>, msg_tx: Sender<Msg>, mut enabled: bool, port_override: Option<String>) {
        // ports that answered but aren't a Campus Crypt / Delve controller: don't re-open them
        // (and reset whatever board is on them) more than every 20 s
        let mut skip_until: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();
        let mut last_status: Option<HwStatus> = None;
        let mut send_status = |s: HwStatus, tx: &Sender<Msg>| {
            if last_status.as_ref() != Some(&s) {
                last_status = Some(s.clone());
                let _ = tx.send(Msg::Status(s));
            }
        };
        loop {
            if !enabled {
                send_status(HwStatus::Disabled, &msg_tx);
                match cmd_rx.recv() {
                    Ok(Cmd::SetEnabled(true)) | Ok(Cmd::Rescan) => enabled = true,
                    Ok(_) => {}
                    Err(_) => return,
                }
                continue;
            }
            send_status(HwStatus::Searching, &msg_tx);
            let now = Instant::now();
            skip_until.retain(|_, t| *t > now);
            let ports: Vec<String> = match &port_override {
                Some(p) => vec![p.clone()],
                None => candidates().into_iter().filter(|p| !skip_until.contains_key(p)).collect(),
            };
            let mut busy: Option<String> = None;
            let mut connected = None;
            let mut rescan = false;
            for p in &ports {
                // check for disable/rescan between probes, too
                let r = match drain_cmds(&cmd_rx) {
                    Some(a) => Probe::Aborted(a),
                    None => probe(p, &cmd_rx),
                };
                match r {
                    Probe::Ok(port, leftover) => {
                        connected = Some((p.clone(), port, leftover));
                        break;
                    }
                    Probe::Busy => busy = Some(p.clone()),
                    Probe::No => {
                        if port_override.is_none() {
                            skip_until.insert(p.clone(), Instant::now() + Duration::from_secs(20));
                        }
                    }
                    Probe::Aborted(Abort::Disable) => {
                        enabled = false;
                        break;
                    }
                    Probe::Aborted(Abort::Rescan) => {
                        rescan = true;
                        break;
                    }
                    Probe::Aborted(Abort::Quit) => return,
                }
            }
            if rescan {
                skip_until.clear();
                continue;
            }
            if !enabled {
                continue;
            }
            if let Some((path, port, leftover)) = connected {
                send_status(HwStatus::Connected(path.clone()), &msg_tx);
                match session(port, leftover, &cmd_rx, &msg_tx) {
                    SessionEnd::Lost => {}
                    SessionEnd::Disabled => enabled = false,
                    SessionEnd::Rescan => {}
                    SessionEnd::Quit => return,
                }
                continue; // reconnect / rescan straight away
            }
            send_status(
                match busy {
                    Some(p) => HwStatus::Busy(p),
                    None => HwStatus::NotFound,
                },
                &msg_tx,
            );
            // wait ~2 s before scanning again, but react to commands immediately
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                match cmd_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Cmd::SetEnabled(false)) => {
                        enabled = false;
                        break;
                    }
                    Ok(Cmd::Rescan) => {
                        skip_until.clear();
                        break;
                    }
                    Ok(_) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return,
                }
            }
        }
    }

    enum SessionEnd {
        Lost,
        Disabled,
        Rescan,
        Quit,
    }

    fn session(mut port: Box<dyn serialport::SerialPort>, mut buf: Vec<u8>, cmd_rx: &Receiver<Cmd>, msg_tx: &Sender<Msg>) -> SessionEnd {
        let _ = port.set_timeout(Duration::from_millis(10));
        let mut last_rx = Instant::now();
        let mut chunk = [0u8; 512];
        loop {
            // outgoing
            loop {
                match cmd_rx.try_recv() {
                    Ok(Cmd::Line(l)) => {
                        let mut bytes = l.into_bytes();
                        bytes.push(b'\n');
                        if port.write_all(&bytes).is_err() {
                            return SessionEnd::Lost;
                        }
                    }
                    Ok(Cmd::Paced(lines)) => {
                        for l in lines {
                            let mut bytes = l.into_bytes();
                            bytes.push(b'\n');
                            if port.write_all(&bytes).is_err() {
                                return SessionEnd::Lost;
                            }
                            // ~90 bytes take 8 ms at 115200 baud; leave the board time to parse
                            thread::sleep(Duration::from_millis(25));
                        }
                        // the board answered LOK lines meanwhile: it's alive
                        last_rx = Instant::now();
                    }
                    Ok(Cmd::SetEnabled(false)) => return SessionEnd::Disabled,
                    Ok(Cmd::SetEnabled(true)) => {}
                    Ok(Cmd::Rescan) => return SessionEnd::Rescan,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return SessionEnd::Quit,
                }
            }
            // incoming
            match port.read(&mut chunk) {
                Ok(n) if n > 0 => {
                    last_rx = Instant::now();
                    buf.extend_from_slice(&chunk[..n]);
                    let mut newest: Option<Joy> = None;
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = buf.drain(..=pos).collect();
                        let s = String::from_utf8_lossy(&line);
                        if let Some(j) = parse_joy(s.trim()) {
                            newest = Some(j);
                        }
                    }
                    if let Some(j) = newest {
                        let _ = msg_tx.send(Msg::Joy(j));
                    }
                    if buf.len() > 4096 {
                        buf.clear();
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => return SessionEnd::Lost,
            }
            // the controller sends a HB line every 0.5 s (Delve's Uno build: J: at 50 Hz);
            // 2 s of silence = unplugged/hung
            if last_rx.elapsed() > Duration::from_secs(2) {
                return SessionEnd::Lost;
            }
            if let Some(name) = port.name() {
                if !std::path::Path::new(&name).exists() {
                    return SessionEnd::Lost; // unplugged
                }
            }
        }
    }
}

#[cfg(all(feature = "hardware", not(target_arch = "wasm32")))]
pub use link::Hardware;

// ---------------------------------------------------------------- stub
#[cfg(not(all(feature = "hardware", not(target_arch = "wasm32"))))]
pub struct Hardware {
    status: HwStatus,
    #[allow(dead_code)]
    js: JoyState,
}

#[cfg(not(all(feature = "hardware", not(target_arch = "wasm32"))))]
#[allow(dead_code)]
impl Hardware {
    pub fn new(_enabled: bool, _port_override: Option<String>) -> Hardware {
        Hardware { status: HwStatus::Unsupported, js: JoyState::default() }
    }
    pub fn poll(&mut self, _dt: f32) {}
    pub fn status(&self) -> &HwStatus {
        &self.status
    }
    pub fn set_enabled(&mut self, _on: bool) {}
    pub fn hold(&mut self, _held: bool) {}
    pub fn send_layout(&mut self, _lines: Vec<String>) -> bool {
        false
    }
    pub fn rescan(&mut self) {}
    pub fn move_vec(&self) -> Vec2 {
        Vec2::ZERO
    }
    pub fn button_held(&self) -> bool {
        false
    }
    pub fn nav(&self) -> JoyNav {
        JoyNav::default()
    }
    pub fn send_state(&mut self, _s: HwState) {}
    pub fn send_event(&mut self, _e: HwEvent) {}
}

/// `--hw-port <path>` on the command line, or the CAMPUS_CRYPT_HW_PORT (or DELVE_HW_PORT) env var.
pub fn port_override() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        for (i, a) in args.iter().enumerate() {
            if a == "--hw-port" {
                if let Some(p) = args.get(i + 1) {
                    return Some(p.clone());
                }
            }
            if let Some(p) = a.strip_prefix("--hw-port=") {
                return Some(p.to_string());
            }
        }
        for var in ["CAMPUS_CRYPT_HW_PORT", "DELVE_HW_PORT"] {
            if let Ok(p) = std::env::var(var) {
                if !p.is_empty() {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_line_carries_the_fight_mode_and_floor_count() {
        let st = HwState { hp: 4, max_hp: 5, level: 2, floor: 1, gold: 3, kills: 0, mode: 'F', show_kills: false, floors: 10 };
        assert_eq!(st.line(), "S:4,5,2,1,3,0,F,F,10");
        let k = HwState { show_kills: true, mode: 'P', ..st };
        assert_eq!(k.line(), "S:4,5,2,1,3,0,P,K,10");
    }

    #[test]
    fn handshake_accepts_the_mega_sketch_and_old_boards_only() {
        for ok in ["HELLO:CRYPT", "HB", "HELLO:DELVE", "J:0,0,0"] {
            assert!(is_controller_line(ok), "{ok}");
        }
        for no in ["", "HELLO", "hello:crypt", "S:1,2", "J:1,2", "garbage"] {
            assert!(!is_controller_line(no), "{no}");
        }
        // the output-only heartbeat is never read as a joystick
        assert!(parse_joy("HB").is_none() && parse_joy("HELLO:CRYPT").is_none());
    }

    #[test]
    fn no_joystick_lines_means_no_input() {
        // the Mega build never sends J:, so the joystick state stays at its default:
        // no movement, no menu edges, no button, however long the game runs
        let mut js = JoyState::default();
        for _ in 0..600 {
            js.update(Joy::default(), 1.0 / 60.0);
            let n = js.nav;
            assert!(!(n.up || n.down || n.left || n.right || n.select));
            assert_eq!(js.move_vec(), Vec2::ZERO);
            assert!(!js.joy.btn);
        }
        // and the stub/real Hardware with nothing connected reports nothing either
        let hw = Hardware::new(false, None);
        assert_eq!(hw.move_vec(), Vec2::ZERO);
        assert!(!hw.button_held());
        let n = hw.nav();
        assert!(!(n.up || n.down || n.left || n.right || n.select));
    }
}
