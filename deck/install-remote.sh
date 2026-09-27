#!/usr/bin/env bash
# Install the AirPlay Remote bridge on a Braiins Deck. It follows the AirPlay
# speaker's song info and serves it, to the Deck itself only, for the
# AirPlay Remote widget.
#
# Usage: deck/install-remote.sh <deck-ip>
#
# Needs the AirPlay speaker (deck/install.sh) first. Safe to re-run.
set -euo pipefail

DECK_IP="${1:?usage: deck/install-remote.sh <deck-ip>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$DECK_IP")

echo "==> Checking the Deck at $DECK_IP"
if ! "${SSH[@]}" true; then
	echo "Cannot log in to root@$DECK_IP; set up key login once with: ssh-copy-id root@$DECK_IP" >&2
	exit 1
fi
if ! "${SSH[@]}" 'test -x /etc/init.d/shairport-sync'; then
	echo "The AirPlay speaker is not installed; run deck/install.sh $DECK_IP first." >&2
	exit 1
fi

echo "==> Copying the bridge"
"${SSH[@]}" 'mkdir -p /usr/share/airplay-remote && cat > /usr/share/airplay-remote/bridge.lua' <"$HERE/remote/bridge.lua"
"${SSH[@]}" 'cat > /usr/share/airplay-remote/hush && chmod 755 /usr/share/airplay-remote/hush' <"$HERE/remote/hush"
"${SSH[@]}" 'cat > /etc/init.d/airplay-remote && chmod 755 /etc/init.d/airplay-remote' <"$HERE/remote/airplay-remote.init"
"${SSH[@]}" 'cat > /www/cgi-bin/airplay-remote && chmod 755 /www/cgi-bin/airplay-remote' <"$HERE/remote/airplay-remote.cgi"

echo "==> Starting it"
"${SSH[@]}" sh -s <<'EOF'
set -e
/etc/init.d/airplay-remote enable
/etc/init.d/airplay-remote restart
sleep 2
if ! ps -eo args | grep -q '[b]ridge.lua'; then
	echo "the bridge did not start; recent log:" >&2
	logread | tail -20 >&2
	exit 1
fi
answer=$(wget -qO- http://127.0.0.1:8000/cgi-bin/airplay-remote)
case "$answer" in
'{"session":'*) ;;
*)
	echo "the bridge's page did not answer as expected: $answer" >&2
	exit 1
	;;
esac
EOF

echo "==> Done. If the widget is not on the Deck yet, install it with: ./deploy.sh $DECK_IP"
