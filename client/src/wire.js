// The browser and Rust consume the same schema. The runtime is vendored locally.
let clientEnvelope;
let serverEnvelope;
const protocolReady = fetch('assets/protocol/game.proto').then(async response => {
  if (!response.ok) throw new Error('Could not load protocol schema');
  const root = protobuf.parse(await response.text(), { keepCase: true }).root;
  clientEnvelope = root.lookupType('riot.ClientEnvelope');
  serverEnvelope = root.lookupType('riot.ServerEnvelope');
  return true;
}).catch(error => { console.error(error); return false; });

function encodeClientMessage(message) {
  const fields = { JoinGame: 'join_game', ExitGame: 'exit_game', PlayerInput: 'player_input' };
  const type = Object.keys(message)[0];
  if (!fields[type] || !clientEnvelope) throw new Error('Invalid client message');
  const value = clientEnvelope.fromObject({ [fields[type]]: message[type] });
  const error = clientEnvelope.verify(value);
  if (error) throw new Error(error);
  return clientEnvelope.encode(value).finish();
}

function decodeServerMessage(bytes) {
  const value = serverEnvelope.toObject(serverEnvelope.decode(bytes), {
    longs: Number, enums: String, defaults: true, arrays: true, oneofs: true,
  });
  const tags = {
    welcome: 'Welcome', state_snapshot: 'StateSnapshot', gun_fired: 'GunFired',
    player_killed: 'PlayerKilled', error: 'Error', player_died: 'PlayerDied',
    player_left: 'PlayerLeft', pong: 'Pong',
  };
  const type = value.payload;
  if (!tags[type]) throw new Error('Missing server payload');
  const payload = value[type];
  if (type === 'state_snapshot') {
    const objects = { player: 'Player', bullet: 'Bullet', weapon: 'Weapon', boundary: 'Boundary', train: 'Train' };
    payload.objects = payload.objects.map(object => {
      const kind = object.object;
      if (!objects[kind]) throw new Error('Unknown snapshot object');
      return { entity_id: object.entity_id, position: object.position || { x: 0, y: 0 }, object: { [objects[kind]]: object[kind] } };
    });
  }
  return { [tags[type]]: payload };
}
