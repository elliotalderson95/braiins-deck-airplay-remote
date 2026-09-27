// AUTO-GENERATED FROM ../manifest.json by `bmc-widget-codegen` v0.1.0.
// Do not edit by hand. Run `just wasm::gen <widget>` after changing the manifest.

#![allow(
    dead_code,
    reason = "fields are widget-specific; not every key is used by every render path"
)]

use bmc_wasm_sdk::params as snapshot;
use bmc_wasm_sdk::params::typed::ParamRead;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightColor {
    Violet,
    WarmWhite,
    Blue,
    Green,
    Orange,
}
impl LightColor {
    /// Every variant, in manifest-declaration order. Useful when a widget
    /// wants to render a "pick one" UI or audit the enum exhaustively.
    pub const ALL: &'static [Self] = &[
        Self::Violet,
        Self::WarmWhite,
        Self::Blue,
        Self::Green,
        Self::Orange,
    ];
    /// Manifest wire value for this variant.
    #[must_use]
    pub fn as_manifest_value(self) -> &'static str {
        match self {
            Self::Violet => "violet",
            Self::WarmWhite => "warm-white",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Orange => "orange",
        }
    }
    /// Human-readable label declared in the manifest's `enum_values`.
    #[must_use]
    pub fn as_manifest_label(self) -> &'static str {
        match self {
            Self::Violet => "Violet",
            Self::WarmWhite => "Warm white",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Orange => "Orange",
        }
    }
    #[must_use]
    pub fn from_manifest_value(s: &str) -> Option<Self> {
        match s {
            "violet" => Some(Self::Violet),
            "warm-white" => Some(Self::WarmWhite),
            "blue" => Some(Self::Blue),
            "green" => Some(Self::Green),
            "orange" => Some(Self::Orange),
            _ => None,
        }
    }
}
bmc_wasm_sdk::impl_manifest_str_enum!(LightColor);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayingLight {
    Steady,
    Pulse,
    Off,
}
impl PlayingLight {
    /// Every variant, in manifest-declaration order. Useful when a widget
    /// wants to render a "pick one" UI or audit the enum exhaustively.
    pub const ALL: &'static [Self] = &[Self::Steady, Self::Pulse, Self::Off];
    /// Manifest wire value for this variant.
    #[must_use]
    pub fn as_manifest_value(self) -> &'static str {
        match self {
            Self::Steady => "steady",
            Self::Pulse => "pulse",
            Self::Off => "off",
        }
    }
    /// Human-readable label declared in the manifest's `enum_values`.
    #[must_use]
    pub fn as_manifest_label(self) -> &'static str {
        match self {
            Self::Steady => "Steady",
            Self::Pulse => "Slow pulse",
            Self::Off => "Off (leave the light alone)",
        }
    }
    #[must_use]
    pub fn from_manifest_value(s: &str) -> Option<Self> {
        match s {
            "steady" => Some(Self::Steady),
            "pulse" => Some(Self::Pulse),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}
bmc_wasm_sdk::impl_manifest_str_enum!(PlayingLight);
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub light_color: Option<LightColor>,
    pub playing_light: Option<PlayingLight>,
}
impl Params {
    /// Materialise a typed snapshot from a dynamic [`snapshot::Params`].
    #[must_use]
    pub fn from_snapshot(snap: &snapshot::Params) -> Self {
        Self {
            light_color: <LightColor as ParamRead>::read_optional(snap, "light_color"),
            playing_light: <PlayingLight as ParamRead>::read_optional(snap, "playing_light"),
        }
    }
    /// Latest typed snapshot delivered for this widget instance.
    /// Cached per-thread; only re-parses when `snapshot::version()` changes
    /// since the last call.
    #[must_use]
    pub fn current() -> Self {
        thread_local! {
            static CACHE : core::cell::RefCell < Option < (u64, Params) >> = const {
            core::cell::RefCell::new(None) };
        }
        let v = snapshot::version();
        CACHE.with(|cell| {
            let mut cache = cell.borrow_mut();
            if let Some((cv, ref params)) = *cache
                && cv == v
            {
                return params.clone();
            }
            let fresh = Self::from_snapshot(&snapshot::current());
            *cache = Some((v, fresh.clone()));
            fresh
        })
    }
    /// Snapshot delivered immediately before [`current`]; `None` until at
    /// least one update has been observed (i.e. during `init` and the
    /// first `render`).
    #[must_use]
    pub fn previous() -> Option<Self> {
        let prev = snapshot::previous();
        if prev.is_empty() {
            None
        } else {
            Some(Self::from_snapshot(&prev))
        }
    }
    /// Manifest keys whose value differs between `self` and `other`.
    ///
    /// Intended for `on_params_update` diffing — pass `current()` and the
    /// inside-hook value of `previous()` to get the set of keys to react
    /// to. Field-by-field `PartialEq`; emitted in struct-field order so
    /// the result is deterministic.
    #[must_use]
    pub fn changed_keys(&self, other: &Self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.light_color != other.light_color {
            out.push("light_color");
        }
        if self.playing_light != other.playing_light {
            out.push("playing_light");
        }
        out
    }
}
