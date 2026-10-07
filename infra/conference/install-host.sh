#!/bin/bash
# Run as root through SSM after the CloudFormation bootstrap is ready.
# Arguments identify the private source artifact and its verified SHA-256.
set -euo pipefail
umask 027
bucket=$1
artifact=$2
expected_sha=$3
region=$4
hostname=$5
release=$6
for attempt in $(seq 1 60); do
  test -f /opt/parley/bootstrap-ready && break
  sleep 5
done
test -f /opt/parley/bootstrap-ready
chown parley:parley /opt/parley/build
archive=/opt/parley/build/source.tar.gz
aws s3 cp "s3://$bucket/$artifact" "$archive" --region "$region" --only-show-errors
printf '%s  %s\n' "$expected_sha" "$archive" | sha256sum --check
install -d -o parley -g parley -m 750 "/opt/parley/releases/$release"
tar -xzf "$archive" --no-same-owner -C "/opt/parley/releases/$release"
chown -R parley:parley "/opt/parley/releases/$release"
su -s /bin/bash parley -c "cd /opt/parley/releases/$release/parley && PATH=/opt/parley/.cargo/bin:\$PATH CARGO_TARGET_DIR=/opt/parley/build/target CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_INCREMENTAL=0 /opt/parley/.cargo/bin/cargo build --locked --release -j 4" > /opt/parley/build/cargo-build.log 2>&1
install -o parley -g parley -m 750 /opt/parley/build/target/release/parley "/opt/parley/releases/$release/parley/parley"
sha256sum "/opt/parley/releases/$release/parley/parley" > "/opt/parley/releases/$release/binary.sha256"
# Root-only file; no credentials in Run Command bodies, logs, or process argv.
aws ssm get-parameter --name /parley/conference-2026/runtime --with-decryption --region "$region" --query Parameter.Value --output text > /etc/parley/runtime.env
chmod 600 /etc/parley/runtime.env
ln -sfn "/opt/parley/releases/$release/parley" /opt/parley/current
cat > /etc/systemd/system/parley.service <<'UNIT'
[Unit]
Description=Parley UCTP conference demo
After=network-online.target
Wants=network-online.target
[Service]
User=parley
Group=parley
WorkingDirectory=/opt/parley/current
EnvironmentFile=/etc/parley/runtime.env
ExecStart=/opt/parley/current/parley
Restart=on-failure
RestartSec=5
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/parley/state
[Install]
WantedBy=multi-user.target
UNIT
curl --fail --silent --show-error --location 'https://caddyserver.com/api/download?os=linux&arch=arm64' -o /usr/local/bin/caddy
chmod 755 /usr/local/bin/caddy
id caddy >/dev/null 2>&1 || useradd --system --create-home --home-dir /var/lib/caddy --shell /sbin/nologin caddy
install -d -o caddy -g caddy -m 750 /etc/caddy
cat > /etc/caddy/Caddyfile <<CADDY
$hostname {
  handle /kit {
    redir * /kit/ 308
  }
  handle_path /kit/* {
    root * /srv/parley-public/kit
    header X-Content-Type-Options nosniff
    header Referrer-Policy no-referrer
    header Content-Security-Policy "default-src 'none'; style-src 'unsafe-inline'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'"
    file_server
  }
  @uctp path /uctp
  handle @uctp {
    reverse_proxy 127.0.0.1:7443
  }
  @conference path /conference /conference/* /uctp-client/* /healthz /v1/conference/* /v1/sms/inbound
  handle @conference {
    reverse_proxy 127.0.0.1:8080
  }
  handle / {
    redir * /conference/ 302
  }
  handle {
    respond 404
  }
}
CADDY
chown root:caddy /etc/caddy/Caddyfile
chmod 640 /etc/caddy/Caddyfile
cat > /etc/systemd/system/caddy.service <<'UNIT'
[Unit]
Description=Parley HTTPS and WSS edge
After=network-online.target parley.service
Wants=network-online.target
[Service]
User=caddy
Group=caddy
ExecStart=/usr/local/bin/caddy run --config /etc/caddy/Caddyfile --adapter caddyfile
ExecReload=/usr/local/bin/caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile
Restart=on-failure
TimeoutStopSec=5
LimitNOFILE=1048576
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/caddy
[Install]
WantedBy=multi-user.target
UNIT
/usr/local/bin/caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
systemctl daemon-reload
systemctl enable --now parley caddy
curl --fail --silent --show-error --retry 10 --retry-connrefused --retry-delay 1 --max-time 5 http://127.0.0.1:8080/healthz
