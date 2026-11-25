#!/bin/sh
# Linux OCR models for ocrs, bundled as resources (tauri.linux.conf.json). Run before a Linux build.
set -eu
dir="$(dirname "$0")/../src-tauri/ocr-models"
mkdir -p "$dir"
for m in text-detection text-recognition; do
  [ -s "$dir/$m.onnx" ] || curl -fsSL -o "$dir/$m.onnx" "https://ocrs-models.s3-accelerate.amazonaws.com/$m.onnx"
done
