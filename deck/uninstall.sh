#!/usr/bin/env bash
# Remove the AirPlay speaker from a Braiins Deck, leaving the Deck as it was.
#
# Usage: deck/uninstall.sh <deck-ip>
set -euo pipefail

DECK_IP="${1:?usage: deck/uninstall.sh <deck-ip>}"

ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$DECK_IP" sh -s <<'EOF'
/etc/init.d/shairport-sync stop 2>/dev/null
/etc/init.d/shairport-sync disable 2>/dev/null
rm -f /etc/hotplug.d/iface/99-deck-airplay
opkg remove shairport-sync-mini
opkg remove libconfig11 libdaemon libmbedtls12
rm -f /etc/shairport-sync.conf /etc/shairport-sync.conf.orig /etc/config/shairport-sync /tmp/shairport-sync-metadata
rm -f /etc/alsa/conf.d/50-airplay.conf
rmdir /etc/alsa/conf.d /etc/alsa 2>/dev/null || true
EOF

echo "AirPlay speaker removed from the Deck at $DECK_IP."
