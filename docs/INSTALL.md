# Installing on an LG TV

Two packages are published per release:

* `io.github.danzig666.lumablocks_<ver>_arm.ipk`: **Luma Blocks**, the normal app.
* `io.github.danzig666.lumablocks.debug_<ver>_arm.ipk`: **Luma Blocks (Dev)**, a separate test
  identity with its own saves. Install it beside the normal app when trying new builds.

Neither needs root at runtime, and neither changes TV-wide settings.

## Option A: Developer Mode (non-rooted TVs)

Requirements: an LG developer account, the **Developer Mode** app on the TV, and LG's CLI
(`npm install -g @webos-tools/cli`) or [webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop).

```sh
ares-setup-device --add tv --info "{'host':'<TV-IP>','port':'9922','username':'prisoner'}"
ares-novacom --device tv --getkey        # passphrase shown in the Developer Mode app
ares-install --device tv io.github.danzig666.lumablocks_0.1.0_arm.ipk
ares-launch  --device tv io.github.danzig666.lumablocks
```

Developer Mode sessions expire and must be renewed in the Developer Mode app. When a session
expires, webOS removes side-loaded apps; that is LG's policy and this project does not work
around it. From Windows: `tools\Install-LumaBlocks.ps1 -Route DevMode`.

## Option B: Rooted TV with Homebrew Channel

Requirements: a rooted TV with Homebrew Channel and SSH enabled. Root is only used to install;
the app runs as a normal app.

* **Homebrew Channel / Dev Manager**: install the `.ipk` file directly.
* **SSH**: `TV_HOST=<TV-IP> scripts/tv.sh install io.github.danzig666.lumablocks_0.1.0_arm.ipk`.
  From Windows: `tools\Install-LumaBlocks.ps1 -Route Rooted -TvHost <TV-IP>`.

## Helpers (no addresses or credentials are stored in the repo)

Put your settings in a git-ignored `.tv.env` (or the environment):

```sh
TV_HOST=192.168.1.50
TV_MODE=rooted            # or devmode
TV_KEY=~/.ssh/tv_webos    # optional
FLAVOR=debug              # or release
```

| Command | Does |
|---|---|
| `scripts/tv.sh install <ipk>` | install / update |
| `scripts/tv.sh launch` / `stop` | start / close the app |
| `scripts/tv.sh logs` | print the current and previous log |
| `scripts/tv.sh traces <dir>` | copy recorded gesture traces |
| `scripts/tv.sh selftest` | run `--self-test` on the TV (shell context, see the note in the script) |
| `scripts/tv.sh remove` | uninstall |

Logs and traces live in
`/media/developer/apps/usr/palm/applications/<app id>/data/{logs,traces}`. Windows:
`tools\Get-LumaBlocksLogs.ps1`.

## Updating

Install the newer `.ipk` over the old one. Settings, records and a saved game are kept in the
app's `data/` directory. Whether webOS preserves that directory across an update is
**unverified**; it is on the [TV checklist](TV_CHECKLIST.md).
