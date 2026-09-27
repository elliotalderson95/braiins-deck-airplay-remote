#!/usr/bin/env bash
# Remove the AirPlay Remote bridge from a Braiins Deck (the speaker stays).
#
# Usage: deck/uninstall-remote.sh <deck-ip>
set -euo pipefail

DECK_IP="${1:?usage: deck/uninstall-remote.sh <deck-ip>}"

ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$DECK_IP" sh -s <<'EOF'
/etc/init.d/airplay-remote stop 2>/dev/null
/etc/init.d/airplay-remote disable 2>/dev/null
rm -f /etc/init.d/airplay-remote /www/cgi-bin/airplay-remote
rm -rf /usr/share/airplay-remote /tmp/airplay-remote
EOF

echo "AirPlay Remote bridge removed from the Deck at $DECK_IP."
