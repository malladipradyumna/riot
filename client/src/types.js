// JavaScript types matching server/src/types.rs.

/** @typedef {number} PlayerId */
/** @typedef {string} GameId */
/** @typedef {number} EntityId */
/** @typedef {'Blue' | 'Brown' | 'Green'} CharacterType */
/** @typedef {{ x: number, y: number }} Position */
/** @typedef {'Pistol' | 'DesertEagle' | 'Ak47' | 'RocketLauncher' | 'Flamethrower'} WeaponType */

/**
 * @typedef {{ Weapon: { weapon_type: WeaponType } }
 *   | { Bullet: { owner: PlayerId, weapon_type: WeaponType, aim_angle: number } }
 *   | { Player: { player_id: PlayerId, character_type: CharacterType, username: string, health: number, ammo: number, ammo_capacity: number, speed: number, riding_train: boolean, respawn_count: number, kills: number, deaths: number, aim_angle: number, weapon_type: WeaponType } }
 *   | { Train: { carriage_index: number, corner_count: number, aim_angle: number, width: number, height: number, speed: number } }
 *   | { Boundary: { width: number, height: number } }} GameObject
 */

/**
 * @typedef {Object} ObjectState
 * @property {EntityId} entity_id
 * @property {Position} position
 * @property {GameObject} object
 */

export {};
