//! What the Deck's AirPlay speaker is playing, as the bridge on the Deck
//! reports it, and the remote commands the sending iPhone accepts (DACP).

// `ufmt` is named by what `fmt!` expands to.
use bmc_wasm_sdk::{Color, fmt, ufmt};

use crate::{LightColor, PlayingLight};

/// The bridge's state, served by the Deck's own web server to the Deck only.
pub const BRIDGE_URL: &str = "http://127.0.0.1:8000/cgi-bin/airplay-remote";
/// Asks the bridge to mute the speaker now: after a pause, the iPhone's
/// audio already buffered on the Deck would otherwise play on for 1–2 s.
pub const HUSH_URL: &str = "http://127.0.0.1:8000/cgi-bin/airplay-remote?hush=on";
/// Asks the bridge to unmute the speaker now, for a resume.
pub const UNHUSH_URL: &str = "http://127.0.0.1:8000/cgi-bin/airplay-remote?hush=off";

/// The mDNS service type an AirPlay sender advertises for remote control.
pub const DACP_SERVICE: &str = "_dacp._tcp";

/// AirPlay's volume scale runs from -30 dB (quietest) to 0 dB; -144 is mute.
const AIRPLAY_MIN_DB: f32 = -30.0;
const AIRPLAY_MUTE_DB: f32 = -144.0;
/// One press of an iPhone volume button: its 30 dB range in 16 steps.
pub const VOLUME_STEP_DB: f32 = 30.0 / 16.0;

/// Past this, a "duration" is a live stream's placeholder, not a length.
const MAX_TRACK_SECS: f32 = 24.0 * 3600.0;

/// Reads fields of a JSON document by path (`/title`), so parsing is
/// testable without the host's JSON parser.
pub trait JsonLookup {
    fn str(&self, path: &str) -> Option<String>;
    fn f64(&self, path: &str) -> Option<f64>;
    fn bool(&self, path: &str) -> Option<bool>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    /// An iPhone is connected to the speaker.
    pub session: bool,
    pub playing: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// AirPlay volume, -30..=0 dB, or -144 when muted.
    pub volume_db: Option<f32>,
    /// Playback position when the progress was reported, in seconds.
    pub position: Option<f32>,
    pub duration: Option<f32>,
    /// When the progress was reported, in Unix seconds.
    pub progress_at: Option<i64>,
    /// Which DACP service belongs to the connected iPhone.
    pub dacp_id: Option<String>,
    /// The token the iPhone expects on every remote command.
    pub active_remote: Option<String>,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "volumes, positions and Unix times all fit their narrower types"
)]
#[must_use]
pub fn parse(json: &impl JsonLookup) -> NowPlaying {
    let text = |path: &str| json.str(path).filter(|value| !value.trim().is_empty());
    NowPlaying {
        session: json.bool("/session").unwrap_or(false),
        playing: json.bool("/playing").unwrap_or(false),
        title: text("/title"),
        artist: text("/artist"),
        volume_db: json.f64("/volume").map(|db| db as f32),
        position: json.f64("/position").map(|secs| secs as f32),
        duration: json.f64("/duration").map(|secs| secs as f32),
        progress_at: json.f64("/progress_at").map(|secs| secs as i64),
        dacp_id: text("/dacp_id"),
        active_remote: text("/active_remote"),
    }
}

impl NowPlaying {
    /// The playback position at `now_secs`, counting on from the last
    /// report while playing. `None` when there is no track length to
    /// place it in, as for a live stream.
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "seconds since a report are few")]
    pub fn position_at(&self, now_secs: i64) -> Option<(f32, f32)> {
        let duration = self.duration.filter(|d| *d > 0.0 && *d < MAX_TRACK_SECS)?;
        let mut position = self.position?;
        if self.playing {
            let reported = self.progress_at?;
            position += now_secs.saturating_sub(reported).max(0) as f32;
        }
        Some((position.clamp(0.0, duration), duration))
    }

    /// Volume as a 0..=1 fraction of AirPlay's range; muted reads as 0.
    #[must_use]
    pub fn volume_fraction(&self) -> Option<f32> {
        let db = self.volume_db?;
        Some(if db <= AIRPLAY_MUTE_DB {
            0.0
        } else {
            ((db - AIRPLAY_MIN_DB) / -AIRPLAY_MIN_DB).clamp(0.0, 1.0)
        })
    }

    /// Shows a command's effect before the iPhone's own report arrives.
    pub fn anticipate(&mut self, command: Command, now_secs: i64) {
        match command {
            Command::PlayPause => {
                if let Some((position, _)) = self.position_at(now_secs) {
                    self.position = Some(position);
                }
                self.progress_at = Some(now_secs);
                self.playing = !self.playing;
            }
            Command::VolumeUp | Command::VolumeDown => {
                let step = if command == Command::VolumeUp {
                    VOLUME_STEP_DB
                } else {
                    -VOLUME_STEP_DB
                };
                self.volume_db = self.volume_db.map(|db| {
                    let from = if db <= AIRPLAY_MUTE_DB {
                        AIRPLAY_MIN_DB
                    } else {
                        db
                    };
                    (from + step).clamp(AIRPLAY_MIN_DB, 0.0)
                });
            }
            Command::Next | Command::Previous => {}
        }
    }
}

/// Longest a paused song stays on screen once its iPhone has ended the audio
/// stream (see [`carry_over`]).
pub const KEEP_PAUSED_SECS: i64 = 30 * 60;

/// What the widget knows beyond the bridge's report, for [`carry_over`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Carry {
    /// The shown session's iPhone still offers remote control.
    pub reachable: bool,
    /// A play pressed on the Deck is waiting for the iPhone's new stream.
    pub resuming: bool,
    /// How long the bridge has reported no session, in seconds.
    pub idle_secs: i64,
}

/// The session to show, given what is on screen and the bridge's latest
/// report. An iPhone ends its audio stream about 6 s into a pause and starts
/// a new one about 4 s after play, and the bridge reports each as the session
/// ending or starting. So while the iPhone still offers remote control, a
/// paused song stays on screen with its buttons, for up to
/// [`KEEP_PAUSED_SECS`], and the same iPhone's new stream keeps the song's
/// details until its own arrive.
#[must_use]
pub fn carry_over(
    shown: &NowPlaying,
    mut fresh: NowPlaying,
    carry: Carry,
    now_secs: i64,
) -> NowPlaying {
    if !shown.session {
        return fresh;
    }
    if fresh.session {
        if fresh.dacp_id.is_none() || fresh.dacp_id == shown.dacp_id {
            if fresh.title.is_none() {
                fresh.title.clone_from(&shown.title);
                fresh.artist.clone_from(&shown.artist);
            }
            if fresh.position.is_none() {
                fresh.position = shown.position_at(now_secs).map(|(position, _)| position);
                fresh.duration = shown.duration;
                fresh.progress_at = Some(now_secs);
            }
            fresh.dacp_id = fresh.dacp_id.or_else(|| shown.dacp_id.clone());
            fresh.active_remote = fresh.active_remote.or_else(|| shown.active_remote.clone());
        }
        return fresh;
    }
    if !carry.reachable || carry.idle_secs > KEEP_PAUSED_SECS {
        return fresh;
    }
    let mut kept = shown.clone();
    if kept.playing && !carry.resuming {
        kept.position = shown
            .position_at(now_secs)
            .map(|(position, _)| position)
            .or(kept.position);
        kept.progress_at = Some(now_secs);
        kept.playing = false;
    }
    kept
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
}

impl Command {
    pub const ALL: [Self; 5] = [
        Self::Previous,
        Self::PlayPause,
        Self::Next,
        Self::VolumeDown,
        Self::VolumeUp,
    ];

    #[must_use]
    pub fn dacp_path(self) -> &'static str {
        match self {
            Self::PlayPause => "playpause",
            Self::Next => "nextitem",
            Self::Previous => "previtem",
            Self::VolumeUp => "volumeup",
            Self::VolumeDown => "volumedown",
        }
    }
}

/// Whether an mDNS service name is the remote-control service of the
/// iPhone whose DACP id is `dacp_id` (`iTunes_Ctrl_<id>._dacp._tcp.local.`).
#[must_use]
pub fn is_remote_of(service_name: &str, dacp_id: &str) -> bool {
    service_name
        .strip_prefix("iTunes_Ctrl_")
        .and_then(|rest| rest.split('.').next())
        .is_some_and(|id| id.eq_ignore_ascii_case(dacp_id))
}

/// Where a command goes: the iPhone's DACP service at an IPv4 `host`.
#[must_use]
pub fn command_url(host: &str, port: u16, command: Command) -> String {
    fmt!(
        "http://{}:{}/ctrl-int/1/{}",
        host,
        port,
        command.dacp_path()
    )
}

/// `3:05` or `1:02:03`.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clamped, non-negative track time in whole seconds"
)]
pub fn clock(secs: f32) -> String {
    let total = secs.max(0.0).floor() as u32;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    let pad = |n: u32| {
        if n < 10 {
            fmt!("0{}", n)
        } else {
            fmt!("{}", n)
        }
    };
    if hours > 0 {
        fmt!(
            "{}:{}:{}",
            hours,
            pad(minutes).as_str(),
            pad(seconds).as_str()
        )
    } else {
        fmt!("{}:{}", minutes, pad(seconds).as_str())
    }
}

/// What the light strip should show for the speaker's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    Off,
    Steady(Color),
    Pulse(Color),
}

/// On while music plays, off once it pauses or stops, or if the operator
/// turned the light off. `speaker` is `None` when the bridge isn't answering.
/// A widget added before these settings existed has no value for them, so an
/// unset one takes the manifest's default.
#[must_use]
pub fn light(
    speaker: Option<&NowPlaying>,
    mode: Option<PlayingLight>,
    color: Option<LightColor>,
) -> Light {
    let playing = speaker.is_some_and(|np| np.session && np.playing);
    let mode = mode.unwrap_or(PlayingLight::Steady);
    let color = match color.unwrap_or(LightColor::Violet) {
        LightColor::Violet => Color::from_rgb(91, 76, 240),
        LightColor::WarmWhite => Color::from_rgb(255, 196, 120),
        LightColor::Blue => Color::from_rgb(40, 110, 255),
        LightColor::Green => Color::from_rgb(40, 220, 90),
        LightColor::Orange => Color::from_rgb(255, 120, 20),
    };
    match (playing, mode) {
        (false, _) | (true, PlayingLight::Off) => Light::Off,
        (true, PlayingLight::Steady) => Light::Steady(color),
        (true, PlayingLight::Pulse) => Light::Pulse(color),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct Json(HashMap<&'static str, Value>);

    enum Value {
        Str(&'static str),
        Num(f64),
        Bool(bool),
    }

    impl JsonLookup for Json {
        fn str(&self, path: &str) -> Option<String> {
            match self.0.get(path)? {
                Value::Str(s) => Some(String::from(*s)),
                Value::Num(_) | Value::Bool(_) => None,
            }
        }
        fn f64(&self, path: &str) -> Option<f64> {
            match self.0.get(path)? {
                Value::Num(n) => Some(*n),
                Value::Str(_) | Value::Bool(_) => None,
            }
        }
        fn bool(&self, path: &str) -> Option<bool> {
            match self.0.get(path)? {
                Value::Bool(b) => Some(*b),
                Value::Str(_) | Value::Num(_) => None,
            }
        }
    }

    /// The bridge's output for the session recorded on the Deck.
    fn recorded() -> Json {
        Json(HashMap::from([
            ("/session", Value::Bool(true)),
            ("/playing", Value::Bool(true)),
            (
                "/title",
                Value::Str("Lazare & ADDAM - Muhuuuuu (Official Audio)"),
            ),
            ("/artist", Value::Str("Spinnin’ Deep")),
            ("/volume", Value::Num(-14.37)),
            ("/position", Value::Num(4.09)),
            ("/duration", Value::Num(225.30)),
            ("/progress_at", Value::Num(1_790_496_218.0)),
            ("/dacp_id", Value::Str("BA91E6C04CA0CAF0")),
            ("/active_remote", Value::Str("7551234567")),
        ]))
    }

    #[test]
    fn the_recorded_state_reads_as_a_playing_session() {
        let np = parse(&recorded());
        assert!(np.session && np.playing);
        assert_eq!(np.artist.as_deref(), Some("Spinnin’ Deep"));
        assert_eq!(np.dacp_id.as_deref(), Some("BA91E6C04CA0CAF0"));
        assert_eq!(np.progress_at, Some(1_790_496_218));
    }

    #[test]
    fn a_missing_or_blank_field_reads_as_absent() {
        let mut json = recorded();
        json.0.remove("/title");
        json.0.insert("/artist", Value::Str("  "));
        let np = parse(&json);
        assert_eq!((np.title, np.artist), (None, None));
        assert!(!parse(&Json::default()).session);
    }

    #[test]
    fn playback_counts_on_from_the_report_while_playing() {
        let np = parse(&recorded());
        let (position, duration) = np
            .position_at(1_790_496_218 + 10)
            .expect("BUG: has a length");
        assert!((position - 14.09).abs() < 0.01, "{position}");
        assert!((duration - 225.30).abs() < 0.01);
    }

    #[test]
    fn a_paused_position_stays_put_and_never_passes_the_end() {
        let mut np = parse(&recorded());
        np.playing = false;
        let (position, _) = np
            .position_at(1_790_496_218 + 60)
            .expect("BUG: has a length");
        assert!((position - 4.09).abs() < 0.01);
        np.playing = true;
        let (position, duration) = np
            .position_at(1_790_496_218 + 10_000)
            .expect("BUG: has a length");
        assert!((position - duration).abs() < f32::EPSILON);
    }

    #[test]
    fn a_live_stream_has_no_progress() {
        let mut np = parse(&recorded());
        np.duration = Some(MAX_TRACK_SECS * 2.0);
        assert_eq!(np.position_at(1_790_496_218), None);
    }

    #[test]
    fn volume_maps_airplay_decibels_to_a_fraction() {
        let mut np = NowPlaying::default();
        for (db, fraction) in [(0.0, 1.0), (-30.0, 0.0), (-15.0, 0.5), (-144.0, 0.0)] {
            np.volume_db = Some(db);
            let got = np.volume_fraction().expect("BUG: volume set");
            assert!((got - fraction).abs() < 1e-4, "{db} -> {got}");
        }
    }

    #[test]
    fn volume_steps_like_the_iphone_buttons_and_stays_in_range() {
        let mut np = NowPlaying {
            volume_db: Some(-1.0),
            ..NowPlaying::default()
        };
        np.anticipate(Command::VolumeUp, 0);
        assert!((np.volume_db.expect("BUG: set") - 0.0).abs() < f32::EPSILON);
        np.volume_db = Some(-144.0);
        np.anticipate(Command::VolumeUp, 0);
        assert!((np.volume_db.expect("BUG: set") - (-30.0 + VOLUME_STEP_DB)).abs() < 1e-4);
        np.volume_db = Some(-29.0);
        np.anticipate(Command::VolumeDown, 0);
        assert!((np.volume_db.expect("BUG: set") - (-30.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn pausing_freezes_the_position_where_it_was() {
        let mut np = parse(&recorded());
        np.anticipate(Command::PlayPause, 1_790_496_218 + 20);
        assert!(!np.playing);
        let (position, _) = np
            .position_at(1_790_496_218 + 90)
            .expect("BUG: has a length");
        assert!((position - 24.09).abs() < 0.01, "{position}");
    }

    #[test]
    fn the_remote_service_is_matched_by_dacp_id() {
        let name = "iTunes_Ctrl_BA91E6C04CA0CAF0._dacp._tcp.local.";
        assert!(is_remote_of(name, "BA91E6C04CA0CAF0"));
        assert!(is_remote_of(name, "ba91e6c04ca0caf0"));
        assert!(!is_remote_of(name, "BA91E6C04CA0CAF1"));
        assert!(!is_remote_of("Other._dacp._tcp.local.", "BA91E6C04CA0CAF0"));
    }

    #[test]
    fn commands_go_to_the_iphone_s_dacp_path() {
        assert_eq!(
            command_url("192.168.1.7", 62414, Command::PlayPause),
            "http://192.168.1.7:62414/ctrl-int/1/playpause"
        );
        assert_eq!(Command::Next.dacp_path(), "nextitem");
        assert_eq!(Command::VolumeDown.dacp_path(), "volumedown");
    }

    #[test]
    fn clock_pads_minutes_and_seconds() {
        assert_eq!(clock(4.9), "0:04");
        assert_eq!(clock(225.3), "3:45");
        assert_eq!(clock(3_725.0), "1:02:05");
        assert_eq!(clock(-3.0), "0:00");
    }

    #[test]
    fn the_light_is_on_only_while_music_plays() {
        let playing = parse(&recorded());
        let paused = NowPlaying {
            playing: false,
            ..playing.clone()
        };
        let idle = NowPlaying::default();
        let violet = Color::from_rgb(91, 76, 240);

        assert_eq!(
            light(
                Some(&playing),
                Some(PlayingLight::Steady),
                Some(LightColor::Violet)
            ),
            Light::Steady(violet)
        );
        assert_eq!(
            light(
                Some(&playing),
                Some(PlayingLight::Pulse),
                Some(LightColor::Violet)
            ),
            Light::Pulse(violet)
        );
        for off in [Some(&paused), Some(&idle), None] {
            assert_eq!(
                light(off, Some(PlayingLight::Steady), Some(LightColor::Violet)),
                Light::Off
            );
        }
    }

    #[test]
    fn a_widget_saved_before_the_light_settings_gets_the_defaults() {
        let playing = parse(&recorded());
        assert_eq!(
            light(Some(&playing), None, None),
            Light::Steady(Color::from_rgb(91, 76, 240))
        );
    }

    #[test]
    fn the_operator_can_keep_the_light_off() {
        let playing = parse(&recorded());
        assert_eq!(
            light(
                Some(&playing),
                Some(PlayingLight::Off),
                Some(LightColor::Blue)
            ),
            Light::Off
        );
    }

    /// When the recorded session's progress was reported.
    const REPORTED: i64 = 1_790_496_218;

    fn paused() -> NowPlaying {
        NowPlaying {
            playing: false,
            ..parse(&recorded())
        }
    }

    #[test]
    fn a_pause_outlives_the_iphone_ending_its_stream() {
        let carry = Carry {
            reachable: true,
            resuming: false,
            idle_secs: 10,
        };
        let shown = paused();
        assert_eq!(
            carry_over(&shown, NowPlaying::default(), carry, REPORTED + 20),
            shown
        );
    }

    #[test]
    fn nothing_plays_once_the_iphone_leaves_or_after_a_long_pause() {
        let shown = paused();
        let gone = Carry::default();
        let stale = Carry {
            reachable: true,
            resuming: false,
            idle_secs: KEEP_PAUSED_SECS + 1,
        };
        for carry in [gone, stale] {
            assert!(!carry_over(&shown, NowPlaying::default(), carry, REPORTED).session);
        }
    }

    #[test]
    fn a_play_from_the_deck_shows_while_the_stream_restarts() {
        let mut shown = paused();
        shown.anticipate(Command::PlayPause, REPORTED + 30);
        let carry = Carry {
            reachable: true,
            resuming: true,
            idle_secs: 30,
        };
        let kept = carry_over(&shown, NowPlaying::default(), carry, REPORTED + 33);
        assert!(kept.session && kept.playing);
        let (position, _) = kept.position_at(REPORTED + 33).expect("BUG: has a length");
        assert!((position - 7.09).abs() < 0.01, "{position}");
    }

    #[test]
    fn a_stream_that_ends_while_playing_freezes_where_it_was() {
        let shown = parse(&recorded());
        let carry = Carry {
            reachable: true,
            resuming: false,
            idle_secs: 0,
        };
        let kept = carry_over(&shown, NowPlaying::default(), carry, REPORTED + 5);
        assert!(kept.session && !kept.playing);
        let (position, _) = kept.position_at(REPORTED + 60).expect("BUG: has a length");
        assert!((position - 9.09).abs() < 0.01, "{position}");
    }

    #[test]
    fn a_restarted_stream_keeps_the_song_until_its_details_arrive() {
        let shown = paused();
        let restarted = NowPlaying {
            session: true,
            playing: true,
            dacp_id: shown.dacp_id.clone(),
            ..NowPlaying::default()
        };
        let now = carry_over(&shown, restarted, Carry::default(), REPORTED + 100);
        assert_eq!((&now.title, &now.artist), (&shown.title, &shown.artist));
        assert_eq!(now.active_remote, shown.active_remote);
        let (position, _) = now.position_at(REPORTED + 102).expect("BUG: has a length");
        assert!((position - 6.09).abs() < 0.01, "{position}");
    }

    #[test]
    fn another_iphone_starts_from_its_own_details() {
        let other = NowPlaying {
            session: true,
            playing: true,
            dacp_id: Some(String::from("0000000000000001")),
            ..NowPlaying::default()
        };
        assert_eq!(
            carry_over(&paused(), other.clone(), Carry::default(), REPORTED),
            other
        );
    }

    #[test]
    fn pausing_from_the_deck_turns_the_light_off_at_once() {
        let mut np = parse(&recorded());
        np.anticipate(Command::PlayPause, 1_790_496_218);
        assert_eq!(
            light(
                Some(&np),
                Some(PlayingLight::Steady),
                Some(LightColor::Green)
            ),
            Light::Off
        );
    }
}
