#!/bin/sh
# Installs Airi's agent in Android's Linux terminal. The Airi app puts this next to the agent and
# gives you the command that runs it, with a token that only this phone's copy of Airi knows.
set -eu

token=${1:?Run this with the command from the Airi app}
here=$(dirname "$(readlink -f "$0")")
config=${XDG_CONFIG_HOME:-$HOME/.config}

install -D -m 755 "$here/airi-agent" "$HOME/.local/bin/airi-agent"

mkdir -p "$config/airi" "$config/systemd/user"
(umask 077 && printf '%s\n' "$token" > "$config/airi/token")

cat > "$config/systemd/user/airi-agent.service" <<'UNIT'
[Unit]
Description=Airi, which shows Linux apps on Android

[Service]
ExecStart=%h/.local/bin/airi-agent serve
Restart=always
RestartSec=1

[Install]
WantedBy=default.target
UNIT

# Without this the agent would wait for someone to open a terminal tab after Linux boots.
sudo loginctl enable-linger "$(id -un)"
systemctl --user daemon-reload
systemctl --user enable airi-agent
systemctl --user restart airi-agent

rm -rf "$here" || true
echo
echo 'Airi is installed. If Android asks about port 47821, tap "Allow always", then go back to Airi.'
