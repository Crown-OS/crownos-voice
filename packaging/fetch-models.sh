#!/bin/sh
# Installs the official DeepFilterNet3 ONNX export (used by the onnx voice backend) into
# ${1:-/usr/share/crownos/voice/models}/deepfilternet3/.
set -eu

MODELS_ROOT="${1:-/usr/share/crownos/voice/models}"
DESTINATION="$MODELS_ROOT/deepfilternet3"
ARCHIVE_URL="https://github.com/Rikorose/DeepFilterNet/raw/v0.5.6/models/DeepFilterNet3_onnx.tar.gz"
ARCHIVE_SHA256="c94d91f70911001c946e0fabb4aa9adc37045f45a03b56008cb0c8244cb63616"
ARCHIVE_DIR="tmp/export"
MODEL_FILES="enc.onnx erb_dec.onnx df_dec.onnx config.ini"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT INT TERM

curl --fail --location --retry 3 --silent --show-error --output "$workdir/model.tar.gz" "$ARCHIVE_URL"
echo "$ARCHIVE_SHA256  $workdir/model.tar.gz" | sha256sum --check --quiet -

members=""
for file in $MODEL_FILES; do
    members="$members $ARCHIVE_DIR/$file"
done
# shellcheck disable=SC2086
tar --extract --gzip --no-same-owner --file "$workdir/model.tar.gz" --directory "$workdir" $members

install -d -m 0755 "$DESTINATION"
for file in $MODEL_FILES; do
    install -m 0644 "$workdir/$ARCHIVE_DIR/$file" "$DESTINATION/$file"
done
echo "DeepFilterNet3 installed in $DESTINATION"
