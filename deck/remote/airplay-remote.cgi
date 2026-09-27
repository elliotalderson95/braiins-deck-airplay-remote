#!/bin/sh
# What the Deck's AirPlay speaker is playing, for the AirPlay Remote widget.
# Answers the Deck itself only: the state carries the token that lets its
# holder pause, skip and change the volume on the iPhone that is playing.
#
#   ?hush=on    mute the speaker now, for the buffered tail of a pause
#   ?hush=off   unmute it now

case "$REMOTE_ADDR" in
127.0.0.1 | ::1 | ::ffff:127.0.0.1) ;;
*)
	printf 'Status: 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nforbidden\n'
	exit 0
	;;
esac

case "$QUERY_STRING" in
hush=on) /usr/share/airplay-remote/hush on ;;
hush=off) /usr/share/airplay-remote/hush off ;;
esac

printf 'Content-Type: application/json\r\nCache-Control: no-store\r\n\r\n'
cat /tmp/airplay-remote/state.json 2>/dev/null || printf '{"session":false,"playing":false}\n'
