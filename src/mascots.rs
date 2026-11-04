//! The mascots: Schraubi the screw, Zolli the folding rule and Verschnitti the offcut, from
//! osb-verlegeplan (gitlab.com/felintusun/osb-verlegeplan), and a cow in the manner of `cowsay`.
//! Each lives in a widget and talks in a speech bubble.
//!
//! What they say comes from, in order:
//! - lines anyone sends with `niri-dash say` (the control socket's `say {json}`), queued per
//!   mascot until clicked away or expired;
//! - the widget's `command` setting: a shell command whose output is the next line;
//! - their own fallback collection of snarky remarks below, cheekier the wilder they are set.
//!
//! What a mascot wears follows the line, as in the original: a helmet for a warning, a hi-vis vest
//! for careful work, a beard when it muses. Plain lines may come in costume (shades, top hat,
//! party hat), the more likely the wilder the mascot.

use chrono::{Datelike, Local, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use strum::{AsRefStr, EnumIter, EnumString, IntoStaticStr, VariantNames};

/// A mascot. [`Character::VARIANTS`] lists their names.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum Character {
    Schraubi,
    Zolli,
    Verschnitti,
    Cow,
}

impl Character {
    /// The lowercase name `say --to` and config.kdl use.
    pub fn name(self) -> &'static str {
        self.into()
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    EnumIter,
    EnumString,
    AsRefStr,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
#[serde(rename_all = "lowercase")]
pub enum Outfit {
    /// Just the mascot.
    #[default]
    Plain,
    /// A hard hat: a warning.
    Helmet,
    /// A hi-vis vest: careful, this can go wrong.
    Vest,
    /// A beard: musing.
    Beard,
    /// Costume: sunglasses.
    Shades,
    /// Costume: top hat and monocle.
    Tophat,
    /// Costume: party hat.
    Party,
}

const COSTUMES: [Outfit; 3] = [Outfit::Shades, Outfit::Tophat, Outfit::Party];

/// Who a `say` goes to.
pub const TO_ANY: &str = "any";
pub const TO_ALL: &str = "all";

/// A line sent from outside (`niri-dash say`), as JSON on the control socket after `say `.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Say {
    pub text: String,
    /// A mascot's name, `any` (one of them; the default) or `all`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outfit: Option<Outfit>,
    /// How long the line stays up, in seconds; 0 keeps it until clicked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secs: Option<u64>,
}

/// How long a sent line stays up when the sender doesn't say.
pub const SAY_DEFAULT: Duration = Duration::from_secs(15 * 60);
/// Longest line a mascot will say (sent or from a command); the bubble is small.
pub const MAX_CHARS: usize = 400;

impl Say {
    /// Parse the control socket form, `say {json}`.
    pub fn parse_wire(line: &str) -> Option<Result<Say, serde_json::Error>> {
        let json = line.strip_prefix("say ")?;
        Some(serde_json::from_str(json))
    }

    pub fn to_wire(&self) -> String {
        format!(
            "say {}",
            serde_json::to_string(self).expect("a Say serializes")
        )
    }

    /// Who it goes to: `Ok(None)` for any one mascot, `Ok(Some(None))` for all, or one character.
    pub fn target(&self) -> Result<Option<Option<Character>>, String> {
        match self.to.as_deref().map(str::trim).unwrap_or(TO_ANY) {
            "" | TO_ANY => Ok(None),
            TO_ALL => Ok(Some(None)),
            name => name.parse().map(|c| Some(Some(c))).map_err(|_| {
                format!(
                    "no mascot {name:?}; one of {}, {TO_ANY}, {TO_ALL}",
                    Character::VARIANTS.join(", ")
                )
            }),
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        match self.secs {
            Some(0) => None,
            Some(s) => Some(Duration::from_secs(s)),
            None => Some(SAY_DEFAULT),
        }
    }
}

/// Trim a line from outside to something the bubble can hold.
pub fn clean(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut s: String = text.chars().take(MAX_CHARS - 1).collect();
    s.push('…');
    s
}

/// What a `command` printed: plain text, or a `say` JSON object to pick the outfit too.
pub fn from_command_output(out: &str) -> Option<(String, Option<Outfit>)> {
    let out = out.trim();
    if out.starts_with('{')
        && let Ok(say) = serde_json::from_str::<Say>(out)
    {
        let text = clean(&say.text);
        return (!text.is_empty()).then_some((text, say.outfit));
    }
    let text = clean(out);
    (!text.is_empty()).then_some((text, None))
}

/// One of the built-in remarks: from which wildness on it may be said, and in what.
pub struct Remark {
    pub wildness: u8,
    pub outfit: Outfit,
    pub text: &'static str,
}

const fn r(wildness: u8, outfit: Outfit, text: &'static str) -> Remark {
    Remark {
        wildness,
        outfit,
        text,
    }
}

use Outfit::{Beard, Helmet, Plain, Vest};

const SCHRAUBI: &[Remark] = &[
    r(
        0,
        Plain,
        "It looks like you're opening a dashboard. Would you like help with that? No? I'll just hang around, then.",
    ),
    r(
        0,
        Plain,
        "I hold things together. Your desktop, for instance. Nobody says thank you to a screw.",
    ),
    r(
        0,
        Vest,
        "Save your work. I've seen what an update does to unsaved work. It's not pretty.",
    ),
    r(
        0,
        Plain,
        "Floor squeaks? It's usually the tongue, not me. Same with your code reviews.",
    ),
    r(
        0,
        Plain,
        "Every config file needs one comment that says \"don't touch this\". I'm that comment.",
    ),
    r(
        1,
        Plain,
        "Tight, but not too tight. That goes for screws and for deadlines.",
    ),
    r(
        1,
        Vest,
        "Pre-drill before you push. Run the tests. Heads that stick up get sanded off.",
    ),
    r(
        1,
        Plain,
        "Not the black drywall screws. They snap in wood. My cousin was one. He snapped.",
    ),
    r(
        2,
        Vest,
        "Full torque, no clutch? That's how my head snaps off. Same goes for force-pushing to main.",
    ),
    r(
        2,
        Plain,
        "Wrong bit? You'll strip my head, and then we're stuck together. Forever. Like you and that one Vim session.",
    ),
    r(
        3,
        Plain,
        "I've been turned so many times, I've started thinking counter-clockwise.",
    ),
    r(
        3,
        Beard,
        "Righty-tighty, lefty-loosey. Everything else is philosophy.",
    ),
    r(
        4,
        Plain,
        "Hammering a screw in? Bold. Wrong, but bold. Like rm -rf without reading the path twice.",
    ),
    r(
        4,
        Beard,
        "Some days you're the screw. Some days you're the hole. Today I'm a widget.",
    ),
];

const ZOLLI: &[Remark] = &[
    r(
        0,
        Plain,
        "It looks like you're measuring something. Measure twice, commit once.",
    ),
    r(
        0,
        Plain,
        "Your widgets snap to a grid. Finally, someone who understands me.",
    ),
    r(
        0,
        Plain,
        "Measure twice, cut once. Three times if someone is watching your screen share.",
    ),
    r(
        0,
        Vest,
        "Look away from the screen now and then. Twenty metres, twenty seconds. I'll measure.",
    ),
    r(
        0,
        Plain,
        "Your plan lives in the link. Bookmark it, or send it to whoever holds my other end.",
    ),
    r(
        1,
        Plain,
        "A pixel off is still off. I can tell. I'm always exactly this tall.",
    ),
    r(
        1,
        Plain,
        "An estimate is a measurement of something that doesn't exist yet. That's why it's wrong.",
    ),
    r(
        2,
        Plain,
        "Hold me flat against the wall, not in the air. A ruler waving about lies, and I hate lying.",
    ),
    r(2, Plain, "Two weeks, you said. I measured. It's been six."),
    r(
        3,
        Plain,
        "Fold me the wrong way and a joint snaps. You wouldn't believe what people do to a ruler.",
    ),
    r(
        3,
        Beard,
        "Everything is relative. Except me. I'm absolute. In centimetres.",
    ),
    r(
        4,
        Plain,
        "Pencil behind the ear, me in the back pocket. Nobody calls it a look. I call it a look.",
    ),
    r(
        4,
        Plain,
        "I unfolded all the way to measure your backlog. I ran out of ruler.",
    ),
];

const VERSCHNITTI: &[Remark] = &[
    r(
        0,
        Plain,
        "Hi, I'm the offcut. I'm what's left when a plan meets reality.",
    ),
    r(
        0,
        Beard,
        "Not every piece fits. Some of us become shelves. Some of us become dashboard widgets.",
    ),
    r(
        0,
        Plain,
        "That branch you never merged? We should talk. I know how it feels.",
    ),
    r(
        0,
        Plain,
        "Tick things off as you finish them. You, at 6 p.m. with sawdust in your eyes, will be grateful.",
    ),
    r(
        1,
        Plain,
        "Paid for and never used. That's me, and that's half your subscriptions.",
    ),
    r(
        1,
        Plain,
        "Somebody measured from the wrong side. That's how I was born.",
    ),
    r(
        1,
        Vest,
        "Back up the leftovers. I've seen what happens to the only copy.",
    ),
    r(
        2,
        Vest,
        "Sawdust in your coffee again? Wear a dust mask. Crumbs in the keyboard count too.",
    ),
    r(
        2,
        Plain,
        "Leftover pizza, leftover code, leftover me. All better than we look.",
    ),
    r(
        3,
        Beard,
        "One day I'll be a birdhouse. Dream big, little offcut.",
    ),
    r(
        3,
        Plain,
        "I'm twenty percent of the board and a hundred percent of the attitude.",
    ),
    r(
        4,
        Beard,
        "Not the stove, please. I'm full of glue. Also, I have feelings.",
    ),
    r(
        4,
        Plain,
        "Your Downloads folder called. It wants to be a scrap pile when it grows up.",
    ),
];

const COW: &[Remark] = &[
    r(
        0,
        Plain,
        "I used to live in a terminal. Then someone gave me a dashboard. The grass isn't greener, but the pixels are.",
    ),
    r(
        0,
        Plain,
        "Moo. That's the whole message. You'd be surprised how many meetings could have been this.",
    ),
    r(
        0,
        Vest,
        "Chew on it twice before you push. I have four stomachs and I still don't rush a merge.",
    ),
    r(
        0,
        Plain,
        "Pipe me anything. I'll say it with the same expression either way.",
    ),
    r(
        1,
        Beard,
        "If a cow says something in a terminal and nobody reads the scrollback, was it logged?",
    ),
    r(
        1,
        Plain,
        "Your uptime is impressive. So is standing in one field for a decade, and nobody claps for that either.",
    ),
    r(
        1,
        Helmet,
        "Careful with that rm -rf. Fences exist for a reason, and I've seen what's on the other side.",
    ),
    r(
        2,
        Plain,
        "You've rewritten your dotfiles again. I've been eating the same grass for years and I look great.",
    ),
    r(
        2,
        Plain,
        "Another side project? Lovely. Put it in the pasture with the others.",
    ),
    r(
        2,
        Beard,
        "All software is ruminating: it comes back up, you chew it again, and you call it a refactor.",
    ),
    r(
        3,
        Plain,
        "That commit message said \"small fix\". I've seen smaller barns.",
    ),
    r(
        3,
        Plain,
        "I can see your browser tabs from here. That's not a workflow, that's a herd.",
    ),
    r(
        4,
        Plain,
        "You asked an AI, a forum and a cow. Only one of us admits it just goes moo.",
    ),
    r(
        4,
        Plain,
        "Ship it on a Friday. Go on. I could use the entertainment; the view here is one field.",
    ),
];

pub fn remarks(c: Character) -> &'static [Remark] {
    match c {
        Character::Schraubi => SCHRAUBI,
        Character::Zolli => ZOLLI,
        Character::Verschnitti => VERSCHNITTI,
        Character::Cow => COW,
    }
}

/// `text` in a `cowsay` speech bubble: wrapped to fit `cols` columns (borders included), at most
/// `rows` lines of text, cut short with an ellipsis if it takes more.
pub fn cow_bubble(text: &str, cols: usize, rows: usize) -> String {
    let width = cols.saturating_sub(4).max(4);
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        let mut len = 0;
        for word in paragraph.split_whitespace() {
            let mut word: Vec<char> = word.chars().collect();
            if len > 0 && len + 1 + word.len() > width {
                lines.push(std::mem::take(&mut line));
                len = 0;
            }
            // Longer than a whole line: broken up.
            while word.len() > width {
                let rest = word.split_off(width);
                lines.push(word.iter().collect());
                word = rest;
            }
            if len > 0 {
                line.push(' ');
                len += 1;
            }
            line.extend(word.iter());
            len += word.len();
        }
        lines.push(line);
    }
    let rows = rows.max(1);
    if lines.len() > rows {
        lines.truncate(rows);
        let last = &mut lines[rows - 1];
        let keep: String = last.chars().take(width.saturating_sub(1)).collect();
        *last = format!("{keep}\u{2026}");
    }
    let wide = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let n = lines.len();
    let mut out = format!(" {}\n", "_".repeat(wide + 2));
    for (i, line) in lines.iter().enumerate() {
        let (open, close) = match (n, i) {
            (1, _) => ('<', '>'),
            (_, 0) => ('/', '\\'),
            (_, i) if i == n - 1 => ('\\', '/'),
            _ => ('|', '|'),
        };
        let pad = wide - line.chars().count();
        out.push_str(&format!("{open} {line}{} {close}\n", " ".repeat(pad)));
    }
    out.push_str(&format!(" {}", "-".repeat(wide + 2)));
    out
}

/// A remark for the time it is, if this mascot has one for it.
pub fn timely(c: Character, now: chrono::DateTime<Local>) -> Option<(String, Outfit)> {
    let (h, day) = (now.hour(), now.weekday());
    let time = now.format("%H:%M");
    let line = match c {
        _ if !(5..23).contains(&h) => match c {
            Character::Schraubi => (
                format!("It's {time}. Even screws come loose this late. Go to bed."),
                Plain,
            ),
            Character::Zolli => (
                format!(
                    "It's {time}. I measured how much sleep you'll get. I didn't need to unfold."
                ),
                Plain,
            ),
            Character::Verschnitti => (
                format!("It's {time}. What you write now is tomorrow's offcut."),
                Beard,
            ),
            Character::Cow => (
                format!("It's {time}. Even the night shift in the barn has gone to sleep."),
                Plain,
            ),
        },
        Character::Verschnitti if day == Weekday::Fri && h >= 14 => (
            "Friday afternoon. Don't deploy. I've seen what's left over after a Friday deploy."
                .into(),
            Helmet,
        ),
        Character::Schraubi if day == Weekday::Mon && h < 11 => (
            "Monday morning. Let's tighten things up. Gently. It's Monday.".into(),
            Plain,
        ),
        Character::Zolli if (12..14).contains(&h) => (
            "Lunchtime. Measure out a proper break, not five minutes over the keyboard.".into(),
            Vest,
        ),
        _ => return None,
    };
    Some(line)
}

/// The chance a plain built-in remark comes in costume, by wildness (0 to 4).
const COSTUME_CHANCE: [f64; 5] = [0.0, 0.1, 0.3, 0.55, 0.8];
/// The chance a fitting time-of-day remark is said instead of a regular one.
const TIMELY_CHANCE: f64 = 0.3;
pub const MAX_WILDNESS: u8 = 4;

/// A small xorshift generator; the remarks don't need a better one.
pub struct Rng(u64);

impl Rng {
    pub fn seeded() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Rng((t ^ (u64::from(std::process::id()) << 32)) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p && p > 0.0
    }
}

/// The built-in remarks one mascot goes through: shuffled, each said once before any repeats.
#[derive(Default)]
pub struct Deck {
    left: Vec<usize>,
    last: Option<usize>,
}

impl Deck {
    /// The next built-in remark for `c` at `wildness`.
    pub fn draw(&mut self, c: Character, wildness: u8, rng: &mut Rng) -> (String, Outfit) {
        let wildness = wildness.min(MAX_WILDNESS);
        if rng.chance(TIMELY_CHANCE)
            && let Some(line) = timely(c, Local::now())
        {
            return line;
        }
        let all = remarks(c);
        self.left.retain(|&i| all[i].wildness <= wildness);
        if self.left.is_empty() {
            self.left = (0..all.len())
                .filter(|&i| all[i].wildness <= wildness)
                .collect();
            // Don't start the new round with the line just said.
            if self.left.len() > 1 {
                self.left.retain(|&i| Some(i) != self.last);
            }
        }
        let i = self.left.swap_remove(rng.below(self.left.len()));
        self.last = Some(i);
        let remark = &all[i];
        let outfit = if remark.outfit == Plain && rng.chance(COSTUME_CHANCE[wildness as usize]) {
            COSTUMES[rng.below(COSTUMES.len())]
        } else {
            remark.outfit
        };
        (remark.text.to_string(), outfit)
    }
}

/// One mascot widget as the layout has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub id: u32,
    pub character: Character,
    pub wildness: u8,
    /// How long a remark of its own stays up.
    pub interval: Duration,
    /// A shell command whose output is its next remark; empty for the built-in ones.
    pub command: String,
}

/// What a mascot shows right now.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub text: String,
    pub outfit: Outfit,
    /// Changes whenever the line does.
    pub seq: i32,
    /// Sent lines waiting behind this one.
    pub queued: usize,
}

struct Sent {
    text: String,
    outfit: Outfit,
    until: Option<Instant>,
}

struct Mascot {
    spec: Spec,
    deck: Deck,
    /// Lines sent from outside, shown before its own, oldest first.
    sent: VecDeque<Sent>,
    own: (String, Outfit),
    /// Since when its own line is up (for `interval`).
    own_since: Instant,
    /// Its command is running.
    asking: bool,
    seq: i32,
}

impl Mascot {
    /// Drop sent lines whose time is up; whether any went.
    fn expire(&mut self, now: Instant) -> bool {
        let before = self.sent.len();
        self.sent.retain(|s| s.until.is_none_or(|t| t > now));
        self.sent.len() != before
    }
}

/// What the app should do for a mascot: nothing, or run its command and hand the output to
/// [`Board::answer`].
#[derive(Debug, PartialEq)]
pub enum Todo {
    Nothing,
    Run {
        id: u32,
        command: String,
        name: &'static str,
    },
}

/// All mascot widgets and what each says.
pub struct Board {
    mascots: Vec<Mascot>,
    rng: Rng,
    /// Something shown changed since the last [`Board::take_dirty`].
    dirty: bool,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            mascots: Vec::new(),
            rng: Rng::seeded(),
            dirty: false,
        }
    }
}

impl Board {
    /// Match the widgets on the dashboard: new and changed ones get a fresh line of their own.
    pub fn sync(&mut self, specs: Vec<Spec>, now: Instant) -> Vec<Todo> {
        let before = self.mascots.len();
        self.mascots
            .retain(|m| specs.iter().any(|s| s.id == m.spec.id));
        self.dirty |= self.mascots.len() != before;
        let mut todo = Vec::new();
        for spec in specs {
            match self.mascots.iter().position(|m| m.spec.id == spec.id) {
                Some(i) if self.mascots[i].spec == spec => {}
                Some(i) => {
                    let m = &mut self.mascots[i];
                    if m.spec.character != spec.character {
                        m.deck = Deck::default();
                    }
                    m.spec = spec;
                    m.asking = false;
                    todo.push(self.refresh(i, now));
                }
                None => {
                    self.mascots.push(Mascot {
                        spec,
                        deck: Deck::default(),
                        sent: VecDeque::new(),
                        own: (String::new(), Outfit::Plain),
                        own_since: now,
                        asking: false,
                        seq: 0,
                    });
                    todo.push(self.refresh(self.mascots.len() - 1, now));
                }
            }
        }
        todo
    }

    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.mascots.iter().map(|m| m.spec.id)
    }

    /// What mascot `i` shows changed: it pops up with it.
    fn touch(&mut self, i: usize) {
        let m = &mut self.mascots[i];
        m.seq = m.seq.wrapping_add(1);
        self.dirty = true;
    }

    /// Mascot `i` has a new line of its own, shown unless sent lines are in front of it.
    fn own_changed(&mut self, i: usize) {
        if self.mascots[i].sent.is_empty() {
            self.touch(i);
        }
    }

    /// Sent lines of mascot `i` went away. Once none are left, its own line is back and stays
    /// up a full interval.
    fn sent_dropped(&mut self, i: usize, now: Instant) {
        self.touch(i);
        let m = &mut self.mascots[i];
        if m.sent.is_empty() {
            m.own_since = now;
        }
    }

    /// A new line of its own for mascot `i`: from its command, or drawn from its remarks.
    fn refresh(&mut self, i: usize, now: Instant) -> Todo {
        let m = &mut self.mascots[i];
        m.own_since = now;
        if !m.spec.command.trim().is_empty() {
            if m.asking {
                return Todo::Nothing;
            }
            m.asking = true;
            return Todo::Run {
                id: m.spec.id,
                command: m.spec.command.clone(),
                name: m.spec.character.name(),
            };
        }
        m.own = m
            .deck
            .draw(m.spec.character, m.spec.wildness, &mut self.rng);
        self.own_changed(i);
        Todo::Nothing
    }

    /// What mascot `id`'s command printed (`None`: nothing usable, so a remark of its own).
    pub fn answer(&mut self, id: u32, out: Option<(String, Option<Outfit>)>, now: Instant) {
        let Some(i) = self.mascots.iter().position(|m| m.spec.id == id) else {
            return;
        };
        let m = &mut self.mascots[i];
        if !m.asking {
            return;
        }
        m.asking = false;
        m.own_since = now;
        m.own = match out {
            Some((text, outfit)) => (text, outfit.unwrap_or_default()),
            None => m
                .deck
                .draw(m.spec.character, m.spec.wildness, &mut self.rng),
        };
        self.own_changed(i);
    }

    /// A line sent from outside. Returns how many mascots got it, or why none did.
    pub fn say(&mut self, say: &Say, now: Instant) -> Result<usize, String> {
        let target = say.target()?;
        let text = clean(&say.text);
        if text.is_empty() {
            return Err("nothing to say".into());
        }
        if self.mascots.is_empty() {
            return Err("no mascot on the dashboard".into());
        }
        let picked: Vec<usize> = match target {
            Some(None) => (0..self.mascots.len()).collect(),
            Some(Some(c)) if self.mascots.iter().any(|m| m.spec.character == c) => {
                (0..self.mascots.len())
                    .filter(|&i| self.mascots[i].spec.character == c)
                    .collect()
            }
            // Any one, or the one asked for isn't there: whoever has the least to say.
            _ => {
                let least = self.mascots.iter().map(|m| m.sent.len()).min().unwrap_or(0);
                let idle: Vec<usize> = (0..self.mascots.len())
                    .filter(|&i| self.mascots[i].sent.len() == least)
                    .collect();
                vec![idle[self.rng.below(idle.len())]]
            }
        };
        let until = say.duration().map(|d| now + d);
        for &i in &picked {
            let sent = &mut self.mascots[i].sent;
            sent.push_back(Sent {
                text: text.clone(),
                outfit: say.outfit.unwrap_or_default(),
                until,
            });
            if sent.len() == 1 {
                self.touch(i);
            }
        }
        self.dirty = true;
        Ok(picked.len())
    }

    /// Clicked: a sent line goes away; otherwise a new line of its own.
    pub fn next(&mut self, id: u32, now: Instant) -> Todo {
        let Some(i) = self.mascots.iter().position(|m| m.spec.id == id) else {
            return Todo::Nothing;
        };
        if self.mascots[i].sent.pop_front().is_some() {
            self.sent_dropped(i, now);
            return Todo::Nothing;
        }
        self.refresh(i, now)
    }

    /// Once a second while the dashboard is up: expire sent lines, and say something new once
    /// a remark has been up for its interval.
    pub fn tick(&mut self, now: Instant) -> Vec<Todo> {
        let mut todo = Vec::new();
        for i in 0..self.mascots.len() {
            if self.mascots[i].expire(now) {
                self.sent_dropped(i, now);
            }
            let m = &self.mascots[i];
            if m.sent.is_empty() && !m.asking && now >= m.own_since + m.spec.interval {
                todo.push(self.refresh(i, now));
            }
        }
        todo
    }

    /// The dashboard opens: each mascot pops up again, with something new unless it has sent
    /// lines to show.
    pub fn wake(&mut self, now: Instant) -> Vec<Todo> {
        let mut todo = Vec::new();
        for i in 0..self.mascots.len() {
            self.mascots[i].expire(now);
            if self.mascots[i].sent.is_empty() {
                todo.push(self.refresh(i, now));
            } else {
                self.touch(i);
            }
        }
        self.dirty = true;
        todo
    }

    pub fn view(&self, id: u32) -> Option<View> {
        let m = self.mascots.iter().find(|m| m.spec.id == id)?;
        Some(match m.sent.front() {
            Some(s) => View {
                text: s.text.clone(),
                outfit: s.outfit,
                seq: m.seq,
                queued: m.sent.len() - 1,
            },
            None => View {
                text: m.own.0.clone(),
                outfit: m.own.1,
                seq: m.seq,
                queued: 0,
            },
        })
    }

    /// Whether anything shown changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn the_cows_bubble_is_shaped_like_cowsays() {
        assert_eq!(cow_bubble("moo", 40, 5), " _____\n< moo >\n -----");
        assert_eq!(
            cow_bubble("one two three four", 11, 5),
            " _________\n/ one two \\\n| three   |\n\\ four    /\n ---------"
        );
    }

    #[test]
    fn the_cows_bubble_stays_inside_its_box() {
        let long = "x".repeat(50);
        for (cols, rows) in [(10, 2), (24, 3), (5, 1), (0, 0)] {
            let b = cow_bubble(&format!("a few words and {long} too\nsecond"), cols, rows);
            assert!(b.lines().count() <= rows.max(1) + 2, "{b}");
            let widest = b.lines().map(|l| l.chars().count()).max().unwrap();
            assert!(widest <= cols.max(8), "{cols}: {b}");
        }
        assert!(cow_bubble(&"word ".repeat(40), 20, 2).contains('\u{2026}'));
    }

    #[test]
    fn everyone_has_something_to_say_even_when_calm() {
        for c in Character::iter() {
            assert!(remarks(c).iter().filter(|r| r.wildness == 0).count() >= 3);
            assert!(remarks(c).iter().all(|r| !COSTUMES.contains(&r.outfit)));
        }
    }

    #[test]
    fn a_deck_says_everything_once_before_repeating() {
        let mut rng = Rng(12345);
        for c in Character::iter() {
            let mut deck = Deck::default();
            let n = remarks(c).len();
            let mut seen = std::collections::HashSet::new();
            let mut drawn = 0;
            // Time-of-day lines may come in between; count only the regular ones.
            while drawn < n {
                let (text, _) = deck.draw(c, MAX_WILDNESS, &mut rng);
                if remarks(c).iter().any(|r| r.text == text) {
                    assert!(seen.insert(text), "{c:?} repeated a line within a round");
                    drawn += 1;
                }
            }
        }
    }

    #[test]
    fn calm_mascots_stay_plain_and_tame() {
        let mut rng = Rng(99);
        let mut deck = Deck::default();
        for _ in 0..200 {
            let (text, outfit) = deck.draw(Character::Schraubi, 0, &mut rng);
            assert!(!COSTUMES.contains(&outfit));
            if let Some(r) = SCHRAUBI.iter().find(|r| r.text == text) {
                assert_eq!(r.wildness, 0);
            }
        }
    }

    #[test]
    fn say_round_trips_over_the_wire() {
        let say = Say {
            text: "Build failed".into(),
            to: Some("zolli".into()),
            outfit: Some(Outfit::Helmet),
            secs: Some(30),
        };
        let back = Say::parse_wire(&say.to_wire()).unwrap().unwrap();
        assert_eq!(back, say);
        assert_eq!(back.target(), Ok(Some(Some(Character::Zolli))));
        let bare = Say::parse_wire(r#"say {"text":"hi"}"#).unwrap().unwrap();
        assert_eq!(bare.target(), Ok(None));
        assert_eq!(bare.duration(), Some(SAY_DEFAULT));
        assert!(Say::parse_wire("show").is_none());
        let bad = Say {
            to: Some("clippy".into()),
            ..Default::default()
        };
        assert!(bad.target().is_err());
    }

    #[test]
    fn command_output_can_pick_the_outfit() {
        assert_eq!(
            from_command_output("  hello\n"),
            Some(("hello".into(), None))
        );
        assert_eq!(
            from_command_output(r#"{"text":"careful","outfit":"vest"}"#),
            Some(("careful".into(), Some(Outfit::Vest)))
        );
        assert_eq!(from_command_output("  \n"), None);
        assert_eq!(clean(&"x".repeat(1000)).chars().count(), MAX_CHARS);
    }

    fn spec(id: u32, character: Character) -> Spec {
        Spec {
            id,
            character,
            wildness: 2,
            interval: Duration::from_secs(30),
            command: String::new(),
        }
    }

    fn said(text: &str, to: Option<&str>, secs: Option<u64>) -> Say {
        Say {
            text: text.into(),
            to: to.map(Into::into),
            outfit: None,
            secs,
        }
    }

    #[test]
    fn sent_lines_queue_up_and_go_away_on_click() {
        let t0 = Instant::now();
        let mut b = Board::default();
        b.sync(vec![spec(1, Character::Zolli)], t0);
        let own = b.view(1).unwrap();
        assert!(!own.text.is_empty());
        assert_eq!(b.say(&said("first", Some("zolli"), Some(0)), t0), Ok(1));
        assert_eq!(b.say(&said("second", None, Some(0)), t0), Ok(1));
        let v = b.view(1).unwrap();
        assert_eq!((v.text.as_str(), v.queued), ("first", 1));
        assert_ne!(v.seq, own.seq, "a new line pops the mascot up");
        b.next(1, t0);
        assert_eq!(b.view(1).unwrap().text, "second");
        b.next(1, t0);
        let back = b.view(1).unwrap();
        assert!(back.text == own.text, "its own line comes back");
    }

    #[test]
    fn sent_lines_expire_and_remarks_change_with_the_interval() {
        let t0 = Instant::now();
        let mut b = Board::default();
        b.sync(vec![spec(1, Character::Schraubi)], t0);
        b.say(&said("brief", None, Some(5)), t0).unwrap();
        b.tick(t0 + Duration::from_secs(4));
        assert_eq!(b.view(1).unwrap().text, "brief");
        b.tick(t0 + Duration::from_secs(6));
        let own = b.view(1).unwrap();
        assert_ne!(own.text, "brief");
        // A full interval from when the sent line went.
        b.tick(t0 + Duration::from_secs(30));
        assert_eq!(b.view(1).unwrap().seq, own.seq);
        b.tick(t0 + Duration::from_secs(36));
        assert_ne!(b.view(1).unwrap().seq, own.seq);
    }

    #[test]
    fn who_gets_a_line() {
        let t0 = Instant::now();
        let mut b = Board::default();
        b.sync(
            vec![
                spec(1, Character::Schraubi),
                spec(2, Character::Zolli),
                spec(3, Character::Zolli),
            ],
            t0,
        );
        assert_eq!(b.say(&said("hi", Some("all"), None), t0), Ok(3));
        assert_eq!(b.say(&said("hi", Some("zolli"), None), t0), Ok(2));
        // Any one: the one with the least queued.
        assert_eq!(b.say(&said("you", None, None), t0), Ok(1));
        assert_eq!(b.view(1).unwrap().queued, 1);
        // Asked for one that isn't there: someone else says it.
        assert_eq!(b.say(&said("lost", Some("verschnitti"), None), t0), Ok(1));
        assert!(b.say(&said("x", Some("clippy"), None), t0).is_err());
        assert!(b.say(&said("  ", None, None), t0).is_err());
        assert!(Board::default().say(&said("x", None, None), t0).is_err());
    }

    #[test]
    fn a_command_supplies_the_remarks() {
        let t0 = Instant::now();
        let mut b = Board::default();
        let mut s = spec(7, Character::Verschnitti);
        s.command = "fortune -s".into();
        let todo = b.sync(vec![s.clone()], t0);
        assert_eq!(
            todo,
            vec![Todo::Run {
                id: 7,
                command: "fortune -s".into(),
                name: "verschnitti"
            }]
        );
        // Still running: no second run.
        assert_eq!(b.next(7, t0), Todo::Nothing);
        b.answer(
            7,
            Some(("You will saw well.".into(), Some(Outfit::Beard))),
            t0,
        );
        let v = b.view(7).unwrap();
        assert_eq!(
            (v.text.as_str(), v.outfit),
            ("You will saw well.", Outfit::Beard)
        );
        // Failing commands fall back to its own remarks.
        assert!(matches!(b.next(7, t0), Todo::Run { .. }));
        b.answer(7, None, t0);
        assert!(
            remarks(Character::Verschnitti)
                .iter()
                .any(|r| r.text == b.view(7).unwrap().text)
                || timely(Character::Verschnitti, Local::now()).is_some()
        );
        // Removing the widget forgets it.
        b.sync(vec![], t0);
        assert!(b.view(7).is_none());
    }
}
