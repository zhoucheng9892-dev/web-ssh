//! Image CAPTCHA generation and verification.
//!
//! A fresh captcha is minted by [`generate`] which returns the PNG as a
//! base64 data URL plus the expected text. The expected text is stored in the
//! user's session under [`CAPTCHA_KEY`] and verified once on login, then
//! rotated so it can't be reused.

use captcha::{Captcha, filters::{Dots, Noise, Wave}};

/// Session key holding the current captcha answer (lowercased).
pub const CAPTCHA_KEY: &str = "captcha_text";

/// Generate a new captcha: returns (data_url, answer).
///
/// `data_url` is a ready-to-use `data:image/png;base64,...` string suitable for
/// an `<img src>`; `answer` is the expected text (lowercased) to verify against.
pub fn generate() -> (String, String) {
    let mut c = Captcha::new();
    c.add_chars(4)
        .apply_filter(Noise::new(0.3))
        .apply_filter(Wave::new(1.5, 12.0))
        .apply_filter(Dots::new(6))
        .view(160, 60);
    let answer = c.chars_as_string();
    // `as_base64()` returns a bare base64 string (no data-URL prefix), so wrap
    // it for direct use as an <img src>.
    let data_url = match c.as_base64() {
        Some(b64) => format!("data:image/png;base64,{b64}"),
        None => String::new(),
    };
    (data_url, answer)
}
