# Caddy serves plain browser files and proxies /ws; there is no Node build step.
FROM caddy:2.11.7-alpine
COPY client/index.html /srv/index.html
COPY LICENSE THIRD_PARTY_NOTICES.md /srv/
COPY client/src /srv/src
COPY assets /srv/assets
COPY deploy/Caddyfile /etc/caddy/Caddyfile
