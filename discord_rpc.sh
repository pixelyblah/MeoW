#!/bin/sh
# Requires: socat

CLIENT_ID="1545372090859921438"
PIPE=""
RUNNING=1

cleanup() {
    RUNNING=0
    clear_activity
    echo "[*] Goodbye"
    exit 0
}

trap cleanup INT TERM

find_socket() {
    dir="${XDG_RUNTIME_DIR:-/tmp}"
    i=0
    while [ $i -lt 10 ]; do
        sock="$dir/discord-ipc-$i"
        if [ -S "$sock" ]; then
            echo "$sock"
            return 0
        fi
        i=$((i + 1))
    done
    return 1
}

le32() {
    v=$1
    printf "\\$(printf '%03o' $((v & 0xFF)))"
    printf "\\$(printf '%03o' $(((v >> 8) & 0xFF)))"
    printf "\\$(printf '%03o' $(((v >> 16) & 0xFF)))"
    printf "\\$(printf '%03o' $(((v >> 24) & 0xFF)))"
}

send_frame() {
    opcode=$1
    payload=$2
    len=${#payload}
    header=""
    i=0
    v=$opcode
    while [ $i -lt 4 ]; do
        byte=$((v & 0xFF))
        header="${header}$(printf "\\$(printf '%03o' $byte)")"
        v=$((v >> 8))
        i=$((i + 1))
    done
    v=$len
    i=0
    while [ $i -lt 4 ]; do
        byte=$((v & 0xFF))
        header="${header}$(printf "\\$(printf '%03o' $byte)")"
        v=$((v >> 8))
        i=$((i + 1))
    done
    printf '%b%s' "$header" "$payload" | socat - UNIX-CONNECT:"$PIPE" 2>/dev/null
}

send_json() {
    opcode=$1
    json=$2
    send_frame "$opcode" "$json"
}

send_handshake() {
    send_json 0 "{\"v\":1,\"client_id\":\"${CLIENT_ID}\"}"
}

set_activity() {
    state="$1"
    details="$2"
    large_text="$3"
    small_text="$4"
    button_label="${5:-}"
    button_url="${6:-}"
    nonce="$(date +%s%N$$)"

    buttons=""
    if [ -n "$button_label" ] && [ -n "$button_url" ]; then
        buttons=",\"buttons\":[{\"label\":\"${button_label}\",\"url\":\"${button_url}\"}]"
    fi

    json="{\"nonce\":\"${nonce}\",\"cmd\":\"SET_ACTIVITY\",\"args\":{\"pid\":$$,\"activity\":{\"state\":\"${state}\",\"details\":\"${details}\",\"assets\":{\"large_text\":\"${large_text}\",\"small_text\":\"${small_text}\"}${buttons}}}}"
    send_json 1 "$json"
    echo "[*] Activity set: $state - $details"
}

clear_activity() {
    nonce="$(date +%s%N$$)"
    send_json 1 "{\"nonce\":\"${nonce}\",\"cmd\":\"SET_ACTIVITY\",\"args\":{\"pid\":$$,\"activity\":null}}"
}

# Main
if [ -z "$CLIENT_ID" ] || [ "$CLIENT_ID" = "YOUR_APP_ID_HERE" ]; then
    echo "[!] Set CLIENT_ID in discord_rpc.sh"
    exit 1
fi

if ! command -v socat >/dev/null 2>&1; then
    echo "[!] socat required. Install: sudo apt install socat"
    exit 1
fi

PIPE=$(find_socket) || { echo "[!] Discord not found. Is it running?"; exit 1; }
echo "[*] Socket: $PIPE"

send_handshake
echo "[*] Handshake sent"

state="${1:-Playing a game}"
details="${2:-In the terminal}"
ltext="${3:-Large}"
stext="${4:-Small}"
blabel="${5:-}"
burl="${6:-}"

set_activity "$state" "$details" "$ltext" "$stext" "$blabel" "$burl"

while [ "$RUNNING" = "1" ]; do
    sleep 15
    set_activity "$state" "$details" "$ltext" "$stext" "$blabel" "$burl"
done
