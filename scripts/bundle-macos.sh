#!/usr/bin/env bash
set -euo pipefail

release="target/${TARGET:?}/release"
cli="$release/diffkin"
mode=${1:?expected ad-hoc or developer-id}
case "$mode" in
  ad-hoc) export APPLE_SIGNING_IDENTITY=- ;;
  developer-id)
    for key in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
      [[ -n ${!key:-} ]] || { echo "Missing signing configuration: $key" >&2; exit 1; }
    done
    ;;
  *) echo "Unknown macOS signing mode: $mode" >&2; exit 1 ;;
esac

# Tauri signs the app before creating the DMG. With Developer ID it also
# notarizes and staples the app, then signs the enclosing disk image.
pnpm desktop bundle --target "$TARGET" --bundles dmg
images=("$release"/bundle/dmg/*.dmg)
[[ ${#images[@]} == 1 && -f ${images[0]} ]] || { echo "Expected one DMG" >&2; exit 1; }
work=$(mktemp -d)
volume="$work/volume"
keychain="$work/cli.keychain-db"
mounted=false
cleanup() {
  if [[ -f $keychain ]]; then security delete-keychain "$keychain"; fi
  if [[ $mounted == true ]]; then hdiutil detach "$volume"; fi
  rm -rf "$work"
}
trap cleanup EXIT

# Tauri removes the intermediate .app for a DMG-only build. Check the copy
# users will install, including its resource seal and any stapled ticket.
mkdir "$volume"
# The image includes the project's MIT license and prompts on stdin.
hdiutil attach -readonly -nobrowse -mountpoint "$volume" "${images[0]}" <<< Y
mounted=true
app="$volume/Diffkin.app"
test -s "$app/Contents/_CodeSignature/CodeResources"
codesign --verify --deep --strict "$app"

if [[ $mode == ad-hoc ]]; then
  codesign --force --sign - "$cli"
  codesign --verify --strict "$cli"
  exit 0
fi

xcrun stapler validate "$app"
codesign --verify --strict "${images[0]}"

# Tauri's temporary signing keychain is gone after bundling. Import the same
# identity into a temporary keychain for the standalone CLI.
password=$(openssl rand -hex 32)
printf '%s' "$APPLE_CERTIFICATE" | base64 -D > "$work/certificate.p12"
security create-keychain -p "$password" "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$work/certificate.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
rm "$work/certificate.p12"
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
codesign --force --sign "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" --options runtime --timestamp "$cli"
security delete-keychain "$keychain"
codesign --verify --strict "$cli"

# The notary service accepts ZIP, while the release ships the same Mach-O in
# tar.gz. Gatekeeper can look up its ticket by its signed code hash.
ditto -c -k --norsrc "$cli" "$work/cli.zip"
xcrun notarytool submit "$work/cli.zip" --wait --output-format json \
  --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" | tee "$work/notary.json"
jq -e '.status == "Accepted"' "$work/notary.json" >/dev/null
