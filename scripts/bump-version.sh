#!/usr/bin/env bash
#
# Single source of truth for Link All's release version.
#
# Every shippable artifact carries its own version string, and they all have to
# agree: a tag that says v1.3.0 must not produce an APK labelled 1.2.4. That is
# exactly the drift that accumulated across 1.2.5-1.2.8. The previous Python
# version of this script pointed at platforms/windows/LinkAll.Windows/, a path
# that does not exist, and skipped it silently because the write was guarded by
# an existence check. Nothing verified the result afterwards, so the drift was
# invisible until release time.
#
# Two things prevent a repeat: this script owns every version site, and
# --check fails the build when they disagree.
#
# The three Rust crates inherit from [workspace.package] in the root
# Cargo.toml, so they share a single site rather than needing one each.
#
# Usage:
#   bump-version.sh --current     print just the version, for scripts
#   bump-version.sh --show         print every artifact's current version
#   bump-version.sh --check        exit 1 if the versions disagree (CI gate)
#   bump-version.sh --set 1.3.0    set an explicit version everywhere
#   bump-version.sh --patch        1.2.8 -> 1.2.9
#   bump-version.sh --minor        1.2.8 -> 1.3.0
#   bump-version.sh --major        1.2.8 -> 2.0.0
#
# Android's versionCode and macOS's CFBundleVersion are one shared build
# counter. Any version change increments it, so every release is a fresh
# upload on both stores.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CARGO="$ROOT/Cargo.toml"
GRADLE="$ROOT/platforms/android/app/build.gradle"
WXS="$ROOT/platforms/windows/installer/LinkAll.wxs"
CSPROJ="$ROOT/platforms/windows/LinkAll.WinUI/LinkAll.WinUI.csproj"
PLISTS=(
  "$ROOT/platforms/macos/LinkAll/Info.plist"
  "$ROOT/platforms/macos/ShareExtension/Info.plist"
  "$ROOT/platforms/macos/VirtualCamera/Info.plist"
)

# ---- readers ---------------------------------------------------------------

plist_get() { # <file> <key>
  awk -v k="$2" '
    $0 ~ "<key>" k "</key>" { want = 1; next }
    want && match($0, /<string>[^<]*<\/string>/) {
      print substr($0, RSTART + 8, RLENGTH - 17); exit
    }
  ' "$1"
}

cargo_version()  { sed -n 's/^version = "\([^"]*\)".*/\1/p' "$CARGO" | head -1; }
gradle_version() { sed -n 's/.*versionName "\([^"]*\)".*/\1/p' "$GRADLE" | head -1; }
gradle_build()   { sed -n 's/.*versionCode \([0-9]*\).*/\1/p' "$GRADLE" | head -1; }
wxs_version()    { sed -n 's/^ *Version="\([^"]*\)".*/\1/p' "$WXS" | head -1; }
csproj_version() { sed -n 's/.*<Version>\([^<]*\)<\/Version>.*/\1/p' "$CSPROJ" | head -1; }

# ---- writers ---------------------------------------------------------------

plist_set() { # <file> <key> <value>
  awk -v k="$2" -v v="$3" '
    pending { sub(/<string>[^<]*<\/string>/, "<string>" v "</string>"); pending = 0 }
    { print }
    $0 ~ "<key>" k "</key>" { pending = 1 }
  ' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
}

# GNU sed -i takes no argument and BSD/macOS sed -i requires one, so neither
# spelling is portable. Write through a temp file instead.
sed_inplace() { # <expr> <file>
  sed "$1" "$2" > "$2.tmp" && mv "$2.tmp" "$2"
}

apply() { # <version> <build>
  local v="$1" b="$2"

  sed_inplace "s/^version = \"[^\"]*\"/version = \"$v\"/" "$CARGO"
  sed_inplace "s/versionName \"[^\"]*\"/versionName \"$v\"/" "$GRADLE"
  sed_inplace "s/versionCode [0-9]*/versionCode $b/" "$GRADLE"
  sed_inplace "s/^\( *Version=\"\)[^\"]*\"/\1$v\"/" "$WXS"
  sed_inplace "s/<Version>[^<]*<\/Version>/<Version>$v<\/Version>/" "$CSPROJ"

  local p
  for p in "${PLISTS[@]}"; do
    plist_set "$p" CFBundleShortVersionString "$v"
    plist_set "$p" CFBundleVersion "$b"
  done
}

# ---- reporting -------------------------------------------------------------

# Emits "<label>\t<version>\t<build>"; a build of "-" means the artifact has no
# build counter.
survey() {
  printf 'rust workspace\t%s\t-\n'  "$(cargo_version)"
  printf 'android\t%s\t%s\n'        "$(gradle_version)" "$(gradle_build)"
  printf 'windows installer\t%s\t-\n' "$(wxs_version)"
  printf 'windows app\t%s\t-\n' "$(csproj_version)"
  local p label
  for p in "${PLISTS[@]}"; do
    label="macos $(basename "$(dirname "$p")")"
    printf '%s\t%s\t%s\n' "$label" \
      "$(plist_get "$p" CFBundleShortVersionString)" \
      "$(plist_get "$p" CFBundleVersion)"
  done
}

show() { survey | column -t -s "$(printf '\t')" 2>/dev/null || survey; }

check() {
  local versions builds
  versions=$(survey | cut -f2 | sort -u)
  builds=$(survey | cut -f3 | grep -v '^-$' | sort -u)

  if [ "$(printf '%s\n' "$versions" | wc -l)" -ne 1 ] ||
     [ "$(printf '%s\n' "$builds"   | wc -l)" -ne 1 ]; then
    echo "ERROR: version drift across artifacts." >&2
    echo >&2
    show >&2
    echo >&2
    echo "Fix with: scripts/bump-version.sh --set <version>" >&2
    return 1
  fi

  echo "OK: all artifacts at $versions (build $builds)"
}

# ---- entry point -----------------------------------------------------------

usage() { sed -n '3,28p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

main() {
  local current next build
  current=$(cargo_version)
  build=$(gradle_build)

  case "${1:---show}" in
    --current) cargo_version ;;
    --show)  show ;;
    --check) check ;;
    --set)
      next="${2:-}"
      if ! printf '%s' "$next" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        echo "ERROR: --set needs a version like 1.3.0 (got '${next}')" >&2
        exit 2
      fi
      apply "$next" "$((build + 1))"
      echo "Set all artifacts to $next (build $((build + 1)))"
      ;;
    --patch|--minor|--major)
      local maj min pat
      IFS=. read -r maj min pat <<<"$current"
      case "$1" in
        --patch) next="$maj.$min.$((pat + 1))" ;;
        --minor) next="$maj.$((min + 1)).0" ;;
        --major) next="$((maj + 1)).0.0" ;;
      esac
      apply "$next" "$((build + 1))"
      echo "Bumped $current -> $next (build $((build + 1)))"
      ;;
    -h|--help) usage ;;
    *) echo "ERROR: unknown option '$1'" >&2; echo >&2; usage >&2; exit 2 ;;
  esac
}

main "$@"
