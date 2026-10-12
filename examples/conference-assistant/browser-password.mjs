import { randomBytes, scrypt, timingSafeEqual } from 'node:crypto';
import { promisify } from 'node:util';

const derive = promisify(scrypt);
const options = { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 };

export function validatePasswordVerifier(value) {
  if (value?.version !== 1 || value.algorithm !== 'scrypt' || !/^[a-f0-9]{32}$/.test(value.salt || '')
    || !/^[a-f0-9]{64}$/.test(value.hash || '')) throw new Error('Private browser password configuration required');
  return { version: 1, algorithm: 'scrypt', salt: value.salt, hash: value.hash };
}

export async function createPasswordVerifier(password) {
  if (typeof password !== 'string' || Buffer.byteLength(password) < 12 || Buffer.byteLength(password) > 256)
    throw new Error('Use a browser password between 12 and 256 bytes');
  const salt = randomBytes(16).toString('hex');
  const key = await derive(password, Buffer.from(salt, 'hex'), 32, options);
  return { version: 1, algorithm: 'scrypt', salt, hash: key.toString('hex') };
}

export async function verifyPassword(password, verifier) {
  if (typeof password !== 'string' || !password.length || Buffer.byteLength(password) > 256) return false;
  const key = await derive(password, Buffer.from(verifier.salt, 'hex'), 32, options);
  return timingSafeEqual(key, Buffer.from(verifier.hash, 'hex'));
}
