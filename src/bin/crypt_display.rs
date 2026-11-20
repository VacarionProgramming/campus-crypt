//! crypt_display: talk to the Campus Crypt status display (Arduino Mega 2560 running
//! hardware/campus_crypt_lcd_matrix) with no game running.
//!
//!   crypt_display ports                     list serial ports that look like an Arduino
//!   crypt_display send <file> [--port P]    send the L... lines in <file> (`-` = stdin), one
//!                                           at a time, waiting for each LOK; LSAVE waits for
//!                                           LOK:SAVED. Prints one JSON line with the result.
//!
//! While the game runs it owns the port: use hardware/tools/live.sh, which hands the
//! lines to the game instead (src/display_live.rs).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/dev") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let mac = name.starts_with("cu.") && (lower.contains("usbmodem") || lower.contains("usbserial") || lower.contains("wch") || lower.contains("slab"));
            let linux = name.starts_with("ttyACM") || name.starts_with("ttyUSB");
            if mac || linux {
                out.push(format!("/dev/{name}"));
            }
        }
    }
    out.sort();
    out
}

struct Board {
    port: Box<dyn serialport::SerialPort>,
    buf: Vec<u8>,
}

impl Board {
    /// Open the port (which resets the Mega) and wait for HELLO:CRYPT.
    fn open(path: &str) -> Result<Board, String> {
        let port = serialport::new(path, 115_200).timeout(Duration::from_millis(50)).open().map_err(|e| format!("{path}: {e}"))?;
        let mut b = Board { port, buf: Vec::new() };
        let start = Instant::now();
        let mut pinged = false;
        while start.elapsed() < Duration::from_millis(4000) {
            if !pinged && start.elapsed() > Duration::from_millis(2000) {
                let _ = b.port.write_all(b"PING\n");
                pinged = true;
            }
            if let Some(l) = b.line(Duration::from_millis(100)) {
                if l == "HELLO:CRYPT" {
                    return Ok(b);
                }
            }
        }
        Err(format!("{path}: no HELLO:CRYPT (is the campus_crypt_lcd_matrix sketch on it?)"))
    }

    /// The next line within `wait`, if any.
    fn line(&mut self, wait: Duration) -> Option<String> {
        let start = Instant::now();
        let mut chunk = [0u8; 256];
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let raw: Vec<u8> = self.buf.drain(..=pos).collect();
                return Some(String::from_utf8_lossy(&raw).trim().to_string());
            }
            if start.elapsed() >= wait {
                return None;
            }
            if let Ok(n) = self.port.read(&mut chunk) {
                self.buf.extend_from_slice(&chunk[..n]);
            }
        }
    }

    /// Send one line and wait for its answer (`LOK`, `LOK:SAVED` or `LERR`).
    fn send(&mut self, l: &str) -> Result<(), String> {
        self.port.write_all(format!("{l}\n").as_bytes()).map_err(|e| e.to_string())?;
        let (want, wait) = if l == "LSAVE" { ("LOK:SAVED", 10_000) } else { ("LOK", 1500) };
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(wait) {
            match self.line(Duration::from_millis(100)).as_deref() {
                Some(a) if a == want => return Ok(()),
                Some("LERR") => return Err(format!("board refused: {l}")),
                _ => {}
            }
        }
        Err(format!("no {want} for: {l}"))
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port_arg = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("ports") => {
            let p: Vec<String> = candidates().iter().map(|s| json_str(s)).collect();
            println!("{{\"ports\":[{}]}}", p.join(","));
        }
        Some("send") => {
            let Some(file) = args.get(1) else {
                eprintln!("usage: crypt_display send <file|-> [--port P]");
                std::process::exit(2);
            };
            let mut text = String::new();
            let read = if file == "-" { std::io::stdin().read_to_string(&mut text).map(|_| ()) } else { std::fs::read_to_string(file).map(|t| text = t) };
            if let Err(e) = read {
                println!("{{\"ok\":false,\"error\":{}}}", json_str(&format!("{file}: {e}")));
                std::process::exit(1);
            }
            let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| l.starts_with('L') && l.len() <= 90 && l.is_ascii()).collect();
            let ports = port_arg.map(|p| vec![p]).unwrap_or_else(candidates);
            if ports.is_empty() {
                println!("{{\"ok\":false,\"error\":\"no Arduino-like serial port found (is the board plugged in?)\"}}");
                std::process::exit(1);
            }
            let mut last_err = String::new();
            for p in &ports {
                let mut board = match Board::open(p) {
                    Ok(b) => b,
                    Err(e) => {
                        last_err = e;
                        continue;
                    }
                };
                let mut errors = Vec::new();
                for l in &lines {
                    if let Err(e) = board.send(l) {
                        errors.push(json_str(&e));
                    }
                }
                println!("{{\"ok\":{},\"port\":{},\"sent\":{},\"errors\":[{}]}}", errors.is_empty(), json_str(p), lines.len(), errors.join(","));
                std::process::exit(if errors.is_empty() { 0 } else { 1 });
            }
            println!("{{\"ok\":false,\"error\":{}}}", json_str(&last_err));
            std::process::exit(1);
        }
        _ => {
            eprintln!("usage: crypt_display ports | send <file|-> [--port P]");
            std::process::exit(2);
        }
    }
}
