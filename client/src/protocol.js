// JSDoc types provide editor checking while keeping this plain JavaScript.
// These are application objects. wire.js encodes them using assets/protocol/game.proto.

/** @typedef {import('./types.js').PlayerId} PlayerId */
/** @typedef {import('./types.js').GameId} GameId */
/** @typedef {import('./types.js').ObjectState} ObjectState */
/** @typedef {import('./types.js').CharacterType} CharacterType */
/** @typedef {import('./types.js').WeaponType} WeaponType */
/** @typedef {import('./types.js').Position} Position */

/**
 * @typedef {Object} ClientRequest
 * @property {string} client_request_id
 * @property {number} timestamp_millis
 */

/**
 * @typedef {ClientRequest & {
 *   username: string,
 *   character_type: CharacterType,
 *   game_id: GameId | null
 * }} JoinGamePayload
 */

/**
 * @typedef {ClientRequest & {
 *   username: string,
 *   game_id: GameId,
 *   player_id: PlayerId
 * }} ExitGamePayload
 */

/**
 * @typedef {ClientRequest & {
 *   player_id: PlayerId,
 *   game_id: GameId,
 *   up: boolean,
 *   down: boolean,
 *   left: boolean,
 *   right: boolean,
 *   shooting: boolean,
 *   aim_angle: number
 * }} PlayerInputPayload
 */

/**
 * @typedef {{ JoinGame: JoinGamePayload }
 *   | { ExitGame: ExitGamePayload }
 *   | { PlayerInput: PlayerInputPayload }} ClientMessage
 */

/**
 * @typedef {Object} ServerResponse
 * @property {string} server_request_id
 * @property {number} timestamp_millis
 * @property {PlayerId} player_id
 * @property {GameId} game_id
 */

/**
 * @typedef {ServerResponse & {
 *   last_processed_client_request_id: string
 * }} ProcessedResponse
 */

/** @typedef {{ Welcome: ServerResponse }} WelcomeMessage */
/** @typedef {{ GunFired: ServerResponse & { weapon_type: WeaponType, position: Position, aim_angle: number } }} GunFiredMessage */
/** @typedef {{ StateSnapshot: { game_id: GameId, timestamp_millis: number, objects: ObjectState[], acknowledgements: { player_id: PlayerId, last_processed_client_request_id: string }[] } }} StateSnapshotMessage */
/** @typedef {{ PlayerDied: ProcessedResponse & { dead_player_id: PlayerId } }} PlayerDiedMessage */
/** @typedef {{ PlayerLeft: ProcessedResponse & { abandoned_player_id: PlayerId } }} PlayerLeftMessage */

// The WebSocket transports binary Protobuf envelopes.
/**
 * @typedef {WelcomeMessage | StateSnapshotMessage | PlayerDiedMessage | PlayerLeftMessage | GunFiredMessage | { PlayerKilled: PlayerKilledMessage } | { Error: { code: string } } | { Pong: {} }} ServerMessage
 */

export {};

/**
 * @typedef {{ game_id: import('./types.js').GameId, killer_id: number, victim_id: number, killer_name: string, victim_name: string, weapon_type: import('./types.js').WeaponType }} PlayerKilledMessage
 */
