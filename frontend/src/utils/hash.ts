// Password hashing for the wire: the frontend sends SHA-256(password) as hex
// instead of the plaintext password. The backend then applies Argon2id on top
// before storing, so the DB never holds anything reversible and plaintext
// passwords never appear in logs / request bodies.
//
// We use the browser's native Web Crypto API (no deps, available in all
// evergreen browsers and secure contexts). The result is a lowercase hex string.

/** Compute SHA-256(input) and return it as a lowercase hex string. */
export async function sha256Hex(input: string): Promise<string> {
  const data = new TextEncoder().encode(input)
  const digest = await crypto.subtle.digest('SHA-256', data)
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}
