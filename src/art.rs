//! Procedural pixel art and drawing helpers. Every sprite is a tiny text grid
//! drawn as rectangles, so the game ships with no external image assets
//! (nothing to license or credit).

use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Virtual canvas size; everything is laid out in these coordinates and the
/// camera letterboxes it to the real window.
pub const VW: f32 = 1280.0;
pub const VH: f32 = 720.0;

pub const INK: Color = Color::new(0.08, 0.06, 0.10, 1.0);
pub const KSU_GOLD: Color = Color::new(1.0, 0.77, 0.16, 1.0); // KSU-ish gold
pub const PAPER: Color = Color::new(0.93, 0.90, 0.82, 1.0);
pub const DIM: Color = Color::new(0.70, 0.66, 0.62, 1.0);
pub const SAFE: Color = Color::new(0.40, 0.86, 0.48, 1.0);
pub const RISKY: Color = Color::new(0.95, 0.36, 0.33, 1.0);
pub const MIXED: Color = Color::new(0.98, 0.72, 0.28, 1.0);
pub const FOCUS: Color = Color::new(0.38, 0.70, 1.0, 1.0);
pub const PANEL: Color = Color::new(0.10, 0.08, 0.13, 0.94);

fn pal(c: char) -> Option<Color> {
    let rgb = |r: u8, g: u8, b: u8| Some(Color::from_rgba(r, g, b, 255));
    match c {
        'k' => rgb(20, 16, 24),
        'd' => rgb(52, 50, 62),   // hoodie charcoal
        'D' => rgb(34, 32, 42),
        'y' => rgb(255, 196, 40), // gold
        'Y' => rgb(196, 140, 24),
        's' => rgb(226, 174, 130), // skin
        'S' => rgb(184, 132, 96),
        'h' => rgb(70, 44, 30), // hair
        'w' => rgb(246, 244, 236),
        'l' => rgb(212, 214, 226), // pale ghost / paper
        'L' => rgb(150, 156, 180),
        'b' => rgb(60, 86, 150), // jeans
        'B' => rgb(40, 58, 104),
        'n' => rgb(140, 88, 48), // wood
        'N' => rgb(96, 58, 30),
        'r' => rgb(214, 64, 70),
        'R' => rgb(120, 24, 40),
        'e' => rgb(80, 180, 90),
        'E' => rgb(40, 110, 60),
        'p' => rgb(130, 90, 180),
        'P' => rgb(80, 50, 120),
        'c' => rgb(120, 220, 230),
        'o' => rgb(250, 150, 50),
        'g' => rgb(130, 130, 140),
        'G' => rgb(84, 82, 96),
        'm' => rgb(226, 92, 180), // spam magenta
        'M' => rgb(140, 44, 110),
        'C' => rgb(40, 130, 150), // deep sea
        'x' => rgb(255, 244, 170), // lure glow
        _ => None,
    }
}

pub const STUDENT: [&str; 16] = [
    ".....kkkkkk.....",
    "....khhhhhhk....",
    "...khhhhhhhhk...",
    "...khsssssshk...",
    "...kskksskksk...",
    "...kssssssssk...",
    "....ksssSssk....",
    "...kdkkkkkkdk...",
    "..kddddyyddddk..",
    "..kdkdyddydkdk..",
    "..ksk.dyyd.ksk..",
    "...k.kddddk.k...",
    ".....kbbkbbk....",
    ".....kbbkbbk....",
    ".....kBBkBBk....",
    "....kkkk.kkkk...",
];

const MIMIC: [&str; 16] = [
    "................",
    "..kkkkkkkkkkkk..",
    ".knnnnnyynnnnnk.",
    ".knwkwnyynwkwnk.",
    ".kNNNNNyyNNNNNk.",
    ".kkkkkkkkkkkkkk.",
    ".kwkwkwkwkwkwkk.",
    ".kRRRRllllRRRRk.",
    ".kRRRlLrrLlRRRk.",
    ".kRRRRllllRRRRk.",
    ".kwkwkwkwkwkwkk.",
    ".knnnnkyyknnnnk.",
    ".knnnnkYYknnnnk.",
    ".knnnnnyynnnnnk.",
    ".kNNNNNNNNNNNNk.",
    "..kkkkkkkkkkkk..",
];

const WRAITH: [&str; 16] = [
    "......kkkk......",
    "....kkllllkk....",
    "...kllllllllk...",
    "..kllkkllkkllk..",
    "..klkyykkyyklk..",
    "..klkykkkykklk..",
    "..kllkkllkkllk..",
    "..klllkkkklllk..",
    ".kyyllllllllyyk.",
    ".kyYlllllllllYk.",
    "..kllllyyllllk..",
    "..klllyYYylllk..",
    "..kLlllllllllk..",
    "..kLlLllLllLlk..",
    "..kkLkkLkkLkkk..",
    "...k..k..k..k...",
];

const HYDRA: [&str; 16] = [
    ".kkk...kkk...kkk",
    "keeek.keeek.keee",
    "kewek.kewek.kewe",
    "keeEk.keeEk.keeE",
    ".kyk...kyk...kyk",
    ".kek...kek...kek",
    ".keek..kek..keek",
    "..keek.kek.keek.",
    "...keekeeekeek..",
    "....keeeeeeek...",
    "...keeEeEeEeek..",
    "..keeeEeEeEeeek.",
    "..keEeeeeeeeEek.",
    "...keeeeeeeeek..",
    "....kEEEEEEEk...",
    ".....kkkkkkk....",
];

const IMPOSTOR: [&str; 16] = [
    ".....kkkkkk.....",
    "....kkkkkkkk....",
    "...kkkkkkkkkk...",
    "....ksssssssk...",
    "....kskkskksk...",
    "....kssssssk....",
    "....kkkkkkkk....",
    ".....ksssSk.....",
    "...knnnwwnnnk...",
    "..knnnnwrnnnnk..",
    "..knknnwrnnknk..",
    "..ksknnnnnnksk..",
    "...kknnnnnnk.kk.",
    "....knnnknnnkyyk",
    "....kNNkkNNkkyyk",
    "...kkkk..kkkk.kk",
];

const HOODED: [&str; 16] = [
    "......kkkk......",
    ".....kppppk.....",
    "....kppppppk....",
    "...kppkkkkppk...",
    "...kpkyykyykpk..",
    "...kpkkkkkkpk...",
    "...kppkkkkppk...",
    "..kppppppppppk..",
    "..kpPppppppPpk..",
    ".kppPppppppPppk.",
    ".kppPppppppPppk.",
    ".kpPPppppppPPpk.",
    ".kpPppppppppPpk.",
    ".kPPPPPPPPPPPPk.",
    "..kkkkkkkkkkkk..",
    "................",
];

// The six KSU floors added for the 10-floor run (black and gold, original art).

/// Encounter: The Ticket Troll, a grey troll in a black hoodie waving a fake gold parking ticket.
const TICKET_TROLL: [&str; 16] = [
    "................",
    "....kkkkkkkk....",
    "...kggggggggk...",
    "..kgwkggggwkgk..",
    "..kgkkggggkkgk..",
    "..kggggGGggggk..",
    "..kgwkwkwkwkgk..",
    "...kgggggggkk...",
    "..kkddddddddkyk.",
    ".kgkdyddddydkwyk",
    ".kgkddyyyyddkyyk",
    ".kgkdddddddkkwyk",
    "..kkddddddddkyyk",
    "...kddk..kddkkk.",
    "...kggk..kggk...",
    "...kkkk..kkkk...",
];

/// Encounter: The Commute Chimera, half traffic cone, half gold car (the Kennesaw-Marietta drive).
const COMMUTE_CHIMERA: [&str; 16] = [
    "......k.........",
    ".....kok........",
    "....kowok.......",
    "....kooook......",
    "...kowwwwok.....",
    "...koooooook....",
    "..kkkkkkkkkkk...",
    ".kyyyyyyyyyyyk..",
    "kykkkkykkkkkyyk.",
    "kykLLkykLLLkyyk.",
    "kyyyyyyyyyyyyyxk",
    "kyxyyykkkkyyyyyk",
    "kkkkkkkkkkkkkkkk",
    ".kgggk....kgggk.",
    ".kgkgk....kgkgk.",
    "..kkk......kkk..",
];

/// Encounter: The Gift Card Gargoyle, a stone gargoyle hugging a gold gift card.
const GIFT_CARD_GARGOYLE: [&str; 16] = [
    ".k............k.",
    "kgk..kkkkkk..kgk",
    "kggkkggggggkkggk",
    ".kgkgrkggkrgkgk.",
    "..kggggggggggk..",
    "..kgwkwkkwkwgk..",
    "kk.kggggggggk.kk",
    "kGk.kGggggGk.kGk",
    "kGGkkggggggkkGGk",
    "kGGkkkkkkkkkkGGk",
    ".kkkyyyyyyyykkk.",
    "...kykkkkkkyk...",
    "...kyyyyyyyyk...",
    "...kyYkyykYyk...",
    "...kkkkkkkkkk...",
    "................",
];

/// Encounter: The Push Poltergeist, a ghost with a phone of sign-in approvals you never started.
const PUSH_POLTERGEIST: [&str; 16] = [
    "......kkkk......",
    "....kkllllkk....",
    "...kllllllllk...",
    "..kllkkllkkllk..",
    "..klkrkllkrklk..",
    "..kllkkllkkllk..",
    "..klllkkkklllk..",
    ".kllllllllllkkkk",
    ".klllllllllkdyyk",
    ".klLlllllllkdyek",
    "..kllllLlllkdyyk",
    "..kLllllllLkdeyk",
    "..kLlLllLlLkdyyk",
    "..kkLkkLkkLkkkkk",
    "...k..k..k..k...",
    "................",
];

/// Encounter: The Overpay Ogre, a green ogre in black with a gold dollar sign and an overpaid check.
const OVERPAY_OGRE: [&str; 16] = [
    "................",
    "....kkkkkkkk....",
    "...keeeeeeeek...",
    "..keykeeeekyek..",
    "..keekeeeekeek..",
    "..keeeeEEeeeek..",
    "..kewkkkkkkwek..",
    "...keeeeeeeek...",
    ".kkkddddddddkkk.",
    "keekdddyydddkwwk",
    "keekddyddyddkwek",
    "keekdddyydddkwwk",
    ".kkkdddddddddkk.",
    "...kddk..kddk...",
    "...keek..keek...",
    "...kkkk..kkkk...",
];

/// Encounter: The Cram Lich, a skull in a gold robe holding a candle, cramming from an open book.
const CRAM_LICH: [&str; 16] = [
    ".....kkkkkk.....",
    "....kllllllk....",
    "...kllllllllk...",
    "...klkkllkklk...",
    "...klkrllkrlk...",
    "....kllkkllk....",
    "....klwlwlwk....",
    ".x.kykkkkkkyk...",
    "kyk.kyyyyyyk....",
    ".n.kyyykkkyyyk..",
    ".n.kykwwwwwkyk..",
    ".n.kykrrrrrkyk..",
    ".nkyykwwwwwkyyk.",
    ".nkyykbbbbbkyyk.",
    ".nkyyyyyyyyyyyk.",
    "..kkkkkkkkkkkkk.",
];

// ---------------------------------------------------------------- dungeon cast
//
// The crawl layer's sprites: the Floor Keeper, its four wardens (bosses), the
// floor monsters and the coffee pickup. Same 16x16 text-grid style as above.

/// The Floor Keeper: a hooded charcoal owl gatekeeper with a gold lantern and a ring of keys.
pub const KEEPER: [&str; 16] = [
    "...kk......kk...",
    "..kddk....kddk..",
    "..kdddkkkkdddk..",
    ".kddddddddddddk.",
    ".kdwwwkddkwwwdk.",
    ".kdwykkddkkywdk.",
    ".kdddddoodddddk.",
    "..kddddooddddk.y",
    ".kddddddddddddkx",
    "kddyddddddddydky",
    "kddyddddddddydk.",
    "kddyyyyyyyyyydk.",
    "kdddddddddddddk.",
    ".kdddddddddddk..",
    "..kNNk...kNNk...",
    "..kkkk...kkkk...",
];

/// Warden: The Tow Truck Tyrant, a black-and-gold tow truck off the parking deck (beacon, hook, headlights).
pub const TOW_TYRANT: [&str; 16] = [
    "......rr........",
    ".....kxxk.......",
    "....kkkkkk......",
    "...kyyyyyyk.....",
    "..kykkkkkkyk....",
    "..kykwkkwkyk..n.",
    "..kyyyyyyyyk.n..",
    ".kkyyyyyyyykkn..",
    "kyyyyyyyyyyyykn.",
    "kykkyyyyyykkyk..",
    "kyxkyyyyyykxykgk",
    "kyyyyrrrryyyykgk",
    "kkkkkkkkkkkkkkk.",
    ".kgggk....kgggk.",
    ".kgkgk....kgkgk.",
    "..kkk......kkk..",
];

/// Warden: The Finals Week Golem, a stack of gold-and-black exam books with glaring red eyes.
pub const FINALS_GOLEM: [&str; 16] = [
    "....kkkkkkkk....",
    "...kggggggggk...",
    "...kgrkggkrgk...",
    "...kggggggggk...",
    "..kkkGGkkGGkkk..",
    ".kyyyyyyyyyyyyk.",
    ".kYYYYYYYYYYYYk.",
    "kgkwwwwwwwwwwkgk",
    "kgkkkkkkkkkkkkgk",
    "kgkyyyyyyyyyykgk",
    "kGkYYYYYYYYYYkGk",
    "kk.kwwwwwwwwk.kk",
    "...kkkkkkkkkk...",
    "...kggk..kggk...",
    "..kgggk..kgggk..",
    "..kkkkk..kkkkk..",
];

/// Warden: The Night Owl Queen, a crowned gold owl (a generic owl, not any real mascot).
pub const OWL_QUEEN: [&str; 16] = [
    "....y..y..y.....",
    "....yyyyyyy.....",
    "..k.kkkkkkk.k...",
    "..kykyyyyyykyk..",
    "..kyywwkywwyyk..",
    "..kywkwyywkwyk..",
    "..kyywwoowwyyk..",
    "kk.kyyyooyyyk.kk",
    "kyk.kyyyyyyk.kyk",
    "kyykyYyyyyYykyyk",
    "kyyyyYyddyYyyyyk",
    ".kyyyyYddYyyyyk.",
    "..kkkyyyyyykkk..",
    "....kkkkkkkk....",
    ".....ko..ok.....",
    "................",
];

/// Warden: The Library Stacks Lich, a skull in a black robe with gold trim clutching a padlock.
pub const LIBRARY_LICH: [&str; 16] = [
    ".....kkkkkk.....",
    "....kllllllk....",
    "...kllllllllk...",
    "...klkkllkklk...",
    "...klkyllkylk...",
    "....kllkkllk....",
    "....klwlwlwk....",
    "...kDkkkkkkDk...",
    "..kDDDkyykDDDk..",
    "..kDykyYYykyDk..",
    ".kDDykykkykyDDk.",
    ".kDDykyyyykyDDk.",
    ".kDDDDkkkkDDDDk.",
    "..kDyyyyyyyyDk..",
    "..kDkDkDDkDkDk..",
    "..k.k.k..k.k.k..",
];

/// Monster: Mystery Mac Slime, the dining hall special (gold, steaming, on a tray).
pub const MYSTERY_MEAT: [&str; 16] = [
    "................",
    "................",
    "................",
    "......w..w......",
    ".......w..w.....",
    "....kkkkkkkk....",
    "..kkyoyyyyoykk..",
    ".kyyyyyyoyyyyyk.",
    ".kyykwyyyykwyyk.",
    ".kyokkyyyykkyok.",
    ".kyyyyykkyyyyyk.",
    ".kYyyoyyyyyoyYk.",
    "kkkkkkkkkkkkkkkk",
    "kgggggggggggggGk",
    ".kkkkkkkkkkkkkk.",
    "................",
];

/// Monster: Spam Owl, a gold owl clutching an unread envelope (replaces the Spam Bat).
pub const SPAM_OWL: [&str; 16] = [
    "................",
    "...k........k...",
    "..kyk......kyk..",
    "..kyykkkkkkyyk..",
    "..kywwkyykwwyk..",
    "..kwkwkyykwkwk..",
    "kk.kwwkookwwk.kk",
    "kyk.kyyyyyyk.kyk",
    "kyykyYyyyyYykyyk",
    "kyyyyYwwwwYyyyyk",
    ".kyyyywkkwyyyyk.",
    "..kkkkwwwwkkkk..",
    "......kkkk......",
    ".....ko..ok.....",
    "................",
    "................",
];

/// Monster: Night Owl, a charcoal all-nighter owl with gold eyes and a coffee-stained belly.
pub const NIGHT_OWL: [&str; 16] = [
    "................",
    "...k........k...",
    "..kdk......kdk..",
    "..kddkkkkkkddk..",
    "..kdyykddkyydk..",
    "..kykykddkykyk..",
    "kk.kyykookyyk.kk",
    "kdk.kddddddk.kdk",
    "kddkdDddddDdkddk",
    "kddddDdllldDdddk",
    ".kddddDlwlDdddk.",
    "..kkkkkllllkkkk.",
    "......kkkkk.....",
    ".....ky...yk....",
    "................",
    "................",
];

/// Monster: Finals Week Brute, a red-eyed student in a black-and-gold hoodie hauling notes.
pub const FINALS_BRUTE: [&str; 16] = [
    "................",
    ".....kkkkkk.....",
    "....kddddddk....",
    "...kddssssddk...",
    "...kdsrssrsdk...",
    "...kdssssssdk...",
    "..kkkdkSSkdkkk..",
    ".kddddyyyyddddk.",
    "kddkddyddyddkddk",
    "kddkdddyydddkddk",
    "kyykdddddddkwwwk",
    ".kk.kdddddddkwwk",
    "....kddkkddkkkk.",
    "....kddk.kddk...",
    "...kkkk...kkkk..",
    "................",
];

/// Monster: Parking Patrol, a gold-capped officer with a ticket book.
pub const PARKING_PATROL: [&str; 16] = [
    "................",
    ".....kkkkkk.....",
    "....kyyyyyyk....",
    "...kkkkkkkkkk...",
    ".....ksssskk....",
    ".....kskkskk....",
    ".....ksssskk....",
    "....kkkddkkk....",
    "...kdddydddk....",
    "...kdkdyddkkyyk.",
    "...ksk.dydkkyky.",
    "......kddk.kyyk.",
    ".....kdkkdk.....",
    ".....kdk.kdk....",
    "....kkkk.kkkk...",
    "................",
];

/// Monster: Dead-Zone Caster, a black hood with a gold Wi-Fi fan and a red no-signal dot.
pub const DEAD_ZONE_CASTER: [&str; 16] = [
    "......kkkk......",
    ".....kDDDDk.....",
    "....kDDDDDDk....",
    "...kDDkkkkDDk...",
    "...kDkcDDckDk...",
    "...kDkkkkkkDk...",
    "..kDDDDDDDDDDk..",
    "..kDyyyyyyyyDk..",
    ".kDDDDDDDDDDDDk.",
    ".kDDDyyyyyyDDDk.",
    ".kDDDDDDDDDDDDk.",
    "..kDDDDyyDDDDk..",
    "...kkDDrrDDkk...",
    ".....kkkkkk.....",
    "................",
    "................",
];

/// Monster: Late Shuttle, a little gold campus bus running behind schedule.
pub const LATE_SHUTTLE: [&str; 16] = [
    "................",
    "................",
    "................",
    ".kkkkkkkkkkkkkk.",
    "kyyyyyyyyyyyyyyk",
    "kykkkykkkykkkyxk",
    "kykLkykkLykkLyyk",
    "kykkkykkkykkkyyk",
    "kyyyyyyyyyyyyyyk",
    "kkkkkkkkkkkkkkkk",
    "kyyyyyyyyyyyyyrk",
    "kkkgggkkkkgggkkk",
    "...kgk....kgk...",
    "....k......k....",
    "................",
    "................",
];

/// Pickup: a cup of coffee (+1 Health).
pub const COFFEE: [&str; 16] = [
    "................",
    "......l..l......",
    ".....l..l.......",
    "......l..l......",
    "...kkkkkkkkk....",
    "...kwwwwwwwkkk..",
    "...kyyyyyyykwk..",
    "...kwwwwwwwk.k..",
    "...kwNNNNNwkkk..",
    "...kwNNNNNwk....",
    "...kwwwwwwwk....",
    "....kwwwwwk.....",
    ".....kkkkk......",
    "................",
    "................",
    "................",
];

/// Hoot, the owl companion: a small gold study-buddy owl in round black glasses.
pub const HOOT: [&str; 16] = [
    "................",
    "..kk........kk..",
    "..kyk......kyk..",
    "..kyykkkkkkyyk..",
    ".kyyyyyyyyyyyyk.",
    ".kykkkkyykkkkyk.",
    ".kkwwwkkkkwwwkk.",
    ".kkwkwkyykwkwkk.",
    ".kykkkkookkkkyk.",
    ".kyyyyYooYyyyyk.",
    "kyyYwwwwwwwwYyyk",
    "kyyYwYwwwwYwYyyk",
    ".kyYwwwwwwwwYyk.",
    "..kkYwwwwwwYkk..",
    "....kokkkkok....",
    "................",
];

/// Prop: a buzzing phone with a red notification (the live scam generator).
pub const PHONE: [&str; 16] = [
    "................",
    "....kkkkkkk.....",
    "...kGGGGGGGk.kk.",
    "...kGkkkkkGkkrrk",
    "...kGkyyykGkrwrk",
    "...kGkkkkkGkkrrk",
    "...kGkyyykGk.kk.",
    "...kGkkkkkGk....",
    "...kGkyykkGk....",
    "...kGkkkkkGk....",
    "...kGkrrrkGk....",
    "...kGkkkkkGk....",
    "...kGGGGGGGk....",
    "...kGGGyGGGk....",
    "....kkkkkkk.....",
    "................",
];

/// Prop: a notice board with two pinned messages (spot-the-fake).
pub const NOTICE_BOARD: [&str; 16] = [
    "................",
    "NNNNNNNNNNNNNNNN",
    "NnnnnnnnnnnnnnnN",
    "NnwwwwwnnlllllnN",
    "NnwkkkwnnlkkklnN",
    "NnwwwwwnnlllllnN",
    "NnwkkwwnnlkrklnN",
    "NnwwwwwnnlllllnN",
    "NnwkkkwnnlrrrlnN",
    "NnwwwwwnnlllllnN",
    "NnwyywwnnlllllnN",
    "NnnnnnnnnnnnnnnN",
    "NNNNNNNNNNNNNNNN",
    ".N............N.",
    ".N............N.",
    "................",
];

/// Raster a dungeon floor map (one cell per tile, `tile` px each) into a
/// texture once per floor: flagstones, brick wall faces where walls meet floor,
/// solid dark rock elsewhere. `cell(x, y)` returns 0 = rock, 1 = floor.
pub fn dungeon_texture(w: i32, h: i32, tile: u16, seed: u32, cell: impl Fn(i32, i32) -> u8) -> Texture2D {
    let t = tile as usize;
    let (pw, ph) = (w as usize * t, h as usize * t);
    let mut img = Image::gen_image_color(pw as u16, ph as u16, Color::from_rgba(10, 8, 14, 255));
    let stride = pw * 4;
    let mut fill = |x: usize, y: usize, rw: usize, rh: usize, col: [u8; 4]| {
        for yy in y..(y + rh).min(ph) {
            for xx in x..(x + rw).min(pw) {
                img.bytes[yy * stride + xx * 4..][..4].copy_from_slice(&col);
            }
        }
    };
    let floor_at = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && cell(x, y) == 1;
    for ty in 0..h {
        for tx in 0..w {
            let (x0, y0) = (tx as usize * t, ty as usize * t);
            let hsh = hash(seed, tx * 977 + ty * 131);
            if floor_at(tx, ty) {
                let v = 34 + (hsh % 10) as u8;
                fill(x0, y0, t, t, [18, 14, 20, 255]);
                fill(x0 + 1, y0 + 1, t - 2, t - 2, [v + 4, v, v.saturating_sub(2), 255]);
                if hsh.is_multiple_of(11) {
                    for i in 0..=t / 2 {
                        fill(x0 + t / 5 + i * 3 / 5, y0 + t / 4 + i * 4 / 5, 1, 1, [22, 18, 20, 255]);
                    }
                }
                continue;
            }
            // wall: brick face if it touches floor, dark rock otherwise
            let near = (-1..=1).any(|dy| (-1..=1).any(|dx| floor_at(tx + dx, ty + dy)));
            if !near {
                continue;
            }
            fill(x0, y0, t, t, [24, 18, 28, 255]);
            let rows = 3;
            let bh = t / rows;
            for r in 0..rows {
                let off = if (r + ty as usize).is_multiple_of(2) { 0 } else { t / 2 };
                let mut bx = 0i32 - off as i32;
                while bx < t as i32 {
                    let hb = hash(seed ^ 0x5bd1, (tx * 31 + bx) ^ ((ty * 7 + r as i32) * 7919));
                    let v = 46 + (hb % 14) as u8;
                    let x = (x0 as i32 + bx + 1).max(x0 as i32) as usize;
                    let right = (x0 as i32 + bx + t as i32 / 2 - 1).min((x0 + t) as i32) as usize;
                    if right > x {
                        fill(x, y0 + r * bh + 1, right - x, bh - 2, [v, v - 6, v + 6, 255]);
                    }
                    bx += t as i32 / 2;
                }
            }
            // a darker lip where the wall's face meets the floor below it
            if floor_at(tx, ty + 1) {
                fill(x0, y0 + t - 5, t, 5, [30, 22, 30, 255]);
            }
        }
    }
    texture(&img)
}

/// Battle box: the tiny gold owl the player steers.
pub const BOX_OWL: [&str; 16] = [
    "................",
    "...k........k...",
    "..kYk......kYk..",
    "..kyykkkkkkyyk..",
    ".kyyyyyyyyyyyyk.",
    ".kywwkyyyykwwyk.",
    ".kwkkwyyyywkkwk.",
    ".kwkkwyooywkkwk.",
    ".kywwkyooykwwyk.",
    ".kyyyyyyyyyyyyk.",
    ".kyYyYwwwwYyYyk.",
    "..kyyywkkwyyyk..",
    "..kYyywwwwyyYk..",
    "...kkkkkkkkkk...",
    "....ko....ok....",
    "................",
];

/// Battle box hazard: a falling envelope with a gold seal.
pub const ENVELOPE: [&str; 16] = [
    "................",
    "................",
    "................",
    "kkkkkkkkkkkkkkkk",
    "kwkllllllllllkwk",
    "kllkllllllllkllk",
    "kllLkllllllkLllk",
    "klllLkllllkLlllk",
    "kllllLkyykLllllk",
    "klllllkyykllllLk",
    "kllllllkklllllLk",
    "kLLLLLLLLLLLLLLk",
    "kkkkkkkkkkkkkkkk",
    "................",
    "................",
    "................",
];

/// Battle box hazard: a gold @ sign.
pub const AT_SIGN: [&str; 16] = [
    "................",
    ".....kkkkkk.....",
    "...kkyyyyyykk...",
    "..kyyykkkkyyyk..",
    ".kyykk....kkyyk.",
    ".kyk..kkkk..kyk.",
    "kyyk.kyyyyk.kyyk",
    "kyk.kyykkyyk.kyk",
    "kyk.kyk..kyk.kyk",
    "kyk.kyykkyyykyyk",
    "kyyk.kyyyykyyyk.",
    ".kyyk.kkkk.kkk..",
    ".kyyykk....kk...",
    "..kkyyyyyyyyk...",
    "....kkkkkkkk....",
    "................",
];

/// Sprite for an encounter id (unknown ids get a generic hooded figure).
pub fn character_sprite(id: &str) -> &'static [&'static str; 16] {
    match id {
        "inbox_mimic" => &MIMIC,
        "deadline_wraith" => &WRAITH,
        "password_hydra" => &HYDRA,
        "internship_impostor" => &IMPOSTOR,
        "ticket_troll" => &TICKET_TROLL,
        "commute_chimera" => &COMMUTE_CHIMERA,
        "gift_card_gargoyle" => &GIFT_CARD_GARGOYLE,
        "push_poltergeist" => &PUSH_POLTERGEIST,
        "overpay_ogre" => &OVERPAY_OGRE,
        "cram_lich" => &CRAM_LICH,
        _ => &HOODED,
    }
}

// ---------------------------------------------------------------- texture caches
//
// The art is still generated from text grids at runtime, but each grid is now
// rasterized once into a small nearest-filtered texture instead of being
// re-drawn as hundreds of rectangles every frame.

/// Textures cached by a small key, built on first use.
type TexCache<K> = RefCell<Vec<(K, Texture2D)>>;
/// Wrapped lines cached by (text, width) hash.
type WrapCache = RefCell<HashMap<u64, (String, Rc<[String]>)>>;

thread_local! {
    static SPRITES: TexCache<usize> = const { RefCell::new(Vec::new()) };
    static BACKDROPS: TexCache<(u32, u32, u32)> = const { RefCell::new(Vec::new()) };
    static VIGNETTES: TexCache<(u32, u32)> = const { RefCell::new(Vec::new()) };
    static HEARTS: RefCell<Option<[Texture2D; 2]>> = const { RefCell::new(None) };
    static WRAPS: WrapCache = RefCell::new(HashMap::new());
}

pub fn texture(img: &Image) -> Texture2D {
    let t = Texture2D::from_image(img);
    t.set_filter(FilterMode::Nearest);
    t
}

fn sprite_texture(grid: &'static [&'static str; 16]) -> Texture2D {
    let key = grid as *const _ as usize;
    SPRITES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, t)) = c.iter().find(|(k, _)| *k == key) {
            return t.clone();
        }
        let mut img = Image::gen_image_color(16, 16, Color::new(0.0, 0.0, 0.0, 0.0));
        for (r, row) in grid.iter().enumerate() {
            for (cx, ch) in row.chars().enumerate().take(16) {
                if let Some(col) = pal(ch) {
                    img.set_pixel(cx as u32, r as u32, col);
                }
            }
        }
        let t = texture(&img);
        c.push((key, t.clone()));
        t
    })
}

/// Draw a 16x16 grid sprite with its top-left at (x, y), `px` pixels per cell.
pub fn draw_sprite(grid: &'static [&'static str; 16], x: f32, y: f32, px: f32, flip: bool) {
    draw_sprite_tinted(grid, x, y, px, flip, WHITE);
}

/// `draw_sprite` with a colour multiplier (hit flashes, fading wardens).
pub fn draw_sprite_tinted(grid: &'static [&'static str; 16], x: f32, y: f32, px: f32, flip: bool, tint: Color) {
    let tex = sprite_texture(grid);
    let size = 16.0 * px.ceil();
    draw_texture_ex(
        &tex,
        x.floor(),
        y.floor(),
        tint,
        DrawTextureParams { dest_size: Some(vec2(size, size)), flip_x: flip, ..Default::default() },
    );
}

/// Draw a sprite centred on (cx, cy).
pub fn draw_sprite_centered(grid: &'static [&'static str; 16], cx: f32, cy: f32, px: f32, flip: bool, tint: Color) {
    let half = 8.0 * px.ceil();
    draw_sprite_tinted(grid, cx - half, cy - half, px, flip, tint);
}

/// Soft light: stacked translucent circles.
pub fn glow(x: f32, y: f32, r: f32, col: Color, strength: f32) {
    for i in 0..6 {
        let f = 1.0 - i as f32 / 6.0;
        draw_circle(x, y, r * f, Color::new(col.r, col.g, col.b, 0.035 * strength));
    }
}

/// A wall candle with flickering flame and glow.
pub fn candle(x: f32, y: f32, t: f32, seed: f32) {
    let fl = 1.0 + 0.12 * (t * 9.0 + seed).sin() + 0.08 * (t * 23.0 + seed * 3.0).sin();
    glow(x, y - 6.0, 120.0 * fl, Color::new(1.0, 0.6, 0.2, 1.0), 1.4);
    draw_rectangle(x - 10.0, y + 10.0, 20.0, 6.0, Color::from_rgba(90, 70, 50, 255));
    draw_rectangle(x - 5.0, y - 4.0, 10.0, 16.0, PAPER);
    draw_rectangle(x - 3.0, y - 12.0 - 2.0 * fl, 6.0, 8.0 + 2.0 * fl, Color::new(1.0, 0.65, 0.15, 1.0));
    draw_rectangle(x - 1.5, y - 9.0 - fl, 3.0, 5.0, Color::new(1.0, 0.95, 0.6, 1.0));
}

/// Stone floor + wall backdrop for a room. `seed` varies the brick pattern per room.
/// Rasterized once per (seed, top, bottom) and then drawn as one texture.
pub fn crypt_backdrop(seed: u32, top: f32, bottom: f32) {
    let bg = Color::from_rgba(12, 9, 16, 255);
    clear_background(bg);
    let key = (seed, top.to_bits(), bottom.to_bits());
    let tex = BACKDROPS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, t)) = c.iter().find(|(k, _)| *k == key) {
            return t.clone();
        }
        if c.len() >= 8 {
            c.remove(0);
        }
        let t = texture(&backdrop_image(seed, top, bottom, bg));
        c.push((key, t.clone()));
        t
    });
    draw_texture(&tex, 0.0, top.floor(), WHITE);
}

/// CPU raster of the backdrop, pixel-for-pixel what the old per-frame
/// rectangles produced at 1:1 scale (the image's y=0 is `top`).
fn backdrop_image(seed: u32, top: f32, bottom: f32, bg: Color) -> Image {
    let (w, h) = (VW as u16, (bottom - top.floor()).ceil().max(1.0) as u16);
    let mut img = Image::gen_image_color(w, h, bg);
    let oy = top.floor();
    let stride = w as usize * 4;
    let mut fill = |x: f32, y: f32, rw: f32, rh: f32, col: Color| {
        let px: [u8; 4] = col.into();
        let (x0, y0) = ((x.max(0.0)) as usize, ((y - oy).max(0.0)) as usize);
        let (x1, y1) = (((x + rw).min(w as f32)).max(0.0) as usize, ((y + rh - oy).min(h as f32)).max(0.0) as usize);
        for yy in y0..y1 {
            for xx in x0..x1 {
                img.bytes[yy * stride + xx * 4..][..4].copy_from_slice(&px);
            }
        }
    };
    let tile = 40.0;
    // back wall (bricks)
    let wall_h = 120.0;
    for row in 0..(wall_h / 20.0) as i32 {
        let y = top + row as f32 * 20.0;
        let off = if row % 2 == 0 { 0.0 } else { 20.0 };
        let mut x = -off;
        while x < VW {
            let h = hash(seed, (x as i32) ^ (row * 7919));
            let v = 46 + (h % 14) as u8;
            fill(x + 1.0, y + 1.0, 38.0, 18.0, Color::from_rgba(v, v - 6, v + 6, 255));
            x += 40.0;
        }
    }
    fill(0.0, top + wall_h, VW, 6.0, Color::from_rgba(24, 18, 28, 255));
    // floor flagstones
    let mut y = top + wall_h + 6.0;
    let mut r = 0;
    while y < bottom {
        let mut x = 0.0;
        let mut c = 0;
        while x < VW {
            let h = hash(seed, r * 131 + c);
            let v = 34 + (h % 10) as u8;
            fill(x + 1.0, y + 1.0, tile - 2.0, tile - 2.0, Color::from_rgba(v + 4, v, v - 2, 255));
            if h.is_multiple_of(11) {
                // 1px crack from (x+8, y+10) to (x+22, y+26)
                for i in 0..=16 {
                    let f = i as f32 / 16.0;
                    fill((x + 8.0 + 14.0 * f).floor(), (y + 10.0 + 16.0 * f).floor(), 1.0, 1.0, Color::from_rgba(22, 18, 20, 255));
                }
            }
            x += tile;
            c += 1;
        }
        y += tile;
        r += 1;
    }
    img
}

/// Darken the edges of the play area. Baked into one texture whose alpha is
/// exactly what the 30 stacked 6% black rectangles used to composite to
/// (1 - 0.94^n), so it costs one quad instead of 30 near-fullscreen blends.
pub fn vignette(top: f32, bottom: f32) {
    let key = (top.to_bits(), bottom.to_bits());
    let tex = VIGNETTES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, t)) = c.iter().find(|(k, _)| *k == key) {
            return t.clone();
        }
        let t = texture(&vignette_image(top, bottom));
        c.push((key, t.clone()));
        t
    });
    draw_texture(&tex, 0.0, top.floor(), WHITE);
}

fn vignette_image(top: f32, bottom: f32) -> Image {
    let (w, h) = (VW as usize, (bottom - top).ceil().max(1.0) as usize);
    let mut n = vec![0u8; w * h];
    let mut add = |x: f32, y: f32, rw: f32, rh: f32| {
        let (x0, y0) = (x.max(0.0) as usize, (y - top).max(0.0) as usize);
        let (x1, y1) = (((x + rw).min(w as f32)) as usize, ((y + rh - top).min(h as f32)) as usize);
        for yy in y0..y1 {
            for xx in x0..x1 {
                n[yy * w + xx] += 1;
            }
        }
    };
    for i in 0..10 {
        let d = i as f32 * 16.0;
        add(0.0, top, 160.0 - d, bottom - top);
        add(VW - 160.0 + d, top, 160.0 - d, bottom - top);
        add(0.0, bottom - 100.0 + d * 0.6, VW, 100.0 - d * 0.6);
    }
    let mut img = Image::gen_image_color(w as u16, h as u16, Color::new(0.0, 0.0, 0.0, 0.0));
    for (i, &k) in n.iter().enumerate() {
        if k > 0 {
            img.set_pixel((i % w) as u32, (i / w) as u32, Color::new(0.0, 0.0, 0.0, 1.0 - 0.94f32.powi(k as i32)));
        }
    }
    img
}

pub fn hash(seed: u32, v: i32) -> u32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ (v as u32).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x
}

pub fn panel(x: f32, y: f32, w: f32, h: f32, border: Color) {
    draw_rectangle(x, y, w, h, PANEL);
    draw_rectangle_lines(x, y, w, h, 3.0, border);
    draw_rectangle_lines(x + 5.0, y + 5.0, w - 10.0, h - 10.0, 1.0, Color::new(border.r, border.g, border.b, 0.35));
}

/// Word-wrap `text` to lines no wider than `max_w` at `size`.
/// Memoized: text layout only changes when the text does, so steady-state
/// frames do no measuring and no allocation here.
pub fn wrap(text: &str, size: f32, max_w: f32) -> Rc<[String]> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    size.to_bits().hash(&mut h);
    max_w.to_bits().hash(&mut h);
    let key = h.finish();
    WRAPS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((t, lines)) = c.get(&key) {
            if t == text {
                return lines.clone();
            }
        }
        if c.len() >= 512 {
            c.clear(); // content is small; a full reset is simpler than LRU
        }
        let lines: Rc<[String]> = wrap_uncached(text, size, max_w).into();
        c.insert(key, (text.to_string(), lines.clone()));
        lines
    })
}

fn wrap_uncached(text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut trial = String::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            trial.clear();
            if !cur.is_empty() {
                trial.push_str(&cur);
                trial.push(' ');
            }
            trial.push_str(word);
            if measure_text(&trial, None, size as u16, 1.0).width > max_w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur.push_str(word);
            } else {
                std::mem::swap(&mut cur, &mut trial);
            }
            // one word wider than the box (a long link): hard-break it so it can't overflow
            while measure_text(&cur, None, size as u16, 1.0).width > max_w && cur.chars().count() > 1 {
                let mut cut = cur.len();
                while cut > 1 && measure_text(&cur[..cut], None, size as u16, 1.0).width > max_w {
                    cut = cur[..cut].char_indices().last().map_or(1, |(i, _)| i.max(1));
                }
                let rest = cur.split_off(cut);
                lines.push(std::mem::replace(&mut cur, rest));
            }
        }
        lines.push(cur);
    }
    lines
}

/// Height `text_block` advances for `text` (lines x line height).
pub fn wrapped_height(text: &str, size: f32, max_w: f32) -> f32 {
    wrap(text, size, max_w).len() as f32 * size * 1.15
}

/// Draw `items` as a stack of wrapped paragraphs, each item a list of
/// `(text, colour, indent)` parts. Picks the largest `(main, small)` size pair
/// (the first part of an item uses `main`, the rest `small`) whose stack fits
/// between `top` (first baseline) and `bottom` (lowest pixel), so nothing is ever
/// dropped or drawn past `bottom`. Returns the size pair used.
pub fn fit_sizes(items: &[Vec<(&str, f32)>], top: f32, bottom: f32, w: f32, gap: f32, sizes: &[(f32, f32)]) -> (f32, f32) {
    let fits = |&(big, small): &(f32, f32)| {
        let mut y = top;
        let mut last = big;
        for parts in items {
            for (i, (text, indent)) in parts.iter().enumerate() {
                let s = if i == 0 { big } else { small };
                y += wrapped_height(text, s, w - indent);
                last = s;
            }
            y += gap;
        }
        // y is one line height below the last baseline (plus the trailing gap); descenders reach ~0.3 size
        y - gap - last * 1.15 + last * 0.3 <= bottom
    };
    sizes.iter().copied().find(|p| fits(p)).unwrap_or(*sizes.last().unwrap_or(&(16.0, 16.0)))
}

/// Draw wrapped text; returns the y just below the last line.
pub fn text_block(text: &str, x: f32, y: f32, size: f32, max_w: f32, col: Color) -> f32 {
    text_typed(text, usize::MAX, x, y, size, max_w, col)
}

/// Typewriter version of `text_block`: lays out the *whole* text, then shows
/// only the first `chars` characters. (Wrapping the growing prefix instead made
/// words jump to the next line mid-type.)
pub fn text_typed(text: &str, chars: usize, x: f32, y: f32, size: f32, max_w: f32, col: Color) -> f32 {
    let lh = size * 1.15;
    let mut yy = y;
    let mut left = chars;
    for l in wrap(text, size, max_w).iter() {
        if left == 0 {
            break;
        }
        let n = l.chars().count();
        let s = if left >= n { l.as_str() } else { &l[..l.char_indices().nth(left).map_or(l.len(), |(i, _)| i)] };
        draw_text(s, x, yy, size, col);
        left = left.saturating_sub(n + 1); // +1: the space/newline the wrap consumed
        yy += lh;
    }
    yy
}

/// Every font size the game draws text at (virtual px; glyphs are rasterized at size * dpi).
const TEXT_SIZES: [u16; 14] = [16, 18, 20, 21, 22, 24, 25, 26, 27, 28, 30, 32, 34, 36];
/// Big title sizes only ever draw upper-case headings.
const TITLE_SIZES: [u16; 3] = [48, 64, 120];

/// Fill macroquad's shared font atlas with every glyph the game uses, up front.
///
/// When a new glyph doesn't fit, macroquad grows the atlas by deleting its GL texture
/// and creating a bigger one. If that happens mid-frame, draw calls already batched
/// against the old texture hit a deleted ID (browser console: "glBindTexture ...
/// already deleted texture"). Warming the cache before the first real frame means
/// the atlas is resized once, while nothing is queued.
pub fn prewarm_font_atlas() {
    let font = get_default_font();
    let dpi = miniquad::window::dpi_scale();
    let all: Vec<char> = (' '..='~').collect();
    let caps: Vec<char> = (' '..='`').collect(); // space, punctuation, digits, A-Z
    for (sizes, chars) in [(&TEXT_SIZES[..], &all), (&TITLE_SIZES[..], &caps)] {
        for &s in sizes {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            font.populate_font_cache(chars, (f32::from(s) * dpi).ceil() as u16);
        }
    }
}

pub fn centered(text: &str, cx: f32, y: f32, size: f32, col: Color) {
    let w = measure_text(text, None, size as u16, 1.0).width;
    draw_text(text, (cx - w * 0.5).round(), y, size, col);
}

pub fn shadow_centered(text: &str, cx: f32, y: f32, size: f32, col: Color) {
    centered(text, cx + 3.0, y + 3.0, size, INK);
    centered(text, cx, y, size, col);
}

/// Small pixel heart.
pub fn heart(x: f32, y: f32, px: f32, full: bool) {
    let tex = HEARTS.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| {
                const H: [&str; 6] = [".kk.kk.", "krrkrrk", "krrrrrk", ".krrrk.", "..krk..", "...k..."];
                let mk = |full: bool| {
                    let mut img = Image::gen_image_color(7, 6, Color::new(0.0, 0.0, 0.0, 0.0));
                    for (r, row) in H.iter().enumerate() {
                        for (c, ch) in row.chars().enumerate() {
                            let col = match ch {
                                'k' => INK,
                                'r' if full => RISKY,
                                'r' => Color::from_rgba(70, 40, 50, 255),
                                _ => continue,
                            };
                            img.set_pixel(c as u32, r as u32, col);
                        }
                    }
                    texture(&img)
                };
                [mk(false), mk(true)]
            })[full as usize]
            .clone()
    });
    draw_texture_ex(&tex, x, y, WHITE, DrawTextureParams { dest_size: Some(vec2(7.0 * px, 6.0 * px)), ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sprite_is_16x16_with_known_colours() {
        let all: [&[&str; 16]; 30] = [
            &STUDENT, &MIMIC, &WRAITH, &HYDRA, &IMPOSTOR, &HOODED, &KEEPER, &TOW_TYRANT, &FINALS_GOLEM, &OWL_QUEEN,
            &LIBRARY_LICH, &MYSTERY_MEAT, &SPAM_OWL, &NIGHT_OWL, &FINALS_BRUTE, &PARKING_PATROL, &DEAD_ZONE_CASTER,
            &LATE_SHUTTLE, &TICKET_TROLL, &COMMUTE_CHIMERA, &GIFT_CARD_GARGOYLE, &PUSH_POLTERGEIST, &OVERPAY_OGRE, &CRAM_LICH,
            &HOOT, &PHONE, &NOTICE_BOARD, &BOX_OWL, &ENVELOPE, &AT_SIGN,
        ];
        for (i, g) in all.iter().enumerate() {
            for (r, row) in g.iter().enumerate() {
                assert_eq!(row.chars().count(), 16, "sprite {i} row {r}: {row:?}");
                for c in row.chars() {
                    assert!(c == '.' || pal(c).is_some(), "sprite {i} row {r}: unknown colour {c:?}");
                }
            }
        }
        for row in COFFEE {
            assert_eq!(row.len(), 16);
        }
    }

    /// Every shipped encounter has its own sprite (none fall back to the hooded figure).
    #[test]
    fn every_shipped_encounter_has_its_own_sprite() {
        let (c, _) = crate::content::parse(crate::content::EMBEDDED_ENCOUNTERS).unwrap();
        let mut seen = Vec::new();
        for e in &c.encounters {
            let g = character_sprite(&e.id);
            assert!(!std::ptr::eq(g, &HOODED), "{} uses the generic hooded sprite", e.id);
            assert!(!seen.iter().any(|s| std::ptr::eq(*s, g)), "{} shares a sprite", e.id);
            seen.push(g);
        }
        assert!(std::ptr::eq(character_sprite("someone_new"), &HOODED));
    }
}
