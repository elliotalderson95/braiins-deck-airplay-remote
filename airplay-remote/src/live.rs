//! The entry points the host calls: the poll of the bridge's state, the
//! search for the iPhone's remote-control service, and the commands the
//! buttons send to it.

use std::cell::RefCell;
use std::time::Duration;

use bmc_wasm_sdk::mdns::{MdnsBrowse, MdnsEvent, mdns_browse};
#[expect(
    clippy::wildcard_imports,
    reason = "runtime code uses the SDK's builders, host shims and macros throughout"
)]
use bmc_wasm_sdk::*;

use crate::Params;
use crate::model::{self, BRIDGE_URL, Carry, Command, DACP_SERVICE, JsonLookup, Light, NowPlaying};
use crate::view::{self, Source, View};

/// How often the bridge is read while an iPhone is connected, and when not.
const PLAYING_POLL_MS: u32 = 1_000;
const IDLE_POLL_MS: u32 = 3_000;
/// Off screen nothing is drawn, but the light still follows the music.
const DORMANT_POLL_MS: u32 = 3_000;
const PULSE_PERIOD_MS: u32 = 3_000;
/// Delay before re-reading after a command, giving the iPhone time to report.
const REREAD_DELAY_MS: u32 = 400;
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(3);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const NOTICE_SECS: i64 = 4;
/// How long a command's anticipated effect outranks the bridge's older report.
const HOLD_SECS: i64 = 2;
/// The same for a play pressed after the iPhone ended its paused stream: it
/// takes about 4 s to start a new one.
const RESUME_HOLD_SECS: i64 = 8;

#[derive(Debug, Default, PartialEq)]
enum Bridge {
    #[default]
    Waiting,
    Down,
    Up(NowPlaying),
}

/// A remote-control service seen on the network, at its IPv4 address.
struct Remote {
    name: String,
    host: String,
    port: u16,
}

#[derive(Default)]
struct Live {
    poll: Option<PollHandle>,
    browse: Option<MdnsBrowse>,
    bridge: Bridge,
    remotes: Vec<Remote>,
    notice: Option<(String, i64)>,
    /// Until when the anticipated play state and volume stand.
    hold_until: i64,
    /// Since when the bridge has reported no session while a paused one
    /// stays on screen (see `model::carry_over`).
    idle_since: Option<i64>,
    /// What the light strip was last set to.
    light: Option<Light>,
    /// The scene is off screen (between `on_sleep` and `on_wake`).
    dormant: bool,
}

thread_local! {
    static LIVE: RefCell<Live> = RefCell::new(Live::default());
}

impl JsonLookup for JsonDoc {
    fn str(&self, path: &str) -> Option<String> {
        JsonDoc::str(self, path)
    }
    fn f64(&self, path: &str) -> Option<f64> {
        JsonDoc::f64(self, path)
    }
    fn bool(&self, path: &str) -> Option<bool> {
        JsonDoc::bool(self, path)
    }
}

impl Live {
    fn session(&self) -> Option<&NowPlaying> {
        match &self.bridge {
            Bridge::Up(np) if np.session => Some(np),
            Bridge::Up(_) | Bridge::Waiting | Bridge::Down => None,
        }
    }

    /// The remote-control service of the iPhone with this DACP id, while it
    /// announces one.
    fn remote(&self, dacp_id: &str) -> Option<&Remote> {
        self.remotes
            .iter()
            .find(|remote| model::is_remote_of(&remote.name, dacp_id))
    }

    /// Where `command` goes for the connected iPhone, and the token it wants.
    fn target(&self, command: Command) -> Option<(String, String)> {
        let np = self.session()?;
        let token = np.active_remote.clone()?;
        let remote = self.remote(np.dacp_id.as_deref()?)?;
        Some((
            model::command_url(&remote.host, remote.port, command),
            token,
        ))
    }

    /// What to show for the bridge's latest report: it, with a command's
    /// anticipated effect for a moment, and with a paused song kept on screen
    /// while the iPhone is between audio streams.
    fn follow(&mut self, mut fresh: NowPlaying, now: i64) -> NowPlaying {
        let Bridge::Up(shown) = &self.bridge else {
            self.idle_since = None;
            return fresh;
        };
        let shown = shown.clone();
        let holding = now < self.hold_until;
        if holding && fresh.session {
            fresh.playing = shown.playing;
            fresh.volume_db = shown.volume_db;
            fresh.position = shown.position;
            fresh.progress_at = shown.progress_at;
        }
        let idle_since = if fresh.session {
            None
        } else {
            Some(self.idle_since.unwrap_or(now))
        };
        let carry = Carry {
            reachable: shown
                .dacp_id
                .as_deref()
                .is_some_and(|id| self.remote(id).is_some()),
            resuming: holding && shown.playing,
            idle_secs: idle_since.map_or(0, |since| now - since),
        };
        let next = model::carry_over(&shown, fresh, carry, now);
        // Idle only counts while a paused session is kept on screen.
        self.idle_since = if next.session { idle_since } else { None };
        next
    }

    fn notify(&mut self, text: &str, now: i64) {
        self.notice = Some((String::from(text), now + NOTICE_SECS));
    }

    fn poll_interval(&self) -> u32 {
        if self.dormant {
            DORMANT_POLL_MS
        } else if self.session().is_some() {
            PLAYING_POLL_MS
        } else {
            IDLE_POLL_MS
        }
    }

    /// Lights the strip while music plays and clears it otherwise, touching
    /// the strip only when that changes. The global tier keeps the light on
    /// whichever scene is showing.
    fn sync_light(&mut self) {
        let params = Params::current();
        let speaker = match &self.bridge {
            Bridge::Up(np) => Some(np),
            Bridge::Waiting | Bridge::Down => None,
        };
        let wanted = model::light(speaker, params.playing_light, params.light_color);
        if self.light == Some(wanted) {
            return;
        }
        match wanted {
            Light::Off => led::stop(),
            Light::Steady(color) => led::set_effect_global(LedEffect::Solid, color, 0, None),
            Light::Pulse(color) => {
                led::set_effect_global(LedEffect::Breathe, color, PULSE_PERIOD_MS, None);
            }
        }
        self.light = Some(wanted);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn init() {
    let poll = register_poll(
        build,
        on_state,
        PollConfig {
            interval_ms: Some(IDLE_POLL_MS),
            retry_ms: IDLE_POLL_MS,
            debounce_ms: REREAD_DELAY_MS,
            ..Default::default()
        },
    );
    let browse = mdns_browse(&[DACP_SERVICE], on_mdns);
    if browse.is_none() {
        log_warn!("airplay-remote: the host refused the DACP browse");
    }
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.poll = Some(poll);
        live.browse = browse;
    });
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "signature must match the poll Build fn pointer, which returns Option"
)]
fn build(_handle: PollHandle) -> Option<FetchSpec> {
    Some(FetchSpec::get(BRIDGE_URL).timeout(BRIDGE_TIMEOUT))
}

fn on_state(handle: PollHandle, response: &FetchResponse) {
    let now = SystemTime::now().unix_secs;
    let fresh = response.ok().then(|| model::parse(&response.json()));
    let changed = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let next = if let Some(fresh) = fresh {
            Bridge::Up(live.follow(fresh, now))
        } else {
            live.idle_since = None;
            Bridge::Down
        };
        let changed = live.bridge != next;
        live.bridge = next;
        handle.set_interval(live.poll_interval());
        live.sync_light();
        changed
    });
    if changed {
        request_frame();
    }
}

fn on_mdns(_browse: MdnsBrowse, event: &MdnsEvent<'_>) {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        match event {
            MdnsEvent::Found(json) => {
                let doc = JsonDoc::parse(json.as_bytes());
                let (Some(name), Some(host), Some(port)) =
                    (doc.str("/name"), doc.str("/host"), doc.i64("/port"))
                else {
                    return;
                };
                // No IPv4 address, or no usable port: nothing to send commands to.
                let Ok(port) = u16::try_from(port) else {
                    return;
                };
                if host.is_empty() {
                    return;
                }
                live.remotes.retain(|remote| remote.name != name);
                live.remotes.push(Remote { name, host, port });
            }
            MdnsEvent::Removed(name) => live.remotes.retain(|remote| remote.name != *name),
        }
    });
    request_frame();
}

#[unsafe(no_mangle)]
pub extern "C" fn render(_delta_ms: u32) {
    let now = SystemTime::now().unix_secs;
    let viewport = widget_viewport();

    let result = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        if live.notice.as_ref().is_some_and(|(_, until)| now >= *until) {
            live.notice = None;
        }
        let remote_ready = live.target(Command::PlayPause).is_some();
        let source = match &live.bridge {
            Bridge::Waiting => Source::Waiting,
            Bridge::Down => Source::BridgeDown,
            Bridge::Up(np) => Source::Speaker(np),
        };
        let view = View {
            width: viewport.width,
            height: viewport.height,
            source,
            now_secs: now,
            remote_ready,
            notice: live.notice.as_ref().map(|(text, _)| text.as_str()),
        };
        render_ui(viewport.width, viewport.height, view::widget(&view))
    });

    for command in Command::ALL {
        if result.clicks.contains_key(&view::tap_key(command)) {
            send(command, now);
        }
    }

    // Tick the elapsed time while playing; otherwise wake to clear a notice.
    let (playing, notice_until) = LIVE.with(|live| {
        let live = live.borrow();
        (
            live.session().is_some_and(|np| np.playing),
            live.notice.as_ref().map(|(_, until)| *until),
        )
    });
    if playing {
        request_frame_after(1_000);
    } else if let Some(until) = notice_until {
        let secs = u32::try_from((until - now).max(1)).unwrap_or(1);
        request_frame_after(secs.saturating_mul(1_000));
    }
}

fn send(command: Command, now: i64) {
    let Some((url, token)) = LIVE.with(|live| live.borrow().target(command)) else {
        return;
    };
    if command == Command::PlayPause {
        // The speaker goes quiet (or comes back) the moment the button is
        // pressed, rather than once the iPhone's buffered audio runs out.
        let pausing = LIVE.with(|live| live.borrow().session().is_some_and(|np| np.playing));
        let hush = if pausing {
            model::HUSH_URL
        } else {
            model::UNHUSH_URL
        };
        if FetchRequest::get(hush)
            .timeout(BRIDGE_TIMEOUT)
            .send(on_hush_reply)
            .is_none()
        {
            log_warn!("airplay-remote: the host did not queue the speaker hush");
        }
    }
    let header = fmt!("Active-Remote: {}", token.as_str());
    let sent = FetchRequest::get(&url)
        .headers(&header)
        .timeout(COMMAND_TIMEOUT)
        .send(on_command_reply);
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        if sent.is_none() {
            live.notify("Couldn't send", now);
            return;
        }
        if let Bridge::Up(np) = &mut live.bridge {
            np.anticipate(command, now);
        }
        live.hold_until = now
            + if command == Command::PlayPause && live.idle_since.is_some() {
                RESUME_HOLD_SECS
            } else {
                HOLD_SECS
            };
        live.sync_light();
    });
    request_frame();
}

fn on_hush_reply(response: &FetchResponse) {
    if !response.ok() {
        log_warn!(
            "airplay-remote: the bridge answered a speaker hush with status {}",
            response.status
        );
    }
}

fn on_command_reply(response: &FetchResponse) {
    let now = SystemTime::now().unix_secs;
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        if !response.ok() {
            log_warn!(
                "airplay-remote: the iPhone answered a command with status {}",
                response.status
            );
            live.notify("iPhone didn't respond", now);
            live.hold_until = 0;
        }
        // The iPhone reports its new state to the speaker; read it back.
        if let Some(poll) = live.poll {
            poll.invalidate();
        }
    });
    request_frame();
}

/// The host renders on touch only when asked; taps are read in `render`.
#[unsafe(no_mangle)]
pub extern "C" fn on_touch() {
    request_frame();
}

/// Off screen, read the bridge less often: enough for the light to follow
/// the music, which it does whichever scene is showing.
#[unsafe(no_mangle)]
pub extern "C" fn on_sleep() {
    set_dormant(true);
}

/// Back on screen: read at the full rate, and at once.
#[unsafe(no_mangle)]
pub extern "C" fn on_wake() {
    set_dormant(false);
}

fn set_dormant(dormant: bool) {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.dormant = dormant;
        if let Some(poll) = live.poll {
            poll.set_interval(live.poll_interval());
            if !dormant {
                poll.invalidate();
            }
        }
    });
}

/// The light's style or colour changed in the settings.
#[unsafe(no_mangle)]
pub extern "C" fn on_params_update() {
    LIVE.with(|live| live.borrow_mut().sync_light());
}
