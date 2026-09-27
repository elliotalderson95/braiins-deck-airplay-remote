//! The render tree: what is playing on top, the transport and volume
//! buttons below it (or beside it, on wide viewports).

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "view code uses the SDK's builders, colours and macros throughout"
    )
)]
use bmc_wasm_sdk::*;

use std::f32::consts::FRAC_PI_2;

use crate::model::{Command, NowPlaying, clock};

/// Accent for the play state and the progress bar.
const ACCENT: Color = Color::from_hex(0x5B_4C_F0);

/// SDK 0.6's default line height, as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.4;
/// The smallest size fitted text shrinks to.
const MIN_FIT_SIZE: u16 = 12;
/// A generous average glyph advance, in ems. SDK 0.6 wraps text it cannot
/// shrink into its box instead of cutting it, and a wrapped line would spill
/// onto the next; so text is first capped at what the box holds at
/// `MIN_FIT_SIZE`. Later SDKs cut text themselves (`TextOverflow`).
const GLYPH_EM: f32 = 0.62;

/// What the widget knows about the speaker.
#[derive(Debug)]
pub enum Source<'a> {
    /// No answer from the bridge yet.
    Waiting,
    /// The bridge on the Deck did not answer.
    BridgeDown,
    /// Answered: an iPhone is connected, or nothing is.
    Speaker(&'a NowPlaying),
}

#[derive(Debug)]
pub struct View<'a> {
    pub width: u32,
    pub height: u32,
    pub source: Source<'a>,
    pub now_secs: i64,
    /// The iPhone's remote-control service has been found.
    pub remote_ready: bool,
    /// A short-lived message, e.g. after a command went unanswered.
    pub notice: Option<&'a str>,
}

/// The click id a command's button reports its taps under.
#[must_use]
pub fn tap_key(command: Command) -> String {
    fmt!("cmd-{}", command.dacp_path())
}

/// Sizes for one viewport band. Fonts are fixed per band, not scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    /// Controls beside the song rather than under it.
    side_by_side: bool,
    padding: f32,
    gap: f32,
    /// `0` hides the status line.
    status_size: u32,
    title_size: u32,
    artist_size: u32,
    time_size: u32,
    track_h: f32,
    skip_button: f32,
    play_button: f32,
    volume_button: f32,
    /// Width of the controls column when side by side.
    controls_w: f32,
}

fn geometry(width: u32, height: u32) -> Geometry {
    let base = Geometry {
        side_by_side: false,
        padding: 10.0,
        gap: 5.0,
        status_size: 0,
        title_size: 18,
        artist_size: 14,
        time_size: 11,
        track_h: 3.0,
        skip_button: 42.0,
        play_button: 54.0,
        volume_button: 32.0,
        controls_w: 0.0,
    };
    if width >= 900 {
        Geometry {
            side_by_side: true,
            padding: 32.0,
            gap: 16.0,
            status_size: 18,
            title_size: 44,
            artist_size: 28,
            time_size: 18,
            track_h: 6.0,
            skip_button: 84.0,
            play_button: 116.0,
            volume_button: 60.0,
            controls_w: 440.0,
        }
    } else if height < 300 && width >= 560 {
        Geometry {
            side_by_side: true,
            padding: 16.0,
            gap: 8.0,
            status_size: 14,
            title_size: 24,
            artist_size: 18,
            time_size: 13,
            track_h: 4.0,
            skip_button: 52.0,
            play_button: 68.0,
            volume_button: 40.0,
            controls_w: 250.0,
        }
    } else if height >= 420 {
        Geometry {
            padding: 24.0,
            gap: 10.0,
            status_size: 16,
            title_size: 32,
            artist_size: 22,
            time_size: 15,
            track_h: 5.0,
            skip_button: 68.0,
            play_button: 92.0,
            volume_button: 48.0,
            ..base
        }
    } else if height >= 300 {
        Geometry {
            padding: 14.0,
            gap: 6.0,
            status_size: 13,
            title_size: 24,
            artist_size: 17,
            time_size: 12,
            track_h: 4.0,
            skip_button: 48.0,
            play_button: 62.0,
            volume_button: 36.0,
            ..base
        }
    } else {
        base
    }
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "a viewport is at most a few thousand pixels"
)]
pub fn widget(view: &View<'_>) -> Node {
    let g = geometry(view.width, view.height);
    let (width, height) = (view.width as f32, view.height as f32);
    let inner_w = width - 2.0 * g.padding;
    let body = match &view.source {
        Source::Speaker(np) if np.session => playing(view, np, &g, inner_w),
        Source::Speaker(_) => message(
            &g,
            "Nothing playing",
            "On your iPhone: Control Center → AirPlay → Braiins Deck",
        ),
        Source::BridgeDown => message(
            &g,
            "AirPlay bridge not answering",
            "Reinstall the AirPlay remote bridge on this Deck",
        ),
        Source::Waiting => message(&g, "Connecting…", ""),
    };
    col(
        props!(width: width, height: height, padding: g.padding, background: BLACK),
        [body],
    )
}

fn message(g: &Geometry, headline: &str, detail: &str) -> Node {
    let big = g.title_size.max(20);
    let mut lines = vec![text(
        headline,
        style!(size: big, weight: FontWeight::BOLD, color: WHITE, align: TextAlign::Center),
    )];
    if !detail.is_empty() {
        lines.push(text(
            detail,
            style!(size: (big * 2 / 3).max(13), color: GRAY_40, align: TextAlign::Center),
        ));
    }
    col(
        props!(flex: 1.0, gap: g.gap * 2.0, justify_content: Justify::Center),
        lines,
    )
}

/// Where the controls sit: a region the transport and the − bar + row
/// share, so the volume bar is centred under the play button, and a gutter
/// to its left for the speaker that labels the volume from outside it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Controls {
    region: f32,
    gutter: f32,
    /// A matching empty gutter on the right keeps a stacked layout centred.
    mirrored: bool,
}

impl Controls {
    fn width(&self) -> f32 {
        let gutters = if self.mirrored { 2.0 } else { 1.0 };
        self.region + gutters * self.gutter
    }
}

fn speaker_size(g: &Geometry) -> f32 {
    g.volume_button * 0.7
}

fn controls_layout(g: &Geometry, inner_w: f32) -> Controls {
    let gutter = speaker_size(g) + g.gap * 2.0;
    if g.side_by_side {
        Controls {
            region: g.controls_w,
            gutter,
            mirrored: false,
        }
    } else {
        Controls {
            region: inner_w - 2.0 * gutter,
            gutter,
            mirrored: true,
        }
    }
}

fn playing(view: &View<'_>, np: &NowPlaying, g: &Geometry, inner_w: f32) -> Node {
    let layout = controls_layout(g, inner_w);
    let info_w = if g.side_by_side {
        inner_w - layout.width() - g.padding
    } else {
        inner_w
    };
    let mut info = Vec::with_capacity(5);
    if g.status_size > 0 {
        info.push(status_line(view, np, g, info_w));
    }
    info.push(fitted(
        np.title.as_deref().unwrap_or("Unknown track"),
        info_w,
        line_h(g.title_size),
        g.title_size,
        FontWeight::BOLD,
        WHITE,
    ));
    if let Some(artist) = &np.artist {
        info.push(fitted(
            artist,
            info_w,
            line_h(g.artist_size),
            g.artist_size,
            FontWeight::REGULAR,
            GRAY_30,
        ));
    }
    if g.side_by_side {
        info.push(spacer(1.0));
    }
    if let Some((position, duration)) = np.position_at(view.now_secs) {
        info.push(progress(position, duration, g));
    }

    let controls = controls(np, view.remote_ready, g, &layout);
    if g.side_by_side {
        row(
            props!(flex: 1.0, gap: g.padding),
            [
                col(props!(width: info_w, gap: g.gap), info),
                col(
                    props!(width: layout.width(), justify_content: Justify::Center),
                    [controls],
                ),
            ],
        )
    } else {
        info.push(spacer(1.0));
        info.push(controls);
        col(props!(flex: 1.0, gap: g.gap), info)
    }
}

/// "● Playing on Braiins Deck", or what the remote is waiting for.
fn status_line(view: &View<'_>, np: &NowPlaying, g: &Geometry, width: f32) -> Node {
    let (dot, words, color) = match view.notice {
        Some(notice) => (ORANGE_50, notice, ORANGE_30),
        None if !view.remote_ready => (GRAY_50, "Finding your iPhone…", GRAY_40),
        None if np.playing => (GREEN_50, "Playing on Braiins Deck", GRAY_40),
        None => (GRAY_50, "Paused", GRAY_40),
    };
    #[expect(clippy::cast_precision_loss, reason = "font sizes are tiny")]
    let d = (g.status_size as f32 * 0.55).round();
    row(
        props!(gap: 8.0, cross_align: CrossAlign::Center),
        [
            canvas(
                props!(width: d, height: d),
                [Draw::circle(d / 2.0, d / 2.0, d / 2.0, dot)],
            ),
            fitted(
                words,
                width - d - 8.0,
                line_h(g.status_size),
                g.status_size,
                FontWeight::REGULAR,
                color,
            ),
        ],
    )
}

fn progress(position: f32, duration: f32, g: &Geometry) -> Node {
    let fraction = if duration > 0.0 {
        position / duration
    } else {
        0.0
    };
    let time = |secs: f32| text(clock(secs), style!(size: g.time_size, color: GRAY_40));
    col(
        props!(gap: g.gap),
        [
            progress_bar!(
                ProgressMode::Meter(fraction.clamp(0.0, 1.0)),
                track_h: g.track_h,
                fill_color: ACCENT,
                track_color: GRAY_80
            ),
            row(
                props!(justify_content: Justify::SpaceBetween),
                [time(position), time(duration)],
            ),
        ],
    )
}

/// The transport over the volume row, both across the shared region, with
/// the speaker in the gutter left of the volume row.
fn controls(np: &NowPlaying, ready: bool, g: &Geometry, layout: &Controls) -> Node {
    let level = np.volume_fraction().unwrap_or(0.0);
    let ink = if ready { GRAY_30 } else { GRAY_60 };
    let gutter = |content: Vec<Node>| {
        row(
            props!(width: layout.gutter, cross_align: CrossAlign::Center),
            content,
        )
    };
    let line = |label: Vec<Node>, body: Node| {
        let mut cells = vec![gutter(label), body];
        if layout.mirrored {
            cells.push(gutter(Vec::new()));
        }
        row(props!(cross_align: CrossAlign::Center), cells)
    };
    col(
        props!(gap: g.gap * 2.0, justify_content: Justify::Center),
        [
            line(Vec::new(), transport(np, ready, g, layout.region)),
            line(
                vec![speaker(speaker_size(g), level, ink)],
                volume(np, ready, g, layout.region, level),
            ),
        ],
    )
}

fn transport(np: &NowPlaying, ready: bool, g: &Geometry, width: f32) -> Node {
    row(
        props!(width: width, gap: g.gap * 3.0, justify_content: Justify::Center,
            cross_align: CrossAlign::Center),
        [
            button(Command::Previous, g.skip_button, ready, np.playing),
            button(Command::PlayPause, g.play_button, ready, np.playing),
            button(Command::Next, g.skip_button, ready, np.playing),
        ],
    )
}

/// − bar +, mirror images of each other, so the bar is centred in `width`.
fn volume(np: &NowPlaying, ready: bool, g: &Geometry, width: f32, level: f32) -> Node {
    row(
        props!(width: width, gap: g.gap * 2.0, cross_align: CrossAlign::Center),
        [
            button(Command::VolumeDown, g.volume_button, ready, np.playing),
            col(
                props!(flex: 1.0, justify_content: Justify::Center),
                [progress_bar!(
                    ProgressMode::Meter(level),
                    track_h: g.track_h,
                    fill_color: GRAY_20,
                    track_color: GRAY_80
                )],
            ),
            button(Command::VolumeUp, g.volume_button, ready, np.playing),
        ],
    )
}

/// Sound waves the speaker icon shows for a 0..=1 volume level.
fn sound_waves(level: f32) -> usize {
    if level <= 0.0 {
        0
    } else if level < 0.5 {
        1
    } else {
        2
    }
}

/// A speaker on an `s`-sized square, with waves that follow the level,
/// so the bar beside it reads as volume.
fn speaker(s: f32, level: f32, ink: Color) -> Node {
    let (cx, cy) = (0.46 * s, 0.5 * s);
    let wave = |radius: f32, spread: f32| {
        // Arc angles run clockwise from 12 o'clock, so 3 o'clock faces right.
        Draw::arc(
            cx,
            cy,
            radius * s,
            FRAC_PI_2 - spread,
            FRAC_PI_2 + spread,
            0.08 * s,
            ArcFill::Solid(ink),
            ArcSegments::Continuous,
            ArcCap::Round,
        )
    };
    let mut draws = vec![
        Draw::rect(0.06 * s, 0.36 * s, 0.18 * s, 0.28 * s, ink),
        Draw::fill_path(
            vec![
                (0.24 * s, 0.36 * s),
                (cx, 0.16 * s),
                (cx, 0.84 * s),
                (0.24 * s, 0.64 * s),
            ],
            ink,
            false,
        ),
    ];
    let waves = sound_waves(level);
    if waves >= 1 {
        draws.push(wave(0.2, 0.75));
    }
    if waves >= 2 {
        draws.push(wave(0.38, 0.8));
    }
    canvas(props!(width: s, height: s), draws)
}

/// A round button with a drawn glyph; inert until the iPhone can be reached.
fn button(command: Command, size: f32, ready: bool, playing: bool) -> Node {
    let primary = command == Command::PlayPause;
    let (fill, ink) = match (ready, primary) {
        (true, true) => (WHITE, BLACK),
        (true, false) => (GRAY_80, WHITE),
        (false, _) => (GRAY_90, GRAY_60),
    };
    let r = size / 2.0;
    let mut draws = vec![Draw::circle(r, r, r, fill)];
    draws.extend(glyph(command, size, ink, playing));
    let props = props!(width: size, height: size);
    if ready {
        touchable(&tap_key(command), props, draws)
    } else {
        canvas(props, draws)
    }
}

/// The button's symbol, drawn on an `s`-sized square: ▶ or ❚❚, |◀, ▶|, −, +.
fn glyph(command: Command, s: f32, ink: Color, playing: bool) -> Vec<Draw> {
    let at = |x: f32, y: f32| (x * s, y * s);
    let bar = |x: f32, y: f32, w: f32, h: f32| Draw::rect(x * s, y * s, w * s, h * s, ink);
    let triangle = |points: [(f32, f32); 3]| {
        Draw::fill_path(points.iter().map(|&(x, y)| at(x, y)).collect(), ink, false)
    };
    match command {
        Command::PlayPause if playing => vec![bar(0.35, 0.3, 0.1, 0.4), bar(0.55, 0.3, 0.1, 0.4)],
        // Nudged right of centre: a triangle's visual weight sits left of its box.
        Command::PlayPause => vec![triangle([(0.4, 0.3), (0.4, 0.7), (0.72, 0.5)])],
        Command::Next => vec![
            triangle([(0.32, 0.32), (0.32, 0.68), (0.58, 0.5)]),
            bar(0.6, 0.32, 0.07, 0.36),
        ],
        Command::Previous => vec![
            bar(0.33, 0.32, 0.07, 0.36),
            triangle([(0.68, 0.32), (0.68, 0.68), (0.42, 0.5)]),
        ],
        Command::VolumeDown => vec![bar(0.3, 0.47, 0.4, 0.06)],
        Command::VolumeUp => vec![bar(0.3, 0.47, 0.4, 0.06), bar(0.47, 0.3, 0.06, 0.4)],
    }
}

#[expect(clippy::cast_precision_loss, reason = "font sizes are tiny")]
fn line_h(size: u32) -> f32 {
    (size as f32 * LINE_HEIGHT).ceil()
}

/// One line of text in a `width` × `height` box, shrunk to fit it.
fn fitted(
    content: &str,
    width: f32,
    height: f32,
    size: u32,
    weight: FontWeight,
    color: Color,
) -> Node {
    canvas(
        props!(width: width, height: height),
        [Draw::autofit_text_ranged(
            0.0,
            0.0,
            width,
            height,
            cap_to_width(content, width),
            style!(size: size, weight: weight, color: color),
            AutoFit::Shrink,
            MIN_FIT_SIZE,
            u16::try_from(size).unwrap_or(u16::MAX),
        )],
    )
}

/// `content` cut with an ellipsis to what `width` holds at `MIN_FIT_SIZE`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive width over a glyph advance is a small character count"
)]
fn cap_to_width(content: &str, width: f32) -> String {
    let max_chars = (width / (f32::from(MIN_FIT_SIZE) * GLYPH_EM))
        .floor()
        .max(1.0) as usize;
    if content.chars().count() <= max_chars {
        return String::from(content);
    }
    let kept: String = content.chars().take(max_chars - 1).collect();
    let mut cut = String::from(kept.trim_end());
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> NowPlaying {
        NowPlaying {
            session: true,
            playing: true,
            title: Some(String::from("Lazare & ADDAM - Muhuuuuu (Official Audio)")),
            artist: Some(String::from("Spinnin’ Deep")),
            volume_db: Some(-14.37),
            position: Some(4.09),
            duration: Some(225.3),
            progress_at: Some(100),
            dacp_id: Some(String::from("BA91E6C04CA0CAF0")),
            active_remote: Some(String::from("7551234567")),
        }
    }

    const SIZES: [(u32, u32); 6] = [
        (1280, 480),
        (638, 480),
        (638, 238),
        (317, 238),
        (480, 320),
        (320, 240),
    ];

    #[test]
    fn every_size_builds_every_screen() {
        let playing = session();
        let paused = NowPlaying {
            playing: false,
            ..session()
        };
        let idle = NowPlaying::default();
        for (width, height) in SIZES {
            for (source, remote_ready, notice) in [
                (Source::Speaker(&playing), true, None),
                (
                    Source::Speaker(&paused),
                    true,
                    Some("iPhone didn't respond"),
                ),
                (Source::Speaker(&playing), false, None),
                (Source::Speaker(&idle), false, None),
                (Source::BridgeDown, false, None),
                (Source::Waiting, false, None),
            ] {
                let _ = widget(&View {
                    width,
                    height,
                    source,
                    now_secs: 110,
                    remote_ready,
                    notice,
                });
            }
        }
    }

    #[test]
    fn the_stacked_layouts_fit_their_viewport() {
        for (width, height) in SIZES {
            let g = geometry(width, height);
            if g.side_by_side {
                continue;
            }
            let status = if g.status_size > 0 {
                line_h(g.status_size) + g.gap
            } else {
                0.0
            };
            let progress = g.track_h + g.gap + line_h(g.time_size);
            let controls = g.play_button + g.gap * 2.0 + g.volume_button;
            let needed = 2.0 * g.padding
                + status
                + line_h(g.title_size)
                + line_h(g.artist_size)
                + progress
                + controls
                + 4.0 * g.gap;
            #[expect(clippy::cast_precision_loss, reason = "viewport sizes are small")]
            let available = height as f32;
            assert!(needed <= available, "{width}x{height} needs {needed}");
        }
    }

    #[test]
    fn the_volume_bar_is_centred_under_the_play_button_and_no_narrower() {
        for (width, height) in SIZES {
            let g = geometry(width, height);
            #[expect(clippy::cast_precision_loss, reason = "viewport sizes are small")]
            let inner_w = width as f32 - 2.0 * g.padding;
            let c = controls_layout(&g, inner_w);
            let transport = g.play_button + 2.0 * g.skip_button + 2.0 * g.gap * 3.0;
            assert!(
                transport <= c.region,
                "{width}x{height}: {transport} > {c:?}"
            );

            // Both rows are laid out from the region's left edge.
            let play_centre = c.gutter + c.region / 2.0;
            let bar = c.region - 2.0 * g.volume_button - 2.0 * (g.gap * 2.0);
            let bar_centre = c.gutter + g.volume_button + g.gap * 2.0 + bar / 2.0;
            assert!((play_centre - bar_centre).abs() < 0.01, "{width}x{height}");
            assert!(bar >= g.play_button, "{width}x{height}: bar {bar}");
            if g.side_by_side {
                assert!(c.width() + g.padding < inner_w, "{width}x{height}");
            } else {
                assert!((c.width() - inner_w).abs() < 0.01, "{width}x{height}");
            }
        }
    }

    #[test]
    fn every_command_has_its_own_tap_key() {
        let keys: Vec<String> = Command::ALL.iter().map(|&c| tap_key(c)).collect();
        for (i, key) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(key), "{key}");
        }
        assert_eq!(tap_key(Command::PlayPause), "cmd-playpause");
    }

    #[test]
    fn the_speaker_shows_more_waves_as_the_volume_rises() {
        assert_eq!(sound_waves(0.0), 0);
        assert_eq!(sound_waves(0.2), 1);
        assert_eq!(sound_waves(0.52), 2);
        assert_eq!(sound_waves(1.0), 2);
    }

    #[test]
    fn long_text_ends_in_an_ellipsis() {
        let cut = cap_to_width("Lazare & ADDAM - Muhuuuuu (Official Audio)", 150.0);
        assert!(cut.ends_with('…') && cut.chars().count() <= 20, "{cut}");
        assert_eq!(cap_to_width("Paused", 150.0), "Paused");
    }
}
