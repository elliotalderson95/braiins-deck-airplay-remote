#!/usr/bin/env python3
"""A stand-in for the Deck's AirPlay remote bridge and an iPhone's remote
control (DACP) service, for trying the AirPlay Remote widget on a Mac.

    tools/mock-airplay.py [--port 8766]
    dns-sd -P iTunes_Ctrl_0123456789ABCDEF _dacp._tcp local 8766 mock-iphone.local 127.0.0.1

Then run the testbed with `--rewrite-url http://127.0.0.1:8000=http://127.0.0.1:8766`.

GET /cgi-bin/airplay-remote   the bridge's state, advancing like real playback
GET /ctrl-int/1/<command>     playpause, nextitem, previtem, volumeup, volumedown;
                              needs the Active-Remote token, as an iPhone does

Like an iPhone, it ends its audio stream a few seconds into a pause, which
the bridge reports as no session, and starts a new one a few seconds after
play, whose song details follow a moment later.
"""

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DACP_ID = "0123456789ABCDEF"
TOKEN = "1234567890"
VOLUME_STEP = 30 / 16
TRACKS = [
    ("Midnight Drive", "Neon Harbor", 214.0),
    ("Low Tide", "Coastline Radio", 187.0),
    ("Signal Fire", "The Northern Lights Orchestra and Friends", 263.0),
]

# As measured with an iPhone playing YouTube in Brave.
STREAM_END_SECS = 6.0
STREAM_START_SECS = 4.0
DETAILS_DELAY_SECS = 1.0

state = {
    "track": 0,
    "playing": True,
    "volume": -12.0,
    "position": 42.0,
    "at": time.time(),
    # When a pause ends the audio stream, and when a play starts a new one.
    "stream_ends_at": None,
    "stream_starts_at": None,
}


def position_now() -> float:
    elapsed = time.time() - state["at"] if state["playing"] else 0.0
    return min(state["position"] + elapsed, TRACKS[state["track"]][2])


def snapshot() -> dict:
    title, artist, duration = TRACKS[state["track"]]
    now = time.time()
    ends, starts = state["stream_ends_at"], state["stream_starts_at"]
    if (ends is not None and now >= ends) or (starts is not None and now < starts):
        # What the bridge reports between streams.
        return {"session": False, "playing": False, "updated_at": int(now)}
    snap = {
        "session": True,
        "playing": state["playing"],
        "title": title,
        "artist": artist,
        "album": None,
        "volume": round(state["volume"], 2),
        "position": round(position_now(), 2),
        "duration": duration,
        "progress_at": int(now),
        "dacp_id": DACP_ID,
        "active_remote": TOKEN,
        "updated_at": int(now),
    }
    if starts is not None and now < starts + DETAILS_DELAY_SECS:
        for field in ("title", "artist", "position", "duration", "progress_at"):
            snap[field] = None
    return snap


def command(name: str) -> bool:
    if name == "playpause":
        now = time.time()
        state["position"], state["at"] = position_now(), now
        state["playing"] = not state["playing"]
        ended = state["stream_ends_at"] is not None and now >= state["stream_ends_at"]
        state["stream_starts_at"] = now + STREAM_START_SECS if ended else None
        state["stream_ends_at"] = None if state["playing"] else now + STREAM_END_SECS
    elif name in ("volumeup", "volumedown"):
        step = VOLUME_STEP if name == "volumeup" else -VOLUME_STEP
        state["volume"] = max(-30.0, min(0.0, state["volume"] + step))
    elif name in ("nextitem", "previtem"):
        state["track"] = (state["track"] + (1 if name == "nextitem" else -1)) % len(TRACKS)
        state["position"], state["at"] = 0.0, time.time()
    else:
        return False
    return True


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/cgi-bin/airplay-remote":
            self._send(200, json.dumps(snapshot()).encode())
            return
        if self.path.startswith("/ctrl-int/1/"):
            name = self.path.rsplit("/", 1)[-1]
            if self.headers.get("Active-Remote") != TOKEN:
                print(f"{time.strftime('%H:%M:%S')} {name}: wrong or missing Active-Remote", flush=True)
                self._send(403, b"")
            elif command(name):
                print(f"{time.strftime('%H:%M:%S')} {name} -> {TRACKS[state['track']][0]}, "
                      f"{'playing' if state['playing'] else 'paused'}, volume {state['volume']:.2f}", flush=True)
                self._send(204, b"")
            else:
                self._send(404, b"")
            return
        self._send(404, b"")

    def _send(self, status: int, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args) -> None:
        pass  # commands print their own line; state polls would flood it


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=8766)
    args = parser.parse_args()
    print(f"mock AirPlay bridge + DACP on http://127.0.0.1:{args.port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
