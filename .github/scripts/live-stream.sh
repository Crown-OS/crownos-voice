#!/bin/bash
# Opens an enhanced microphone on a headless PipeWire with a virtual source and records it,
# checking that frames flow and that the stream never misses its deadlines.
# Usage: live-stream.sh <release dir> <work dir>; run under dbus-run-session.
set -euo pipefail

RELEASE_DIR="$1"
WORK_DIR="$2"
SOURCE_NAME="ci-microphone"
HOLD_SECONDS=20
RECORD_SECONDS=8
SAMPLE_RATE=48000
MIN_RECORDED_BYTES=$((RECORD_SECONDS / 2 * SAMPLE_RATE * 4))
PROBLEMS='misses deadlines|demoted|backend failed a frame|could not open a stream'

mkdir -p "$WORK_DIR/backends" "$WORK_DIR/runtime"
chmod 700 "$WORK_DIR/runtime"
export XDG_RUNTIME_DIR="$WORK_DIR/runtime"
export XDG_CACHE_HOME="$WORK_DIR/cache"
cp "$RELEASE_DIR/libvoice_cpu.so" "$WORK_DIR/backends/"

cleanup() { kill $(jobs -p) 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT

wait_for() {
    for _ in $(seq 100); do "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done
    echo "timed out waiting for: $*" >&2
    return 1
}

pipewire >"$WORK_DIR/pipewire.log" 2>&1 &
wait_for pw-cli info 0
wireplumber >"$WORK_DIR/wireplumber.log" 2>&1 &
pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$SOURCE_NAME
    media.class=Audio/Source/Virtual audio.position=[ MONO ] object.linger=true }"
wait_for sh -c "pw-cli ls Node | grep -q '\"$SOURCE_NAME\"'"

CROWN_VOICE_BACKENDS="$WORK_DIR/backends" RUST_LOG=info \
    "$RELEASE_DIR/crown-voiced" >"$WORK_DIR/daemon.log" 2>&1 &
wait_for test -S "$XDG_RUNTIME_DIR/crownos/crownos-voice.sock"

"$RELEASE_DIR/examples/open_stream" "$SOURCE_NAME" "$HOLD_SECONDS" standard \
    >"$WORK_DIR/stream.log" 2>&1 &
wait_for grep -q 'node_name' "$WORK_DIR/stream.log"
node=$(grep -o 'node_name: "[^"]*"' "$WORK_DIR/stream.log" | cut -d'"' -f2)
echo "enhanced node: $(head -n1 "$WORK_DIR/stream.log")"

timeout --signal=INT "$RECORD_SECONDS" pw-record --target "$node" --rate "$SAMPLE_RATE" \
    --channels 1 --format f32 "$WORK_DIR/recorded.wav" || true
recorded=$(stat --format=%s "$WORK_DIR/recorded.wav")
echo "recorded $recorded bytes from $node in $RECORD_SECONDS s"
pw-top --batch-mode --iterations 2 | grep -E 'ERR|crownos-voice|ci-microphone' || true

status=0
if ((recorded < MIN_RECORDED_BYTES)); then
    echo "::error::only $recorded bytes flowed out of $node"
    status=1
fi
if grep -Eq "$PROBLEMS" "$WORK_DIR/daemon.log"; then
    echo "::error::the live stream reported problems"
    status=1
fi
echo "--- crown-voiced"
cat "$WORK_DIR/daemon.log"
exit "$status"
