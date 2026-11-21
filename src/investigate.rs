//! Investigation cases (v0.4): every floor's Floor Keeper fight is an
//! investigation of one suspicious thing the floor's trap sends you: an email
//! in a lookalike inbox, a text thread, a DM, a web page, a sign-in push or a
//! mailed check (`Medium`).
//!
//! Content lives in `assets/content/investigations.json`, one case per
//! encounter id. A case has a few message *variants*; each variant lists the
//! red flags it can carry (`Flag`) and, for each, how the message changes when
//! that flag is planted (`Patch`), the trap's in-character line when you catch
//! it, and the plain-language warning sign (`tell`).
//!
//! Every run (`Case::build`), code picks the variant and plants `PLANTED` red
//! flags: "verify it yourself" always (every fake fails it) plus random others.
//! The message shown is the variant's clean message with those patches applied.
//! The act menu offers a check for every flag the variant knows, so some checks
//! come back clean: the clue to look for changes from run to run, and players
//! learn that not every check finds something. What is fake, which flags are
//! planted and what counts as a tell are decided here, never by an AI.

use std::collections::BTreeMap;

use macroquad::prelude::*;
use serde::Deserialize;

use crate::art::{centered, draw_text, measure_text, panel, text_block, wrap, DIM, INK, KSU_GOLD, PAPER, RISKY, SAFE};
use crate::keeper::mix;

/// Red flags planted per run (one is always `Verify`).
pub const PLANTED: usize = 3;
/// At most this many checks in the act menu (plus up to 3 bad acts: 8 slots).
pub const MAX_CHECKS: usize = 5;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    /// a lookalike sender, number, profile or site name
    Sender,
    /// replies go somewhere else, a brand-new account, a mismatched detail
    Headers,
    /// the link goes somewhere other than it says
    Link,
    /// it asks for a password, code, ID or bank login
    Secret,
    /// fees, gift cards, overpayments, pay-to-play
    Money,
    /// a fake deadline, threats, "act now"
    Pressure,
    /// checked through an official channel, it isn't real
    Verify,
}

pub const FLAGS: [Flag; 7] = [Flag::Sender, Flag::Headers, Flag::Link, Flag::Secret, Flag::Money, Flag::Pressure, Flag::Verify];

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    #[default]
    Email,
    Text,
    Dm,
    Web,
    Push,
    Check,
}

impl Medium {
    pub fn noun(self) -> &'static str {
        match self {
            Medium::Email => "email",
            Medium::Text => "text",
            Medium::Dm => "message",
            Medium::Web => "web page",
            Medium::Push => "sign-in request",
            Medium::Check => "check",
        }
    }
}

impl Flag {
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// The act-menu label for checking this flag, worded for the medium.
    pub fn act_label(self, m: Medium) -> &'static str {
        use Medium::*;
        match (self, m) {
            (Flag::Sender, Email) => "Check the sender",
            (Flag::Sender, Text) => "Check the number",
            (Flag::Sender, Dm) => "Check the profile",
            (Flag::Sender, Web) => "Check the site name",
            (Flag::Sender, Push) => "Check the app and location",
            (Flag::Sender, Check) => "Check who sent it",
            (Flag::Headers, Email) => "Check the reply-to",
            (Flag::Headers, Dm) => "Check the account's history",
            (Flag::Headers, Check) => "Check the bank details",
            (Flag::Headers, _) => "Check the details",
            (Flag::Link, Text) => "Long-press the link",
            (Flag::Link, Web) => "Check the address bar",
            (Flag::Link, Push) => "Check what it's for",
            (Flag::Link, Check) => "Look up the bank yourself",
            (Flag::Link, _) => "Hover the link",
            (Flag::Secret, _) => "Read what it asks for",
            (Flag::Money, _) => "Follow the money",
            (Flag::Pressure, _) => "Check the deadline",
            (Flag::Verify, Email) => "Contact the office yourself",
            (Flag::Verify, Text) => "Call the number on KSU's site",
            (Flag::Verify, Dm) => "Look it up on Handshake",
            (Flag::Verify, Web) => "Go to the official site yourself",
            (Flag::Verify, Push) => "Ask: did I just sign in?",
            (Flag::Verify, Check) => "Call your bank yourself",
        }
    }

    /// What you do (box narration).
    pub fn act_flavor(self) -> &'static str {
        match self {
            Flag::Sender => "* You read who it's from. Slowly. Letter by letter.",
            Flag::Headers => "* You look past the obvious parts at the details underneath.",
            Flag::Link => "* You look where the link really goes, without opening it.",
            Flag::Secret => "* You read exactly what it wants you to hand over.",
            Flag::Money => "* You follow the money: who pays whom, and how.",
            Flag::Pressure => "* You check the clock it's waving at you.",
            Flag::Verify => "* You go around it: you find the official contact yourself and ask.",
        }
    }

    /// A check that finds nothing this run.
    pub fn clean(self) -> &'static str {
        match self {
            Flag::Sender => "* That part checks out. Not every check finds something; keep looking.",
            Flag::Headers => "* The details line up this time. Keep looking.",
            Flag::Link => "* The link goes where it says. Keep looking.",
            Flag::Secret => "* It doesn't ask for anything private. Keep looking.",
            Flag::Money => "* No money changes hands here. Keep looking.",
            Flag::Pressure => "* No fake deadline this time. Keep looking.",
            Flag::Verify => "* The office has a record of it. Keep looking.",
        }
    }

    /// The trap's last line when you spare it, by the first flag you caught.
    pub fn takeaway(self) -> &'static str {
        match self {
            Flag::Sender => "You read who I was, letter by letter. Hardly anyone does.",
            Flag::Headers => "You checked the fine details. Sneaky. Effective.",
            Flag::Link => "You looked before you clicked. Ugh.",
            Flag::Secret => "You noticed what I was really asking for.",
            Flag::Money => "You followed the money, and it led straight to me.",
            Flag::Pressure => "You ignored my clock. The deadline was the trap.",
            Flag::Verify => "You went around me to the real office. So unsporting. So effective.",
        }
    }

    /// Hoot's nudge toward a red flag you haven't caught yet: points at the
    /// kind of clue, worded for the medium, never at the answer.
    pub fn nudge(self, m: Medium) -> &'static str {
        use Medium::*;
        match (self, m) {
            (Flag::Sender, Email) => "Hoo, who is this really from? Read the address letter by letter.",
            (Flag::Sender, Text) => "Hoo, who sent this text? Look at the number itself, not the name.",
            (Flag::Sender, Dm) => "Hoo, who is behind this profile? Look closer at who they really are.",
            (Flag::Sender, Web) => "Hoo, whose site is this really? Read its name carefully.",
            (Flag::Sender, Push) => "Hoo, where is this request coming from? Check the app and the place.",
            (Flag::Sender, Check) => "Hoo, who actually wrote this check? Read the name on it.",
            (Flag::Headers, Email) => "Hoo, if you hit reply, where would it really go?",
            (Flag::Headers, _) => "Hoo, look past the obvious parts. The small details underneath can disagree.",
            (Flag::Link, Web) => "Hoo, what does the address bar say? The page can copy everything but that.",
            (Flag::Link, _) => "Hoo, where does that link really go? Look before you tap.",
            (Flag::Secret, _) => "Hoo, read exactly what it wants you to hand over.",
            (Flag::Money, _) => "Hoo, follow the money. Who pays whom, and how?",
            (Flag::Pressure, _) => "Hoo, why the rush? A ticking clock is a classic trick.",
            (Flag::Verify, _) => "Hoo, you can always go around it. Find the official contact yourself and ask.",
        }
    }

    /// Short name for the checklist.
    pub fn short(self) -> &'static str {
        match self {
            Flag::Sender => "Sender",
            Flag::Headers => "Details",
            Flag::Link => "Link",
            Flag::Secret => "Asks for",
            Flag::Money => "Money",
            Flag::Pressure => "Deadline",
            Flag::Verify => "Verified",
        }
    }
}

// ---------------------------------------------------------------- content

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Msg {
    pub sender_name: String,
    pub sender: String,
    pub reply_to: String,
    pub to: String,
    pub time: String,
    pub subject: String,
    pub body: Vec<String>,
    pub link_text: String,
    pub link_url: String,
    pub attachment: String,
    pub banner: String,
    pub fine_print: String,
    pub amount: String,
}

/// How a planted flag changes the message. Each set field replaces the
/// clean one; `add_body` lines are appended.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Patch {
    pub sender_name: Option<String>,
    pub sender: Option<String>,
    pub reply_to: Option<String>,
    pub to: Option<String>,
    pub time: Option<String>,
    pub subject: Option<String>,
    pub body: Option<Vec<String>>,
    pub add_body: Vec<String>,
    pub link_text: Option<String>,
    pub link_url: Option<String>,
    pub attachment: Option<String>,
    pub banner: Option<String>,
    pub fine_print: Option<String>,
    pub amount: Option<String>,
}

impl Patch {
    /// Apply to `m`, recording in `tags` (field -> flag) and `body_tags` (one per
    /// body line) which planted flag changed what, so the right part lights up.
    pub fn apply_tagged(&self, m: &mut Msg, f: Flag, tags: &mut BTreeMap<&'static str, Flag>, body_tags: &mut Vec<Option<Flag>>) {
        for (name, set) in [
            ("sender_name", self.sender_name.is_some()),
            ("sender", self.sender.is_some()),
            ("reply_to", self.reply_to.is_some()),
            ("to", self.to.is_some()),
            ("time", self.time.is_some()),
            ("subject", self.subject.is_some()),
            ("link", self.link_text.is_some() || self.link_url.is_some()),
            ("attachment", self.attachment.is_some()),
            ("banner", self.banner.is_some()),
            ("fine_print", self.fine_print.is_some()),
            ("amount", self.amount.is_some()),
        ] {
            if set {
                tags.insert(name, f);
            }
        }
        if self.body.is_some() {
            body_tags.clear();
        }
        self.apply(m);
        body_tags.resize(m.body.len() - self.add_body.len(), if self.body.is_some() { Some(f) } else { None });
        body_tags.extend(self.add_body.iter().map(|_| Some(f)));
    }

    pub fn apply(&self, m: &mut Msg) {
        let set = |dst: &mut String, src: &Option<String>| {
            if let Some(s) = src {
                dst.clone_from(s);
            }
        };
        set(&mut m.sender_name, &self.sender_name);
        set(&mut m.sender, &self.sender);
        set(&mut m.reply_to, &self.reply_to);
        set(&mut m.to, &self.to);
        set(&mut m.time, &self.time);
        set(&mut m.subject, &self.subject);
        if let Some(b) = &self.body {
            m.body.clone_from(b);
        }
        m.body.extend(self.add_body.iter().cloned());
        set(&mut m.link_text, &self.link_text);
        set(&mut m.link_url, &self.link_url);
        set(&mut m.attachment, &self.attachment);
        set(&mut m.banner, &self.banner);
        set(&mut m.fine_print, &self.fine_print);
        set(&mut m.amount, &self.amount);
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct FlagDef {
    #[serde(default)]
    pub patch: Patch,
    /// the trap's in-character line when you catch this one
    pub line: String,
    /// the warning sign, in plain words (verdict screen, checklist)
    pub tell: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Variant {
    pub id: String,
    pub msg: Msg,
    pub flags: BTreeMap<Flag, FlagDef>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BadAct {
    pub label: String,
    pub flavor: String,
    pub line: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Investigation {
    #[serde(default)]
    pub medium: Medium,
    /// the trap's first line
    pub opening: String,
    /// the box's first narration
    pub intro: String,
    /// the trap's line when you spare it
    pub spare: String,
    /// the safer next move (verdict screen)
    pub safe_move: String,
    /// Hoot's lines about this floor's trap (asked tips while exploring)
    #[serde(default)]
    pub hoot: Vec<String>,
    pub bad_acts: Vec<BadAct>,
    pub variants: Vec<Variant>,
}

/// A real-looking message for the inbox around the trap's one.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Decoy {
    pub sender_name: String,
    pub subject: String,
    pub time: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Cases {
    #[serde(default)]
    pub decoys: Vec<Decoy>,
    #[serde(default)]
    pub cases: BTreeMap<String, Investigation>,
}

pub const EMBEDDED: &str = include_str!("../assets/content/investigations.json");

pub fn load() -> Cases {
    serde_json::from_str(EMBEDDED).unwrap_or_default()
}

// ---------------------------------------------------------------- one run's case

/// One move in the act menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Check(Flag),
    Bad(u8),
}

#[derive(Clone, Debug)]
pub struct Case {
    /// encounter id (also the character sprite's id)
    pub id: String,
    /// the trap's name, e.g. "The Inbox Mimic"
    pub name: String,
    pub medium: Medium,
    pub opening: String,
    pub intro: String,
    pub spare: String,
    pub safe_move: String,
    pub variant: String,
    /// the message as shown this run (clean message + planted patches)
    pub msg: Msg,
    /// red flags planted this run (sorted)
    pub planted: Vec<Flag>,
    /// (catch line, tell) for each planted flag
    pub lines: BTreeMap<Flag, (String, String)>,
    /// which planted flag changed which field of `msg`
    pub tags: BTreeMap<&'static str, Flag>,
    /// which planted flag added (or rewrote) each body line
    pub body_tags: Vec<Option<Flag>>,
    pub bad: Vec<BadAct>,
    /// other mail in the inbox (email only)
    pub decoys: Vec<Decoy>,
    /// the act menu, in its (seeded) order
    pub acts: Vec<Act>,
}

fn shuffle<T>(v: &mut [T], seed: u32) {
    let mut h = seed;
    for i in (1..v.len()).rev() {
        h = mix(h, i as u32 + 29);
        v.swap(i, h as usize % (i + 1));
    }
}

impl Case {
    /// This run's case for encounter `id` (named `name`), from `seed`.
    pub fn build(inv: &Investigation, decoys: &[Decoy], id: &str, name: &str, seed: u32) -> Option<Case> {
        if inv.variants.is_empty() {
            return None;
        }
        let v = &inv.variants[mix(seed, 0x5641_5249) as usize % inv.variants.len()];
        let known: Vec<Flag> = v.flags.keys().copied().collect();
        let mut planted: Vec<Flag> = known.iter().copied().filter(|f| *f == Flag::Verify).collect();
        let mut others: Vec<Flag> = known.iter().copied().filter(|f| *f != Flag::Verify).collect();
        shuffle(&mut others, mix(seed, 0x464c_4147));
        planted.extend(others.iter().copied().take(PLANTED.saturating_sub(planted.len())));
        planted.sort();
        // checks: every planted flag, plus clean ones (as many as fit)
        let mut checks = planted.clone();
        for f in others.iter().copied().filter(|f| !planted.contains(f)) {
            if checks.len() < MAX_CHECKS {
                checks.push(f);
            }
        }
        let mut msg = v.msg.clone();
        let mut tags = BTreeMap::new();
        let mut body_tags = vec![None; msg.body.len()];
        for f in &planted {
            v.flags[f].patch.apply_tagged(&mut msg, *f, &mut tags, &mut body_tags);
        }
        let lines = planted.iter().map(|f| (*f, (v.flags[f].line.clone(), v.flags[f].tell.clone()))).collect();
        let bad: Vec<BadAct> = inv.bad_acts.iter().take(3).cloned().collect();
        let mut acts: Vec<Act> = checks.iter().map(|f| Act::Check(*f)).collect();
        acts.extend((0..bad.len() as u8).map(Act::Bad));
        shuffle(&mut acts, mix(seed, 0x4143_5453));
        let mut pool: Vec<Decoy> = decoys.to_vec();
        shuffle(&mut pool, mix(seed, 0x4445_4359));
        pool.truncate(3);
        Some(Case {
            id: id.to_string(),
            name: name.to_string(),
            medium: inv.medium,
            opening: inv.opening.clone(),
            intro: inv.intro.clone(),
            spare: inv.spare.clone(),
            safe_move: inv.safe_move.clone(),
            variant: v.id.clone(),
            msg,
            planted,
            lines,
            tags,
            body_tags,
            bad,
            decoys: if inv.medium == Medium::Email { pool } else { Vec::new() },
            acts,
        })
    }

    pub fn is_planted(&self, f: Flag) -> bool {
        self.planted.contains(&f)
    }

    pub fn act_label(&self, a: Act) -> &str {
        match a {
            Act::Check(f) => f.act_label(self.medium),
            Act::Bad(i) => self.bad.get(i as usize).map_or("Do what it says", |b| b.label.as_str()),
        }
    }

    /// The tell for a planted flag.
    pub fn tell(&self, f: Flag) -> &str {
        self.lines.get(&f).map_or("", |(_, t)| t.as_str())
    }
}

// ---------------------------------------------------------------- drawing

const LINK_BLUE: Color = Color::new(0.45, 0.70, 1.0, 1.0);
const PAGE: Color = Color::new(0.09, 0.08, 0.12, 1.0);
const ROW: Color = Color::new(0.13, 0.11, 0.17, 1.0);
const FOUND_BG: Color = Color::new(0.45, 0.12, 0.12, 0.55);
const CLEAN_BG: Color = Color::new(0.12, 0.35, 0.16, 0.45);

impl Case {
    /// How to mark field `name`, which the `f` check looks at: red when the flag
    /// that changed this field has been caught, green when its own check came
    /// back clean, else nothing.
    /// Body line `i`: red when the caught flag that added it has been caught.
    fn line_mark(&self, i: usize, found: u8) -> Option<Color> {
        match self.body_tags.get(i) {
            Some(Some(t)) if found & t.bit() != 0 => Some(FOUND_BG),
            _ => None,
        }
    }

    fn field_mark(&self, name: &str, f: Flag, found: u8, checked: u8) -> Option<Color> {
        match self.tags.get(name) {
            Some(t) if found & t.bit() != 0 => Some(FOUND_BG),
            Some(_) => None,
            None => (checked & f.bit() != 0 && found & f.bit() == 0).then_some(CLEAN_BG),
        }
    }
}

fn fit(s: &str, size: f32, w: f32) -> String {
    if measure_text(s, None, size as u16, 1.0).width <= w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if measure_text(&format!("{out}..."), None, size as u16, 1.0).width > w {
            out.pop();
            break;
        }
    }
    format!("{}...", out.trim_end())
}

/// A labelled header line, highlighted when its flag was checked.
fn header_line(label: &str, value: &str, x: f32, y: f32, w: f32, size: f32, m: Option<Color>) {
    if let Some(c) = m {
        draw_rectangle(x - 6.0, y - size, w + 12.0, size * 1.35, c);
    }
    draw_text(label, x, y, size, DIM);
    let lw = measure_text(label, None, size as u16, 1.0).width + 10.0;
    draw_text(fit(value, size, w - lw), x + lw, y, size, PAPER);
}

impl Case {
    /// The small card at the top left of the fight: a mini inbox for email,
    /// a lock-screen notification for the rest.
    pub fn draw_preview(&self, x: f32, y: f32, w: f32, h: f32) {
        panel(x, y, w, h, KSU_GOLD);
        let m = &self.msg;
        if self.medium == Medium::Email {
            draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 30.0, Color::new(0.28, 0.22, 0.05, 1.0));
            draw_text("KSU Mail  -  Inbox", x + 16.0, y + 25.0, 20.0, KSU_GOLD);
            // the trap's message and two real-looking ones fit without crowding
            let shown = self.decoys.len().min(2);
            let rows = shown + 1;
            // leave the bottom line for the "INSPECT to open it" hint
            let rh = ((h - 66.0) / rows as f32).min(44.0);
            // the trap's message sits second from the top, unread
            let mut i = 0;
            let mut draw_row = |name: &str, subject: &str, time: &str, unread: bool| {
                let ry = y + 40.0 + i as f32 * rh;
                draw_rectangle(x + 8.0, ry, w - 16.0, rh - 4.0, if unread { Color::new(0.30, 0.24, 0.08, 1.0) } else { ROW });
                if unread {
                    draw_circle(x + 20.0, ry + rh * 0.5 - 2.0, 5.0, KSU_GOLD);
                }
                draw_text(fit(name, 17.0, 150.0), x + 32.0, ry + 17.0, 17.0, if unread { KSU_GOLD } else { PAPER });
                draw_text(time, x + w - 20.0 - measure_text(time, None, 15, 1.0).width, ry + 17.0, 15.0, DIM);
                draw_text(fit(subject, 16.0, w - 52.0), x + 32.0, ry + rh - 12.0, 16.0, if unread { PAPER } else { DIM });
                i += 1;
            };
            let mut decoys = self.decoys.iter().take(shown);
            if let Some(d) = decoys.next() {
                draw_row(&d.sender_name, &d.subject, &d.time, false);
            }
            draw_row(&m.sender_name, &m.subject, &m.time, true);
            for d in decoys {
                draw_row(&d.sender_name, &d.subject, &d.time, false);
            }
        } else {
            let (label, col) = match self.medium {
                Medium::Text => ("MESSAGES", Color::new(0.4, 0.85, 0.5, 1.0)),
                Medium::Dm => ("NEW MESSAGE", Color::new(0.75, 0.55, 1.0, 1.0)),
                Medium::Web => ("BROWSER", LINK_BLUE),
                Medium::Push => ("SIGN-IN REQUEST", KSU_GOLD),
                Medium::Check | Medium::Email => ("IN THE MAIL", PAPER),
            };
            draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 30.0, Color::new(col.r * 0.28, col.g * 0.28, col.b * 0.28, 1.0));
            draw_text(label, x + 16.0, y + 25.0, 20.0, col);
            draw_text(&m.time, x + w - 20.0 - measure_text(&m.time, None, 16, 1.0).width, y + 24.0, 16.0, DIM);
            draw_text(fit(&m.sender_name, 19.0, w - 40.0), x + 18.0, y + 60.0, 19.0, KSU_GOLD);
            let first = if m.subject.is_empty() { m.body.first().map_or("", String::as_str) } else { m.subject.as_str() };
            text_block(first, x + 18.0, y + 88.0, 17.0, w - 36.0, PAPER);
        }
        centered("INSPECT to open it", x + w * 0.5, y + h - 10.0, 15.0, DIM);
    }

    /// The full message (INSPECT): a lookalike mail client, phone, browser,
    /// push prompt or check, with every checked field marked (red = a red
    /// flag you caught, green = checked and clean).
    pub fn draw_full(&self, x: f32, y: f32, w: f32, h: f32, found: u8, checked: u8) {
        panel(x, y, w, h, KSU_GOLD);
        draw_rectangle(x + 3.0, y + 3.0, w - 6.0, h - 6.0, PAGE);
        let side = 300.0;
        let (cx, cw) = (x + 20.0, w - side - 50.0);
        let m = &self.msg;
        let fm = |name: &str, f: Flag| self.field_mark(name, f, found, checked);
        // the sender line covers both the name and the address
        let who = |f: Flag| fm("sender", f).or_else(|| fm("sender_name", f));
        match self.medium {
            Medium::Email => {
                // title bar and a folder strip, like a mail client
                draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 34.0, Color::new(0.16, 0.20, 0.34, 1.0));
                draw_text("KSU Mail", x + 18.0, y + 27.0, 21.0, PAPER);
                draw_text("Inbox  >  Message", x + 130.0, y + 27.0, 17.0, DIM);
                let mut yy = y + 70.0;
                if let Some(c) = fm("subject", Flag::Pressure) {
                    draw_rectangle(cx - 6.0, yy - 24.0, cw + 12.0, 32.0, c);
                }
                draw_text(fit(&m.subject, 24.0, cw), cx, yy, 24.0, PAPER);
                yy += 34.0;
                header_line("From:", &format!("{} <{}>", m.sender_name, m.sender), cx, yy, cw, 18.0, who(Flag::Sender));
                yy += 26.0;
                let reply = if m.reply_to.is_empty() { m.sender.clone() } else { m.reply_to.clone() };
                header_line("Reply-To:", &reply, cx, yy, cw, 18.0, fm("reply_to", Flag::Headers));
                yy += 26.0;
                header_line("To:", if m.to.is_empty() { "you@students.kennesaw.example" } else { &m.to }, cx, yy, cw, 18.0, None);
                yy += 26.0;
                header_line("Sent:", &m.time, cx, yy, cw, 18.0, fm("time", Flag::Pressure));
                yy += 14.0;
                if !m.banner.is_empty() {
                    draw_rectangle(cx - 6.0, yy, cw + 12.0, 28.0, Color::new(0.55, 0.42, 0.08, 0.9));
                    draw_text(fit(&m.banner, 17.0, cw - 8.0), cx + 4.0, yy + 20.0, 17.0, INK);
                    yy += 36.0;
                }
                yy += 22.0;
                yy = self.body_lines(cx, yy, cw, found, checked);
                self.attachment_and_print(cx, yy, cw, found, checked);
            }
            Medium::Text | Medium::Dm => {
                let top = if self.medium == Medium::Text { "Messages" } else { "Handshake  -  Messages" };
                draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 34.0, Color::new(0.12, 0.25, 0.16, 1.0));
                draw_text(top, x + 18.0, y + 27.0, 20.0, PAPER);
                let mut yy = y + 68.0;
                header_line(if self.medium == Medium::Text { "From:" } else { "Profile:" }, &format!("{}  {}", m.sender_name, m.sender), cx, yy, cw, 18.0, who(Flag::Sender));
                yy += 26.0;
                if !m.reply_to.is_empty() {
                    header_line("Account:", &m.reply_to, cx, yy, cw, 18.0, fm("reply_to", Flag::Headers));
                    yy += 26.0;
                }
                header_line("Received:", &m.time, cx, yy, cw, 18.0, fm("time", Flag::Pressure));
                yy += 34.0;
                // the thread as a bubble; lines a caught red flag added light up
                let n: usize = m.body.iter().map(|l| wrap(l, 19.0, cw - 60.0).len()).sum();
                let bh = n as f32 * 23.0 + 18.0;
                draw_rectangle(cx, yy - 16.0, cw - 30.0, bh, Color::new(1.0, 1.0, 1.0, 0.08));
                let mut ly = yy + 4.0;
                for (i, line) in m.body.iter().enumerate() {
                    let wl = wrap(line, 19.0, cw - 60.0);
                    if let Some(c) = self.line_mark(i, found) {
                        draw_rectangle(cx + 6.0, ly - 18.0, cw - 42.0, wl.len() as f32 * 23.0, c);
                    }
                    for l in wl.iter() {
                        draw_text(l, cx + 14.0, ly, 19.0, PAPER);
                        ly += 23.0;
                    }
                }
                yy += bh + 10.0;
                self.attachment_and_print(cx, yy, cw, found, checked);
            }
            Medium::Web => {
                // browser chrome: tab, address bar
                draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 34.0, Color::new(0.20, 0.20, 0.24, 1.0));
                draw_text(fit(&m.subject, 18.0, cw), x + 18.0, y + 26.0, 18.0, PAPER);
                let bar_y = y + 46.0;
                if let Some(c) = fm("link", Flag::Link) {
                    draw_rectangle(cx - 6.0, bar_y - 4.0, cw + 12.0, 34.0, c);
                }
                draw_rectangle(cx, bar_y, cw, 26.0, Color::new(0.05, 0.05, 0.08, 1.0));
                draw_text(fit(&format!("https://{}", m.link_url), 17.0, cw - 16.0), cx + 8.0, bar_y + 19.0, 17.0, PAPER);
                let mut yy = bar_y + 60.0;
                header_line("Site:", &format!("{} ({})", m.sender_name, m.sender), cx, yy, cw, 18.0, who(Flag::Sender));
                yy += 26.0;
                if !m.reply_to.is_empty() {
                    header_line("Contact:", &m.reply_to, cx, yy, cw, 18.0, fm("reply_to", Flag::Headers));
                    yy += 26.0;
                }
                yy += 14.0;
                yy = self.body_lines(cx, yy, cw, found, checked);
                self.attachment_and_print(cx, yy, cw, found, checked);
            }
            Medium::Push => {
                draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 34.0, Color::new(0.10, 0.10, 0.12, 1.0));
                draw_text("Lock screen", x + 18.0, y + 27.0, 20.0, PAPER);
                let mut yy = y + 76.0;
                // the push card: title, app, request, time, details, then the buttons
                let card_h = 26.0 + 30.0 + 26.0 * 3.0 + 8.0 + 24.0 * m.body.len() as f32 + 56.0;
                draw_rectangle(cx, yy - 26.0, cw - 30.0, card_h, Color::new(1.0, 1.0, 1.0, 0.07));
                draw_text(fit(&m.subject, 22.0, cw - 60.0), cx + 14.0, yy, 22.0, PAPER);
                yy += 30.0;
                header_line("App:", &format!("{}  {}", m.sender_name, m.sender), cx + 14.0, yy, cw - 60.0, 18.0, who(Flag::Sender));
                yy += 26.0;
                header_line("Request:", &m.reply_to, cx + 14.0, yy, cw - 60.0, 18.0, fm("reply_to", Flag::Headers));
                yy += 26.0;
                header_line("Sent:", &m.time, cx + 14.0, yy, cw - 60.0, 18.0, fm("time", Flag::Pressure));
                yy += 34.0;
                for (i, line) in m.body.iter().enumerate() {
                    if let Some(c) = self.line_mark(i, found) {
                        draw_rectangle(cx + 8.0, yy - 24.0, cw - 46.0, 24.0, c);
                    }
                    draw_text(fit(line, 18.0, cw - 60.0), cx + 14.0, yy - 6.0, 18.0, PAPER);
                    yy += 24.0;
                }
                draw_rectangle(cx + 14.0, yy - 18.0, 140.0, 30.0, SAFE);
                draw_text("Approve", cx + 40.0, yy + 3.0, 19.0, INK);
                draw_rectangle(cx + 170.0, yy - 18.0, 140.0, 30.0, RISKY);
                draw_text("Deny", cx + 210.0, yy + 3.0, 19.0, INK);
                yy += 50.0;
                self.attachment_and_print(cx, yy, cw, found, checked);
            }
            Medium::Check => {
                draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 34.0, Color::new(0.20, 0.18, 0.12, 1.0));
                draw_text("In the mail", x + 18.0, y + 27.0, 20.0, PAPER);
                // the check itself
                let (kx, ky, kw, kh) = (cx, y + 56.0, cw - 20.0, 140.0);
                draw_rectangle(kx, ky, kw, kh, Color::new(0.80, 0.86, 0.78, 1.0));
                draw_rectangle_lines(kx, ky, kw, kh, 2.0, Color::new(0.3, 0.4, 0.3, 1.0));
                if let Some(c) = fm("sender_name", Flag::Sender) {
                    draw_rectangle(kx + 8.0, ky + 8.0, kw * 0.55, 24.0, c);
                }
                draw_text(fit(&m.sender_name, 18.0, kw * 0.55), kx + 14.0, ky + 26.0, 18.0, INK);
                draw_text(&m.time, kx + kw - 160.0, ky + 26.0, 16.0, INK);
                draw_text("PAY TO THE ORDER OF: You", kx + 14.0, ky + 64.0, 17.0, INK);
                if let Some(c) = fm("amount", Flag::Money) {
                    draw_rectangle(kx + kw - 190.0, ky + 44.0, 176.0, 28.0, c);
                }
                draw_text(&m.amount, kx + kw - 180.0, ky + 64.0, 22.0, INK);
                if let Some(c) = fm("sender", Flag::Headers) {
                    draw_rectangle(kx + 8.0, ky + kh - 36.0, kw - 16.0, 26.0, c);
                }
                draw_text(fit(&m.sender, 16.0, kw - 30.0), kx + 14.0, ky + kh - 17.0, 16.0, INK);
                let yy = self.body_lines(cx, ky + kh + 34.0, cw, found, checked);
                self.attachment_and_print(cx, yy, cw, found, checked);
            }
        }
        self.draw_findings(x + w - side - 20.0, y + 50.0, side, h - 70.0, found, checked);
    }

    /// Body text plus the link line.
    fn body_lines(&self, x: f32, mut y: f32, w: f32, found: u8, checked: u8) -> f32 {
        let m = &self.msg;
        for (i, line) in m.body.iter().enumerate() {
            if let Some(c) = self.line_mark(i, found) {
                let h = wrap(line, 19.0, w).len() as f32 * 19.0 * 1.15;
                draw_rectangle(x - 6.0, y - 19.0, w + 12.0, h + 4.0, c);
            }
            y = text_block(line, x, y, 19.0, w, PAPER) + 4.0;
        }
        if !m.link_text.is_empty() {
            y += 6.0;
            let lw = measure_text(&m.link_text, None, 19, 1.0).width;
            draw_rectangle(x, y - 18.0, lw + 20.0, 26.0, Color::new(0.10, 0.22, 0.45, 1.0));
            draw_text(&m.link_text, x + 10.0, y, 19.0, LINK_BLUE);
            y += 28.0;
            // the real address shows once you've checked the link
            if checked & Flag::Link.bit() != 0 {
                let c = self.field_mark("link", Flag::Link, found, checked).unwrap_or(CLEAN_BG);
                header_line("Goes to:", &m.link_url, x, y, w, 17.0, Some(c));
                y += 26.0;
            }
        }
        y
    }

    fn attachment_and_print(&self, x: f32, mut y: f32, w: f32, found: u8, checked: u8) {
        let m = &self.msg;
        if !m.attachment.is_empty() {
            draw_rectangle(x, y - 16.0, measure_text(&m.attachment, None, 17, 1.0).width + 40.0, 26.0, ROW);
            draw_text(&format!("[+] {}", m.attachment), x + 10.0, y + 2.0, 17.0, PAPER);
            y += 32.0;
        }
        if !m.fine_print.is_empty() {
            let c = self.field_mark("fine_print", Flag::Secret, found, checked);
            let lines = wrap(&m.fine_print, 15.0, w).len() as f32;
            if let Some(c) = c {
                draw_rectangle(x - 6.0, y - 14.0, w + 12.0, lines * 17.5 + 6.0, c);
            }
            text_block(&m.fine_print, x, y, 15.0, w, DIM);
        }
    }

    /// The investigation checklist on the right of the full view.
    fn draw_findings(&self, x: f32, y: f32, w: f32, h: f32, found: u8, checked: u8) {
        draw_rectangle(x, y, w, h, Color::new(0.07, 0.06, 0.10, 1.0));
        draw_rectangle_lines(x, y, w, h, 1.5, Color::new(1.0, 0.77, 0.16, 0.4));
        draw_text("YOUR FINDINGS", x + 14.0, y + 26.0, 18.0, KSU_GOLD);
        let n = found.count_ones();
        draw_text(&format!("Red flags: {n} of {}", self.planted.len()), x + 14.0, y + 50.0, 17.0, PAPER);
        let mut yy = y + 82.0;
        for f in FLAGS {
            if checked & f.bit() == 0 {
                continue;
            }
            let caught = found & f.bit() != 0;
            draw_text(if caught { "RED FLAG" } else { "CLEAN" }, x + 14.0, yy, 15.0, if caught { RISKY } else { SAFE });
            draw_text(f.short(), x + 100.0, yy, 15.0, DIM);
            yy += 20.0;
            if caught {
                yy = text_block(self.tell(f), x + 14.0, yy, 15.0, w - 28.0, PAPER) + 6.0;
            }
            if yy > y + h - 20.0 {
                break;
            }
        }
        if checked == 0 {
            text_block("Use ACT to check things. Each check shows up here, and red flags light up on the message.", x + 14.0, yy, 15.0, w - 28.0, DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Cases {
        serde_json::from_str(EMBEDDED).expect("investigations.json parses")
    }

    #[test]
    fn every_case_builds_for_many_seeds() {
        let c = cases();
        assert!(c.cases.len() >= 10, "{} cases", c.cases.len());
        for (id, inv) in &c.cases {
            assert!(inv.variants.len() >= 2, "{id}: needs at least two variants");
            assert_eq!(inv.bad_acts.len(), 3, "{id}: three bad acts");
            for v in &inv.variants {
                assert!(v.flags.contains_key(&Flag::Verify), "{id}/{}: every fake fails verification", v.id);
                assert!(v.flags.len() > PLANTED, "{id}/{}: needs a clean check too", v.id);
            }
            for seed in 0..40 {
                let k = Case::build(inv, &c.decoys, id, "Name", seed).unwrap();
                assert_eq!(k.planted.len(), PLANTED, "{id}");
                assert!(k.is_planted(Flag::Verify), "{id}");
                assert!(k.acts.len() <= 8, "{id}: {} acts", k.acts.len());
                let checks = k.acts.iter().filter(|a| matches!(a, Act::Check(_))).count();
                assert!(checks > PLANTED, "{id}: at least one clean check");
                for f in &k.planted {
                    assert!(k.acts.contains(&Act::Check(*f)), "{id}: planted {f:?} has a check");
                    assert!(!k.tell(*f).is_empty());
                    // every planted flag (but Verify) changed something you can see light up
                    if *f != Flag::Verify {
                        assert!(k.tags.values().any(|t| t == f) || k.body_tags.contains(&Some(*f)), "{id}: {f:?} marks nothing");
                    }
                }
                assert_eq!(k.body_tags.len(), k.msg.body.len(), "{id}");
            }
        }
    }

    #[test]
    fn runs_differ() {
        let c = cases();
        for (id, inv) in &c.cases {
            let mut seen = std::collections::BTreeSet::new();
            for seed in 0..60 {
                let k = Case::build(inv, &c.decoys, id, "Name", seed).unwrap();
                seen.insert((k.variant.clone(), k.planted.clone()));
            }
            assert!(seen.len() >= 4, "{id}: only {} different runs", seen.len());
        }
    }

    #[test]
    fn planted_flags_change_the_message() {
        let c = cases();
        for (id, inv) in &c.cases {
            for v in &inv.variants {
                for (f, def) in &v.flags {
                    if *f == Flag::Verify {
                        continue;
                    }
                    let mut m = v.msg.clone();
                    def.patch.apply(&mut m);
                    assert_ne!(m, v.msg, "{id}/{}: planting {f:?} must show on the message", v.id);
                }
            }
        }
    }

    #[test]
    fn content_is_safe_to_show() {
        let c = cases();
        let mut all = String::new();
        for inv in c.cases.values() {
            all.push_str(&serde_json::to_string(&inv.variants.iter().map(|v| (&v.msg.sender, &v.msg.link_url, &v.msg.reply_to)).collect::<Vec<_>>()).unwrap());
            for v in &inv.variants {
                for d in v.flags.values() {
                    for s in [&d.patch.sender, &d.patch.link_url, &d.patch.reply_to].into_iter().flatten() {
                        all.push_str(s);
                        all.push(' ');
                    }
                }
            }
        }
        // no real domains: addresses and links use .example
        for word in all.split(|c: char| c.is_whitespace() || c == '"' || c == ',' || c == '[' || c == ']' || c == '(' || c == ')') {
            if word.contains('.') && (word.contains('@') || word.contains('/')) {
                assert!(word.contains(".example"), "real-looking domain: {word}");
            }
        }
    }

    #[test]
    fn hoot_lines_pass_the_tip_checks() {
        use crate::ai_features::{hints, validate};
        let c = cases();
        for (id, inv) in &c.cases {
            assert!(inv.hoot.len() >= 3, "{id}: three Hoot lines");
            for l in &inv.hoot {
                assert!(hints::starts_with_hoo(l), "{id}: {l}");
                assert!(validate::game_line(l, crate::ai_features::tips::MAX_TIP_CHARS).is_some(), "{id}: {l}");
            }
        }
        for f in FLAGS {
            for m in [Medium::Email, Medium::Text, Medium::Dm, Medium::Web, Medium::Push, Medium::Check] {
                let l = f.nudge(m);
                assert!(validate::game_line(l, crate::ai_features::tips::MAX_TIP_CHARS).is_some(), "{l}");
            }
        }
    }

    #[test]
    fn decoys_exist() {
        assert!(cases().decoys.len() >= 4);
    }
}
