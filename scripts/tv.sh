#!/usr/bin/env bash
# Local TV helper: install, launch, logs, self-test and trace collection.
#
# Configuration comes from the environment or a git-ignored .tv.env file next to this repo
# (never hard-code an address or password):
#   TV_HOST=192.168.x.y      TV address (required)
#   TV_MODE=rooted|devmode   rooted: ssh as root (Homebrew Channel / SSH enabled)
#                            devmode: LG Developer Mode, ssh as prisoner on port 9922
#   TV_KEY=~/.ssh/tv_webos   ssh key (devmode: the key from `ares-novacom --getkey`)
#   FLAVOR=debug|release     which app identity to act on (default debug)
#
# Usage: scripts/tv.sh install <ipk> | launch | stop | logs | selftest | traces <dir> | remove
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
[ -f "$ROOT/.tv.env" ] && . "$ROOT/.tv.env"
: "${TV_HOST:?set TV_HOST (environment or .tv.env)}"
TV_MODE="${TV_MODE:-rooted}"
FLAVOR="${FLAVOR:-debug}"
APP_ID="io.github.danzig666.lumablocks"
[ "$FLAVOR" = debug ] && APP_ID="$APP_ID.debug"
if [ "$TV_MODE" = devmode ]; then
  SSH=(ssh -p 9922 ${TV_KEY:+-i "$TV_KEY"} "prisoner@$TV_HOST")
  SCP=(scp -P 9922 ${TV_KEY:+-i "$TV_KEY"})
  SCP_DEST="prisoner@$TV_HOST"
  APP_DIR="/media/developer/apps/usr/palm/applications/$APP_ID"
else
  SSH=(ssh ${TV_KEY:+-i "$TV_KEY"} "root@$TV_HOST")
  SCP=(scp ${TV_KEY:+-i "$TV_KEY"})
  SCP_DEST="root@$TV_HOST"
  APP_DIR="/media/developer/apps/usr/palm/applications/$APP_ID"
fi
cmd="${1:-}"
shift || true
case "$cmd" in
  install)
    ipk="${1:?ipk path}"
    sha256sum "$ipk"
    if [ "$TV_MODE" = devmode ]; then
      ares-install --device "${ARES_DEVICE:-tv}" "$ipk"
    else
      "${SCP[@]}" "$ipk" "$SCP_DEST:/tmp/luma-blocks.ipk"
      "${SSH[@]}" "luna-send -w 30000 -i luna://com.webos.appInstallService/dev/install '{\"id\":\"com.ares.defaultName\",\"ipkUrl\":\"/tmp/luma-blocks.ipk\",\"subscribe\":true}' | grep -m1 -E 'installed|failed|error'; rm -f /tmp/luma-blocks.ipk"
    fi
    ;;
  launch)
    if [ "$TV_MODE" = devmode ]; then ares-launch --device "${ARES_DEVICE:-tv}" "$APP_ID"
    else "${SSH[@]}" "luna-send -n 1 luna://com.webos.applicationManager/launch '{\"id\":\"$APP_ID\"}'"; fi
    ;;
  stop)
    if [ "$TV_MODE" = devmode ]; then ares-launch --device "${ARES_DEVICE:-tv}" --close "$APP_ID"
    else "${SSH[@]}" "luna-send -n 1 luna://com.webos.applicationManager/closeByAppId '{\"id\":\"$APP_ID\"}'"; fi
    ;;
  logs)
    "${SSH[@]}" "cat $APP_DIR/data/logs/luma-blocks.log; echo '--- previous run ---'; tail -n 80 $APP_DIR/data/logs/luma-blocks.log.1 2>/dev/null || true"
    ;;
  selftest)
    # Runs the installed binary's non-graphical checks with the app's own libraries.
    # NOTE: this is a shell context, not the launcher's sandbox: Luna access results here do
    # not prove what the launched app may do (use the in-app Input diagnostics for that).
    "${SSH[@]}" "cd $APP_DIR && HOME=$APP_DIR ./bin/luma-blocks --self-test"
    ;;
  traces)
    out="${1:-traces}"
    mkdir -p "$out"
    "${SCP[@]}" "$SCP_DEST:$APP_DIR/data/traces/*.jsonl" "$out/"
    ls -la "$out"
    ;;
  remove)
    if [ "$TV_MODE" = devmode ]; then ares-install --device "${ARES_DEVICE:-tv}" --remove "$APP_ID"
    else "${SSH[@]}" "luna-send -w 20000 -i luna://com.webos.appInstallService/dev/remove '{\"id\":\"$APP_ID\",\"subscribe\":true}'"; fi
    ;;
  *)
    sed -n '2,16p' "$0"
    exit 2
    ;;
esac
