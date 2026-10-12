// Cache the lobby and HUD elements once; render frames reuse these references.
const form = document.querySelector('#lobby-form');
const usernameInput = document.querySelector('#username');
const gameIdInput = document.querySelector('#game-id');
const createButton = document.querySelector('#create-game');
const joinButton = document.querySelector('#join-game');
const status = document.querySelector('#status');
const session = document.querySelector('#session');
const joinedGameId = document.querySelector('#joined-game-id');
const copyGameIdButton = document.querySelector('#copy-game-id');
const damageFlash = document.querySelector('#damage-flash');
const playerInfo = document.querySelector('#player-info');
const exitButton = document.querySelector('#exit-game');
const lobby = document.querySelector('.lobby');
const board = document.querySelector('#board');
const canvas = document.querySelector('#arena');
const context = canvas.getContext('2d');
context.imageSmoothingEnabled = false;
const playerCount = document.querySelector('#player-count');
const fpsDisplay = document.querySelector('#fps');
const minimap = document.querySelector('#minimap');
const minimapContext = minimap.getContext('2d');
const healthBar = document.querySelector('#health-bar');
const healthValue = document.querySelector('#health-value');
const ammoValue = document.querySelector('#ammo-value');
const scoreboardBody = document.querySelector('#scoreboard-body');
const roundLabel = document.querySelector('#round-label');
const roundCountdown = document.querySelector('#round-countdown');

// These dimensions match assets/maps/arena.txt and the server constants.
const MAP_WIDTH = 4800;
const MAP_HEIGHT = 1800;
const camera = { x: 0, y: 0 };
let localPlayerId = null;
// The socket fills this small history; drawing reads it on the same browser thread.
const snapshots = [];
const MAX_SNAPSHOTS = 5;
const INTERPOLATION_DELAY_MS = 100;
let lastProcessedSnapshotTimestamp = -Infinity;
let currentSnapshot = null;
let serverClockOffset = null;
let animationFrame = null;
let fpsFrames = 0;
let fpsStartedAt = null;
let aimAngle = null;
let mousePosition = null;
let displayedLocalPlayer = null;
let mouseFiring = false;
let killAnnouncementTimer = null;
let copyFeedbackTimer = null;
let damageAnimation = null;
let lastLocalHealth = null;
let lastLocalRespawn = null;

// Restart the one-second damage overlay instead of stacking animations.
function showDamageFlash() {
  damageAnimation?.cancel();
  damageAnimation = damageFlash.animate([{ opacity: 1 }, { opacity: 0 }], {
    duration: 1000,
    easing: 'ease-out',
  });
}

// Prefer the clipboard API; selection keeps copying usable on plain-HTTP LAN sessions.
copyGameIdButton.addEventListener('click', async () => {
  if (!joinedGameId.value) return;
  let copied = false;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(joinedGameId.value);
      copied = true;
    }
  } catch (_) {
    /* Plain HTTP can block Clipboard API; use the selected-input fallback. */
  }
  if (!copied) {
    joinedGameId.focus();
    joinedGameId.select();
    copied = document.execCommand('copy');
  }
  copyGameIdButton.textContent = copied ? 'Copied!' : 'Select + copy';
  clearTimeout(copyFeedbackTimer);
  copyFeedbackTimer = setTimeout(() => {
    copyGameIdButton.textContent = 'Copy';
  }, 1500);
});

// Mouse coordinates are on the page; aiming needs coordinates in the game world.
function updateAim(player) {
  if (!player) return;
  let targetX = camera.x + canvas.width / 2;
  let targetY = camera.y + canvas.height / 2;
  if (mousePosition) {
    const bounds = canvas.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    targetX =
      camera.x +
      ((mousePosition.x - bounds.left) / bounds.width) * canvas.width;
    targetY =
      camera.y +
      ((mousePosition.y - bounds.top) / bounds.height) * canvas.height;
  } else if (aimAngle !== null) {
    return; // Keep the initial direction until the mouse is moved over the arena.
  }
  const dx = targetX - player.x;
  const dy = targetY - player.y;
  if (dx !== 0 || dy !== 0) aimAngle = Math.atan2(dy, dx);
  else if (aimAngle === null) aimAngle = 0;
}

// Ignore old frames so a delayed message cannot rewind the game.
function storeSnapshot(snapshot) {
  const latest = snapshots[snapshots.length - 1];
  if (!Number.isFinite(snapshot.timestamp_millis)) return;
  if (latest && snapshot.timestamp_millis <= latest.serverTime) return;
  const receivedAt = performance.now();
  const players = new Map();
  const bullets = new Map();
  const trains = new Map();
  const weapons = new Map();
  for (const object of snapshot.objects) {
    const player = object.object.Player;
    if (!player) {
      if (object.object.Train) {
        trains.set(object.entity_id, {
          ...object.object.Train,
          entityId: object.entity_id,
          x: object.position.x,
          y: object.position.y,
        });
      }
      if (object.object.Weapon) {
        weapons.set(object.entity_id, {
          ...object.object.Weapon,
          entityId: object.entity_id,
          x: object.position.x,
          y: object.position.y,
        });
      }
      if (object.object.Bullet) {
        bullets.set(object.entity_id, {
          entityId: object.entity_id,
          owner: object.object.Bullet.owner,
          weapon_type: object.object.Bullet.weapon_type,
          aim_angle: object.object.Bullet.aim_angle,
          x: object.position.x,
          y: object.position.y,
        });
      }
      continue;
    }
    players.set(player.player_id, {
      ...player,
      entityId: object.entity_id,
      x: object.position.x,
      y: object.position.y,
    });
  }
  // A new phase or round must not blend with positions from the previous one.
  const round = snapshot.round;
  if (
    latest &&
    round &&
    (latest.round?.number !== round.number ||
      latest.round?.intermission !== round.intermission)
  ) {
    snapshots.length = 0;
    pressedKeys.clear();
    mouseFiring = false;
    stopTrainAudio();
    stopTerrainAudio();
    clearTimeout(killAnnouncementTimer);
    document.querySelector('#kill-announcement').textContent = '';
  }
  // Keep at most five frames, enough for the short interpolation delay.
  snapshots.push({
    serverTime: snapshot.timestamp_millis,
    players,
    bullets,
    trains,
    weapons,
    round,
  });
  if (snapshots.length > MAX_SNAPSHOTS) snapshots.shift();
  // Anchor server time to the browser's monotonic clock, independent of its wall clock.
  if (serverClockOffset === null)
    serverClockOffset = snapshot.timestamp_millis - receivedAt;
}

// Draw slightly behind the server to interpolate between known positions.
function interpolatedEntities(now, kind) {
  const displayTime = now + serverClockOffset - INTERPOLATION_DELAY_MS;
  let before = snapshots[0];
  let after = before;
  for (const snapshot of snapshots) {
    if (snapshot.serverTime <= displayTime) {
      before = snapshot;
      after = snapshot;
    } else {
      after = snapshot;
      break;
    }
  }
  // Clamp playback to known endpoints instead of guessing past the latest frame.
  const span = after.serverTime - before.serverTime;
  const fraction =
    span > 0
      ? Math.max(0, Math.min(1, (displayTime - before.serverTime) / span))
      : 0;
  const entities = [];
  // Use the newest membership so joins and exits appear immediately.
  for (const [id, player] of currentSnapshot[kind]) {
    const start = before[kind].get(id);
    const end = after[kind].get(id);
    if (kind === 'bullets') {
      // New bullets must wait for the delayed playback timeline. Showing their
      // newest position first makes them jump backward when interpolation starts.
      if (!start || displayTime < before.serverTime) continue;
      if (!end) {
        entities.push(start);
        continue;
      }
    }
    if (
      !start ||
      !end ||
      (kind === 'players' &&
        (start.respawn_count !== player.respawn_count ||
          end.respawn_count !== player.respawn_count))
    ) {
      // New players and respawns appear at their current position without a long slide.
      entities.push(player);
      continue;
    }
    let x = start.x + (end.x - start.x) * fraction;
    let y = start.y + (end.y - start.y) * fraction;
    if (
      kind === 'players' &&
      !player.riding_train &&
      fraction > 0 &&
      fraction < 1
    ) {
      const dx = end.x - start.x;
      const dy = end.y - start.y;
      const distance = Math.hypot(dx, dy);
      if (distance > 0) {
        // Integrate terrain speed along the known snapshot path, never past its endpoint.
        x = start.x;
        y = start.y;
        let remaining = (span * fraction) / 1000;
        let traveled = 0;
        const initialTerrain = terrainAt(x, y);
        const speedMultiplier =
          (start.speed ?? TERRAIN_SPEEDS[initialTerrain]) /
          TERRAIN_SPEEDS[initialTerrain];
        while (remaining > 0 && traveled < distance) {
          const dt = Math.min(remaining, 1 / 120);
          const tile = terrainAt(x, y);
          const speed = TERRAIN_SPEEDS[tile] * speedMultiplier;
          traveled = Math.min(distance, traveled + speed * dt);
          x = start.x + (dx / distance) * traveled;
          y = start.y + (dy / distance) * traveled;
          remaining -= dt;
        }
      }
    }
    entities.push({ ...player, x, y });
  }
  return entities;
}

// Draw at the display's refresh rate, independently of input and snapshot rates.
function drawFrame(now) {
  // The browser schedules the next frame; network callbacks only store new state.
  animationFrame = requestAnimationFrame(drawFrame);
  const latest = snapshots[snapshots.length - 1];
  if (!latest) return;
  // Process changed HUD state once, while position interpolation continues every frame.
  if (latest.serverTime > lastProcessedSnapshotTimestamp) {
    currentSnapshot = latest;
    lastProcessedSnapshotTimestamp = latest.serverTime;
    renderScoreboard(currentSnapshot.players);
    const local = currentSnapshot.players.get(localPlayerId);
    if (local) {
      if (
        lastLocalHealth !== null &&
        lastLocalRespawn === local.respawn_count
      ) {
        if (local.health < lastLocalHealth) showDamageFlash();
      }
      lastLocalHealth = local.health;
      lastLocalRespawn = local.respawn_count;
    }
  }
  // Count down against the server deadline, using the anchored server clock offset.
  const round = currentSnapshot.round;
  const intermission = Boolean(round?.intermission);
  board.classList.toggle('intermission', intermission);
  roundLabel.textContent = intermission
    ? `Round ${round.number} complete · Next round in`
    : `Round ${round?.number ?? 1} · Time left`;
  const secondsLeft = round
    ? Math.max(
        0,
        Math.ceil((round.ends_at_millis - (now + serverClockOffset)) / 1000),
      )
    : 300;
  roundCountdown.textContent = intermission
    ? String(secondsLeft)
    : `${Math.floor(secondsLeft / 60)}:${String(secondsLeft % 60).padStart(2, '0')}`;
  document.querySelector('#scoreboard-title').textContent = intermission
    ? 'Round results'
    : 'Scoreboard';
  if (intermission) return;
  // Even without a new snapshot, advance the interpolated position each frame.
  const players = interpolatedEntities(now, 'players');
  const trains = interpolatedEntities(now, 'trains');
  renderArena(players, interpolatedEntities(now, 'bullets'), trains, [
    ...currentSnapshot.weapons.values(),
  ]);
  // Sound sources follow the same displayed world positions as the sprites.
  updateTrainAudio(trains, displayedLocalPlayer);
  updateTerrainAudio(players, displayedLocalPlayer);
  if (fpsStartedAt === null) fpsStartedAt = now;
  fpsFrames += 1;
  const elapsed = now - fpsStartedAt;
  if (elapsed >= 1000) {
    fpsDisplay.textContent = `FPS: ${Math.round((fpsFrames * 1000) / elapsed)}`;
    fpsFrames = 0;
    fpsStartedAt = now;
  }
}

// Build the standings from authoritative kills/deaths, including the hitmen.
function renderScoreboard(players) {
  // Highest kills first; fewer deaths and then ID make ties stable.
  const ranked = [...players.values()].sort(
    (a, b) =>
      (b.kills ?? 0) - (a.kills ?? 0) ||
      (a.deaths ?? 0) - (b.deaths ?? 0) ||
      a.player_id - b.player_id,
  );
  const rows = ranked.map((player) => {
    const row = document.createElement('tr');
    row.classList.toggle('local-player', player.player_id === localPlayerId);
    for (const value of [
      player.username,
      player.kills ?? 0,
      player.deaths ?? 0,
    ]) {
      const cell = document.createElement('td');
      // Names are text, never HTML supplied by another player.
      cell.textContent = String(value);
      row.append(cell);
    }
    return row;
  });
  scoreboardBody.replaceChildren(...rows);
}

// Clear the old session, animation, sounds, and HUD before leaving or joining again.
function resetRendering() {
  if (animationFrame !== null) cancelAnimationFrame(animationFrame);
  animationFrame = null;
  snapshots.length = 0;
  currentSnapshot = null;
  lastProcessedSnapshotTimestamp = -Infinity;
  serverClockOffset = null;
  fpsFrames = 0;
  fpsStartedAt = null;
  fpsDisplay.textContent = 'FPS: —';
  healthBar.value = 100;
  healthValue.textContent = '100 / 100';
  ammoValue.textContent = 'Ammo: —';
  // Stop ambient nodes before dropping the positions that drive them.
  stopTrainAudio();
  stopTerrainAudio();
  damageAnimation?.cancel();
  damageAnimation = null;
  lastLocalHealth = null;
  lastLocalRespawn = null;
  healthBar.classList.remove('on-train');
  board.classList.remove('intermission');
  clearTimeout(copyFeedbackTimer);
  copyGameIdButton.textContent = 'Copy';
  scoreboardBody.replaceChildren();
  minimapContext.clearRect(0, 0, minimap.width, minimap.height);
  clearTimeout(killAnnouncementTimer);
  killAnnouncementTimer = null;
  document.querySelector('#kill-announcement').textContent = '';
  aimAngle = null;
  mousePosition = null;
  displayedLocalPlayer = null;
  camera.x = 0;
  camera.y = 0;
  mouseFiring = false;
}

// Crop the weapon sheet and flip weapons whose source art points the other way.
function drawWeaponSprite(sprite, weapon, x, y, width, height) {
  context.save();
  context.translate(x, y);
  if (weapon.spriteFlipped) {
    context.translate(width, 0);
    context.scale(-1, 1);
  }
  context.drawImage(sprite, ...weapon.spriteSource, 0, 0, width, height);
  context.restore();
}

// Draw the HUD and camera view: ground, trains, pickups, bullets, then players.
function renderArena(players, bullets, trains, weapons) {
  if (!context) return;
  // Health and ammo come straight from snapshots; positions are interpolated separately.
  const authoritativePlayer = currentSnapshot?.players.get(localPlayerId);
  const health = Math.max(0, Math.min(100, authoritativePlayer?.health ?? 100));
  ammoValue.textContent = authoritativePlayer
    ? `Ammo: ${authoritativePlayer.ammo ?? 0} / ${authoritativePlayer.ammo_capacity ?? 0}${authoritativePlayer.riding_train ? ' · Resupplying + healing' : ''}`
    : 'Ammo: —';
  healthBar.value = health;
  healthValue.textContent = `${Math.ceil(health)} / 100`;
  healthBar.classList.toggle(
    'on-train',
    Boolean(authoritativePlayer?.riding_train),
  );
  renderMinimap(players, trains);
  displayedLocalPlayer =
    players.find((player) => player.player_id === localPlayerId) || null;
  // Follow our player with a fixed world viewport, independent of CSS screen size.
  if (displayedLocalPlayer) {
    camera.x = displayedLocalPlayer.x - canvas.width / 2;
    camera.y = displayedLocalPlayer.y - canvas.height / 2;
    updateAim(displayedLocalPlayer);
  }
  // Clear the viewport and draw only terrain tiles that can be on screen.
  context.fillStyle = '#191c17';
  context.fillRect(0, 0, canvas.width, canvas.height);
  for (
    let row = Math.max(0, Math.floor(camera.y / TILE_SIZE));
    row <
    Math.min(
      terrainRows.length,
      Math.ceil((camera.y + canvas.height) / TILE_SIZE),
    );
    row++
  ) {
    for (
      let col = Math.max(0, Math.floor(camera.x / TILE_SIZE));
      col <
      Math.min(
        terrainRows[row].length,
        Math.ceil((camera.x + canvas.width) / TILE_SIZE),
      );
      col++
    ) {
      const image = assetImages.get(TERRAIN_ASSETS[terrainRows[row][col]]);
      if (image) {
        context.save();
        context.translate(
          (col + 0.5) * TILE_SIZE - camera.x,
          (row + 0.5) * TILE_SIZE - camera.y,
        );
        if (terrainRows[row][col] === 't' && (row === 6 || row === 30))
          context.rotate(Math.PI / 2);
        context.drawImage(
          image,
          -TILE_SIZE / 2,
          -TILE_SIZE / 2,
          TILE_SIZE,
          TILE_SIZE,
        );
        context.restore();
      }
    }
  }
  context.strokeStyle = '#707887';
  context.lineWidth = 4;
  context.strokeRect(2 - camera.x, 2 - camera.y, MAP_WIDTH - 4, MAP_HEIGHT - 4);
  context.font = '12px monospace';
  context.textAlign = 'center';

  // Trains are drawn from snapshots and rotated around their carriage centers.
  for (const train of trains) {
    const trainSprite = assetImages.get(
      train.carriage_index > 0
        ? 'assets/props/train-carriage.svg'
        : 'assets/props/train.svg',
    );
    const x = train.x - camera.x;
    const y = train.y - camera.y;
    if (
      !trainSprite ||
      x < -180 ||
      y < -180 ||
      x > canvas.width + 180 ||
      y > canvas.height + 180
    )
      continue;
    context.save();
    context.translate(x, y);
    context.rotate(train.aim_angle);
    context.drawImage(
      trainSprite,
      -train.width / 2,
      -train.height / 2,
      train.width,
      train.height,
    );
    context.restore();
  }

  // Ground guns use server wall-clock expiry, matching the border countdown.
  const serverNow = performance.now() + serverClockOffset;
  for (const pickup of weapons) {
    if (pickup.expires_at_millis <= serverNow) continue;
    const x = pickup.x - camera.x;
    const y = pickup.y - camera.y;
    if (x < -26 || y < -26 || x > canvas.width + 26 || y > canvas.height + 26)
      continue;
    const asset = WEAPON_ASSETS[pickup.weapon_type];
    const sprite = asset && assetImages.get(asset.sprite);
    if (!sprite) continue;
    context.fillStyle = '#242419';
    context.fillRect(x - 22, y - 22, 44, 44);
    drawWeaponSprite(sprite, asset, x - 16, y - 8, 32, 16);
    const progress = Math.max(
      0,
      Math.min(
        1,
        1 - (pickup.expires_at_millis - serverNow) / pickup.lifetime_millis,
      ),
    );
    drawPickupTimer(x, y, progress);
  }

  // Bullet angle is its travel direction, including momentum inherited at launch.
  for (const bullet of bullets) {
    const x = bullet.x - camera.x;
    const y = bullet.y - camera.y;
    if (x < -12 || y < -12 || x > canvas.width + 12 || y > canvas.height + 12)
      continue;
    const sprite = bulletSprite(bullet.weapon_type, '#ffd34d');
    if (!sprite) continue;
    context.save();
    context.translate(x, y);
    context.rotate(bullet.aim_angle + Math.PI / 2);
    context.drawImage(sprite, -sprite.width / 2, -sprite.height / 2);
    context.restore();
  }

  playerCount.textContent = `${players.length} ${players.length === 1 ? 'player' : 'players'}`;
  // Draw each character and its independently aimed gun, then the name label.
  for (const player of players) {
    const x = player.x - camera.x;
    const y = player.y - camera.y;
    const isLocal = player.player_id === localPlayerId;
    if (x < -40 || y < -40 || x > canvas.width + 40 || y > canvas.height + 40)
      continue;
    const angle = isLocal ? (aimAngle ?? 0) : player.aim_angle;
    const character =
      CHARACTER_ASSETS[player.character_type] || CHARACTER_ASSETS.Blue;
    const bodySprite = assetImages.get(character.sprite);
    const weapon = WEAPON_ASSETS[player.weapon_type];
    const gunSprite = weapon && assetImages.get(weapon.sprite);
    context.save();
    context.translate(x, y);
    context.rotate(angle);
    if (bodySprite) context.drawImage(bodySprite, -11, -14, 22, 28);
    if (gunSprite)
      drawWeaponSprite(
        gunSprite,
        weapon,
        6,
        2,
        weapon.width || 14,
        weapon.height || 7,
      );
    context.restore();

    context.fillStyle = '#e4d3a1';
    const label = `${player.username}${isLocal ? ' (you)' : ''}`;
    const halfWidth = context.measureText(label).width / 2 + 4;
    const labelX = Math.max(halfWidth, Math.min(canvas.width - halfWidth, x));
    context.fillText(label, labelX, y < 40 ? y + 34 : y - 24);
  }
}

// Fill the square border over the pickup lifetime; the server removes expired guns.
function drawPickupTimer(x, y, progress) {
  const size = 44;
  context.strokeStyle = '#75664a';
  context.lineWidth = 3;
  context.strokeRect(x - size / 2, y - size / 2, size, size);
  const corners = [
    [x - 22, y - 22],
    [x + 22, y - 22],
    [x + 22, y + 22],
    [x - 22, y + 22],
    [x - 22, y - 22],
  ];
  let remaining = progress * size * 4;
  context.strokeStyle = '#f1ca69';
  context.beginPath();
  context.moveTo(...corners[0]);
  for (let side = 0; side < 4 && remaining > 0; side++) {
    const fraction = Math.min(1, remaining / size);
    const [startX, startY] = corners[side];
    const [endX, endY] = corners[side + 1];
    context.lineTo(
      startX + (endX - startX) * fraction,
      startY + (endY - startY) * fraction,
    );
    remaining -= size;
  }
  context.stroke();
}

// Project world positions onto the small radar without terrain or camera offsets.
function renderMinimap(players, trains) {
  minimapContext.fillStyle = '#10180f';
  minimapContext.fillRect(0, 0, minimap.width, minimap.height);
  for (const train of trains) {
    minimapContext.fillStyle = '#61aaf4';
    minimapContext.fillRect(
      (train.x / MAP_WIDTH) * minimap.width - 3,
      (train.y / MAP_HEIGHT) * minimap.height - 2,
      6,
      4,
    );
  }
  // Draw the local marker last so it stays visible when players overlap.
  for (const local of [false, true]) {
    for (const player of players) {
      if ((player.player_id === localPlayerId) !== local) continue;
      minimapContext.fillStyle = local ? '#82d965' : '#ec6859';
      minimapContext.fillRect(
        (player.x / MAP_WIDTH) * minimap.width - 2,
        (player.y / MAP_HEIGHT) * minimap.height - 2,
        4,
        4,
      );
    }
  }
}

/** @type {WebSocket | null} */
let socket = null;
let inputTimer = null;
let leavingGame = false;
const pressedKeys = new Set();
const gameKeys = new Set([
  'KeyW',
  'KeyA',
  'KeyS',
  'KeyD',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'KeyF',
  'ShiftLeft',
  'ShiftRight',
]);

// Stop sending input and forget held controls when this connection ends.
function stopInput() {
  clearInterval(inputTimer);
  inputTimer = null;
  pressedKeys.clear();
  mouseFiring = false;
}

// Send held keys, not movement distances. The server decides how far we move.
function sendInput() {
  if (
    !socket ||
    socket.readyState !== WebSocket.OPEN ||
    localPlayerId === null ||
    leavingGame ||
    currentSnapshot?.round?.intermission ||
    aimAngle === null
  )
    return;
  updateAim(displayedLocalPlayer);
  /** @type {import('./protocol.js').ClientMessage} */
  const message = {
    PlayerInput: {
      client_request_id: `input-${Date.now()}`,
      timestamp_millis: Date.now(),
      player_id: localPlayerId,
      game_id: joinedGameId.value,
      up: pressedKeys.has('KeyW') || pressedKeys.has('ArrowUp'),
      down: pressedKeys.has('KeyS') || pressedKeys.has('ArrowDown'),
      left: pressedKeys.has('KeyA') || pressedKeys.has('ArrowLeft'),
      right: pressedKeys.has('KeyD') || pressedKeys.has('ArrowRight'),
      shooting: pressedKeys.has('KeyF') || mouseFiring,
      sprinting: pressedKeys.has('ShiftLeft') || pressedKeys.has('ShiftRight'),
      aim_angle: aimAngle,
    },
  };
  socket.send(encodeClientMessage(message));
}

// Record held keys; fire presses are also sent immediately to preserve quick taps.
window.addEventListener('keydown', (event) => {
  if (
    localPlayerId === null ||
    leavingGame ||
    currentSnapshot?.round?.intermission ||
    !gameKeys.has(event.code)
  )
    return;
  if (
    event.target instanceof HTMLInputElement &&
    !event.target.readOnly &&
    !event.target.disabled
  )
    return;
  event.preventDefault();
  unlockAudio();
  pressedKeys.add(event.code);
  if (event.code === 'KeyF' && !event.repeat) sendInput();
});
// Releases remove intent even when focus has moved onto a UI control.
window.addEventListener('keyup', (event) => {
  pressedKeys.delete(event.code);
  if (event.code === 'KeyF') sendInput();
  if (
    localPlayerId !== null &&
    gameKeys.has(event.code) &&
    !(
      event.target instanceof HTMLInputElement &&
      !event.target.readOnly &&
      !event.target.disabled
    )
  ) {
    event.preventDefault();
  }
});

// Send a released state when focus is lost, so movement/fire do not stay held.
function releaseKeys() {
  pressedKeys.clear();
  mouseFiring = false;
  sendInput();
}

// Remember page coordinates; updateAim applies scaling and the camera offset.
canvas.addEventListener('mousemove', (event) => {
  if (localPlayerId === null || leavingGame) return;
  mousePosition = { x: event.clientX, y: event.clientY };
  updateAim(displayedLocalPlayer);
});
// A user gesture can unlock browser audio before the first firing event arrives.
canvas.addEventListener('pointerdown', () => {
  if (localPlayerId !== null) unlockAudio();
});
// Only left-click inside the arena starts firing; focus keeps keyboard controls there.
canvas.addEventListener('mousedown', (event) => {
  if (event.button !== 0 || localPlayerId === null || leavingGame) return;
  event.preventDefault();
  canvas.focus({ preventScroll: true });
  mousePosition = { x: event.clientX, y: event.clientY };
  mouseFiring = true;
  updateAim(displayedLocalPlayer);
  sendInput();
});
// Listen on the window so releasing outside the canvas still stops firing.
window.addEventListener('mouseup', (event) => {
  if (event.button !== 0) return;
  const wasFiring = mouseFiring;
  mouseFiring = false;
  if (wasFiring) sendInput();
});

// Clear input when switching windows or hiding the tab.
window.addEventListener('blur', releaseKeys);
document.addEventListener('visibilitychange', () => {
  if (document.hidden) releaseKeys();
});

// Tell the game we are leaving, stop local loops, and close this connection.
exitButton.addEventListener('click', () => {
  if (!socket || socket.readyState !== WebSocket.OPEN || localPlayerId === null)
    return;
  leavingGame = true;
  exitButton.disabled = true;
  stopInput();
  resetRendering();
  socket.send(
    encodeClientMessage({
      ExitGame: {
        client_request_id: `exit-${Date.now()}`,
        timestamp_millis: Date.now(),
        username: usernameInput.value,
        game_id: joinedGameId.value,
        player_id: localPlayerId,
      },
    }),
  );
  socket.close();
});

// Disable lobby controls while an asset load or connection attempt is in progress.
function setBusy(busy) {
  createButton.disabled = busy;
  joinButton.disabled = busy;
  usernameInput.disabled = busy;
  gameIdInput.disabled = busy;
  document.querySelector('#character-picker').disabled = busy;
  document.querySelector('#bot-settings').disabled = busy;
}

/** Create or join a room after validating the form and loading its assets.
 * @param {string | null} gameId
 */
async function joinGame(gameId) {
  usernameInput.value = usernameInput.value.trim();
  if (!usernameInput.reportValidity()) return;
  gameIdInput.setCustomValidity('');
  if (gameId !== null && !gameId) {
    gameIdInput.setCustomValidity('Enter a game ID to join.');
    gameIdInput.reportValidity();
    return;
  }

  const botCountInput = document.querySelector('#bot-count');
  if (gameId === null && !botCountInput.reportValidity()) return;
  // Freeze creation settings before connecting; joining never overrides a room.
  const botSettings =
    gameId === null
      ? {
          bot_count: Number(botCountInput.value),
          bot_difficulty: Number(
            document.querySelector('#bot-difficulty').value,
          ),
        }
      : {};
  unlockAudio();
  const characterType = form.querySelector(
    'input[name="character_type"]:checked',
  ).value;
  setBusy(true);
  status.textContent = 'Loading sprites…';
  if (!(await assetsReady)) {
    setBusy(false);
    status.textContent =
      'Could not load sprites. Refresh the page and check the client server.';
    return;
  }
  stopInput();
  leavingGame = false;
  exitButton.disabled = false;
  session.hidden = true;
  board.hidden = true;
  resetRendering();
  localPlayerId = null;
  status.textContent = 'Connecting…';
  const connection = new WebSocket(serverWebSocketUrl(window.location));
  connection.binaryType = 'arraybuffer';
  socket = connection;
  let welcomed = false;
  let failed = false;
  // Require Welcome within ten seconds; opening a socket alone is not a successful join.
  const timeout = setTimeout(() => {
    failed = true;
    status.textContent =
      'Connection timed out. Check that the server is running.';
    connection.close();
  }, 10000);

  // Once the WebSocket opens, send exactly one join request for this attempt.
  connection.addEventListener('open', () => {
    status.textContent = gameId === null ? 'Creating game…' : 'Joining game…';
    /** @type {import('./protocol.js').ClientMessage} */
    const message = {
      JoinGame: {
        client_request_id: `join-${Date.now()}`,
        timestamp_millis: Date.now(),
        username: usernameInput.value,
        character_type: characterType,
        game_id: gameId,
        ...botSettings,
      },
    };
    connection.send(encodeClientMessage(message));
  });

  // Discrete events update effects; snapshots update the bounded render history.
  connection.addEventListener('message', (event) => {
    let message;
    try {
      message = decodeServerMessage(new Uint8Array(event.data));
    } catch {
      failed = true;
      status.textContent =
        'Could not decode server message. Refresh the client and restart the server.';
      connection.close();
      return;
    }
    // Death announcements and audio use the server names and the old death position.
    if (message.PlayerKilled) {
      const kill = message.PlayerKilled;
      if (welcomed && !leavingGame && kill.game_id === joinedGameId.value) {
        playSound(
          'PlayerDeath',
          kill.position,
          displayedLocalPlayer || currentSnapshot?.players.get(localPlayerId),
          0.65,
        );
        const announcement = document.querySelector('#kill-announcement');
        const weaponName =
          {
            Pistol: 'Pistol',
            Ak47: 'AK-47',
            DesertEagle: 'Desert Eagle',
            RocketLauncher: 'Rocket Launcher',
          }[kill.weapon_type] || kill.weapon_type;
        announcement.textContent = `${kill.killer_name} killed ${kill.victim_name} with ${weaponName}`;
        clearTimeout(killAnnouncementTimer);
        killAnnouncementTimer = setTimeout(() => {
          announcement.textContent = '';
          killAnnouncementTimer = null;
        }, 5000);
      }
    }
    // An application error ends this attempt and lets the user try again.
    if (message.Error) {
      failed = true;
      status.textContent = `Server: ${message.Error.code}`;
      connection.close();
      return;
    }
    if (
      message.PlayerDamaged &&
      welcomed &&
      !leavingGame &&
      message.PlayerDamaged.game_id === joinedGameId.value &&
      message.PlayerDamaged.player_id === localPlayerId
    ) {
      showDamageFlash();
    }
    if (
      message.TrainWhistle &&
      welcomed &&
      !leavingGame &&
      message.TrainWhistle.game_id === joinedGameId.value
    ) {
      playSound(
        'TrainWhistle',
        message.TrainWhistle.position,
        displayedLocalPlayer || currentSnapshot?.players.get(localPlayerId),
        0.6,
      );
    }
    if (message.GunFired) {
      const shot = message.GunFired;
      if (welcomed && !leavingGame && shot.game_id === joinedGameId.value) {
        playSound(
          shot.weapon_type,
          shot.position,
          displayedLocalPlayer || currentSnapshot?.players.get(localPlayerId),
        );
      }
    }
    // Welcome assigns our identity; input may start only after this reply.
    if (message.Welcome) {
      clearTimeout(timeout);
      welcomed = true;
      localPlayerId = message.Welcome.player_id;
      joinedGameId.value = message.Welcome.game_id;
      playerInfo.textContent = `Player ID: ${message.Welcome.player_id}`;
      session.hidden = false;
      status.textContent = `Joined as ${usernameInput.value}.`;
      form.hidden = true;
      lobby.classList.add('in-game');
      stopInput();
      inputTimer = setInterval(sendInput, 30);
    }
    // Render only frames belonging to the successfully joined game.
    if (message.StateSnapshot) {
      const snapshot = message.StateSnapshot;
      if (!welcomed || leavingGame || snapshot.game_id !== joinedGameId.value)
        return;
      storeSnapshot(snapshot);
      const firstSnapshot = board.hidden;
      board.hidden = false;
      if (firstSnapshot) canvas.focus({ preventScroll: true });
      if (animationFrame === null)
        animationFrame = requestAnimationFrame(drawFrame);
    }
  });

  // Surface connection failures in the lobby instead of leaving disabled controls.
  connection.addEventListener('error', () => {
    failed = true;
    status.textContent =
      'Could not connect. Start the Rust server on port 8000.';
  });

  // Restore the lobby for both voluntary exits and unexpected disconnects.
  connection.addEventListener('close', () => {
    clearTimeout(timeout);
    stopInput();
    if (socket === connection) socket = null;
    setBusy(false);
    session.hidden = true;
    board.hidden = true;
    form.hidden = false;
    lobby.classList.remove('in-game');
    resetRendering();
    localPlayerId = null;
    if (!failed) {
      status.textContent = leavingGame
        ? 'Left the game.'
        : welcomed
          ? 'Disconnected. You can join again.'
          : 'Connection closed before joining.';
    }
  });
}

// Both lobby buttons and Enter share the same validated join flow.
createButton.addEventListener('click', () => joinGame(null));
joinButton.addEventListener('click', () => joinGame(gameIdInput.value.trim()));
gameIdInput.addEventListener('input', () => gameIdInput.setCustomValidity(''));
form.addEventListener('submit', (event) => {
  event.preventDefault();
  if (!createButton.disabled) joinGame(gameIdInput.value.trim() || null);
});
