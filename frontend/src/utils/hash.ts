// Password hashing for the wire: the frontend sends SHA-256(password) as hex
// instead of the plaintext password. The backend then applies Argon2id on top
// before storing, so the DB never holds anything reversible and plaintext
// passwords never appear in logs / request bodies.
//
// We use js-sha256, a pure-JS implementation that works in ALL browser
// environments without requiring Web Crypto API (crypto.subtle is only
// available in "secure contexts" which some Chrome configs block on HTTP).

import { sha256 } from 'js-sha256'

/** Compute SHA-256(input) and return it as a lowercase hex string. */
export async function sha256Hex(input: string): Promise<string> {
  return sha256(input)
}
