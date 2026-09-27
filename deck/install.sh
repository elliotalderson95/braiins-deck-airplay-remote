#!/usr/bin/env bash
# Turn a Braiins Deck into an AirPlay speaker named "Braiins Deck".
#
# Usage: deck/install.sh <deck-ip>
#
# Safe to re-run, e.g. after a Deck firmware update removed it. Needs SSH key
# login to the Deck (once: ssh-copy-id root@<deck-ip>).
set -euo pipefail

DECK_IP="${1:?usage: deck/install.sh <deck-ip>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
PKG_DIR="$HERE/packages"
FEED="https://downloads.openwrt.org/releases/22.03.4/packages/arm_cortex-a7_neon-vfpv4"
REMOTE_DIR=/tmp/deck-airplay
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$DECK_IP")

# "<feed> <file> <sha256>", pinned from the OpenWrt 22.03.4 package index —
# the release and CPU architecture the Deck's own firmware is built on.
PACKAGES=(
	"packages libconfig11_1.7.3-1_arm_cortex-a7_neon-vfpv4.ipk f7caf76c669f0d5897c18e3e7063512b5c5c80c9919aada304fec07579ae857f"
	"packages libdaemon_0.14-5_arm_cortex-a7_neon-vfpv4.ipk 4e6d5bd23caa7040699ffe1a90c853e32628f42eaf81df02b31216a092228b3e"
	"base libmbedtls12_2.28.8-1_arm_cortex-a7_neon-vfpv4.ipk 3d446b65fd3f6abd7c2ac4b8a8daf0e1b660589c5d5710c91a140dad237e3387"
	"packages shairport-sync-mini_3.3.9-2_arm_cortex-a7_neon-vfpv4.ipk 14d5cd5847e27f06e20c50153f094de320fde9d5d63503897f2adf1354c2f55f"
)

echo "==> Checking the Deck at $DECK_IP"
if ! "${SSH[@]}" true; then
	echo "Cannot log in to root@$DECK_IP. Check the IP (swipe down on the Deck's screen)" >&2
	echo "and set up key login once with: ssh-copy-id root@$DECK_IP" >&2
	exit 1
fi

echo "==> Fetching and verifying the official OpenWrt packages"
mkdir -p "$PKG_DIR"
for entry in "${PACKAGES[@]}"; do
	read -r feed file sha <<<"$entry"
	if [ ! -f "$PKG_DIR/$file" ]; then
		curl -fsSL --retry 3 -o "$PKG_DIR/$file.part" "$FEED/$feed/$file"
		mv "$PKG_DIR/$file.part" "$PKG_DIR/$file"
	fi
	if ! echo "$sha  $PKG_DIR/$file" | shasum -a 256 -c --quiet -; then
		rm -f "$PKG_DIR/$file"
		echo "Checksum mismatch for $file; refusing to install it." >&2
		exit 1
	fi
done

echo "==> Copying to the Deck"
"${SSH[@]}" "rm -rf $REMOTE_DIR && mkdir -p $REMOTE_DIR"
for entry in "${PACKAGES[@]}"; do
	read -r _ file _ <<<"$entry"
	"${SSH[@]}" "cat > $REMOTE_DIR/$file" <"$PKG_DIR/$file"
done
"${SSH[@]}" "cat > $REMOTE_DIR/shairport-sync.conf" <"$HERE/shairport-sync.conf"
"${SSH[@]}" "cat > $REMOTE_DIR/alsa-airplay.conf" <"$HERE/alsa-airplay.conf"
"${SSH[@]}" "cat > $REMOTE_DIR/99-deck-airplay" <"$HERE/99-deck-airplay"

echo "==> Installing and starting the AirPlay receiver"
"${SSH[@]}" sh -s <<'EOF'
set -e
cd /tmp/deck-airplay
opkg install ./libconfig11_*.ipk ./libdaemon_*.ipk ./libmbedtls12_*.ipk ./shairport-sync-mini_*.ipk

[ -f /etc/shairport-sync.conf.orig ] || cp /etc/shairport-sync.conf /etc/shairport-sync.conf.orig
cp shairport-sync.conf /etc/shairport-sync.conf
mkdir -p /etc/alsa/conf.d
cp alsa-airplay.conf /etc/alsa/conf.d/50-airplay.conf
mkdir -p /etc/hotplug.d/iface
cp 99-deck-airplay /etc/hotplug.d/iface/99-deck-airplay

uci set shairport-sync.shairport_sync_file.enabled='1'
uci set shairport-sync.shairport_sync.enabled='0'
uci commit shairport-sync
/etc/init.d/shairport-sync enable
/etc/init.d/shairport-sync restart

cd / && rm -rf /tmp/deck-airplay
sleep 3
if ! pidof shairport-sync >/dev/null || ! netstat -lnt | grep -q ':5000 '; then
	echo "shairport-sync did not come up; recent log:" >&2
	logread | grep -i shairport | tail -20 >&2
	exit 1
fi
EOF

echo "==> Done. On your iPhone, pick \"Braiins Deck\" as the AirPlay speaker."
