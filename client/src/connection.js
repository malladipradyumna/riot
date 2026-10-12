// Use the local game port in development and same-origin /ws behind Caddy.
function serverWebSocketUrl(location) {
  // Python's local static server uses a separate game port; deployment uses Caddy.
  if (
    location.protocol === 'https:' ||
    !location.port ||
    location.port === '80'
  ) {
    return `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`;
  }
  const host = location.hostname || '127.0.0.1';
  return `ws://${host.includes(':') && !host.startsWith('[') ? `[${host}]` : host}:8000`;
}
