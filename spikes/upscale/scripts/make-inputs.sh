#!/usr/bin/env bash
# Build the spike's test images in $1 (default: ./inputs).
#
# - screen.png: one `screencapture -x` of the main (Retina) display, cropped into
#   UI text and icon regions. Recapture it yourself; crops assume a 3600x2338 screen.
# - code_*.png: a Rust file rendered by headless Chrome at device scale 1, 2, 4 and 8.
#   Scale 1 is a 1x capture, scale 2 a Retina capture; the larger ones are the
#   ground truth that each upscaler is scored against.
# - photo_*.png: a crop of a sample photo that ships with the iOS Simulator
#   runtime (Xcode), downscaled so a full-resolution reference exists.
set -euo pipefail

out=${1:-inputs}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
mkdir -p "$out"
cd "$out"

[ -f screen.png ] || screencapture -x screen.png

crop() { # src dst x y w h
  sips -c "$6" "$5" --cropOffset "$4" "$3" "$1" --out "$2" >/dev/null
}
# Retina terminal text (a table) and dock icons.
crop screen.png ui_text_1000.png 40 540 1000 700
crop screen.png ui_text_300.png 60 700 300 200
crop screen.png icons_1000.png 1090 2130 1000 200
crop screen.png icons_300.png 1300 2130 300 200

python3 -I "$here/code_html.py" "$repo/src-tauri/src/region.rs" 60 code.html
render() { # scale width height out
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --no-first-run \
    --user-data-dir="$PWD/chrome-profile" --force-device-scale-factor="$1" \
    --window-size="$2,$3" --screenshot="$PWD/$4" "file://$PWD/code.html" 2>/dev/null
}
# 1x capture of 1000x700 and its 2x/4x truth.
render 1 1000 700 code_1x_1000.png
render 2 1000 700 code_1x_1000_ref2.png
render 4 1000 700 code_1x_1000_ref4.png
# 1x capture of 300x200.
render 1 300 200 code_1x_300.png
render 2 300 200 code_1x_300_ref2.png
render 4 300 200 code_1x_300_ref4.png
# Retina capture: 500x350 points = 1000x700 pixels, truth at 4x and 8x.
render 2 500 350 code_2x_1000.png
render 4 500 350 code_2x_1000_ref2.png
render 8 500 350 code_2x_1000_ref4.png
rm -rf chrome-profile

# Photo: a 4000x2800 crop, downscaled to 1000x700 (input) and 2000x1400 (2x truth).
photo=$(find /Library/Developer/CoreSimulator/Volumes -path '*SampleContent/Media/DCIM/100APPLE/IMG_0002.JPG' 2>/dev/null | head -1 || true)
sips -s format png "$photo" --out sample.png >/dev/null
crop sample.png photo_1000_ref4.png 144 24 4000 2800
sips -z 1400 2000 photo_1000_ref4.png --out photo_1000_ref2.png >/dev/null
sips -z 700 1000 photo_1000_ref4.png --out photo_1000.png >/dev/null
crop photo_1000.png photo_300.png 350 250 300 200
rm sample.png
