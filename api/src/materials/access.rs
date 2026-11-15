//! Short-lived links to a material's original file.
//!
//! The API authenticates with a bearer header, which `<img>`, `<audio>` and `<video>` cannot
//! send. For those the frontend asks (with its bearer token) for a link to *this API*:
//! `/api/materials/:id/file?t=<token>`. The token names one material and one teacher, expires,
//! and is signed with a key that only lives in this process (links die with a restart; the
//! frontend simply asks again). It is not a link to the object store, which stays unreachable
//! from the browser.

use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

/// Long enough to watch a clip; short enough that a copied link is soon useless.
pub const LINK_MINUTES: i64 = 60;

type HmacSha256 = Hmac<Sha256>;

pub struct LinkSigner {
    key: [u8; 32],
}

impl Default for LinkSigner {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkSigner {
    pub fn new() -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self { key }
    }

    fn mac(&self, material_id: i64, teacher_id: i64, expires: i64) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC takes a key of any size");
        mac.update(format!("material:{material_id}:teacher:{teacher_id}:exp:{expires}").as_bytes());
        mac
    }

    /// A token for `material_id`, valid for [`LINK_MINUTES`], and when it expires.
    pub fn issue(&self, material_id: i64, teacher_id: i64) -> (String, DateTime<Utc>) {
        self.issue_at(material_id, teacher_id, Utc::now())
    }

    pub fn issue_at(
        &self,
        material_id: i64,
        teacher_id: i64,
        now: DateTime<Utc>,
    ) -> (String, DateTime<Utc>) {
        let expires = now + Duration::minutes(LINK_MINUTES);
        let ts = expires.timestamp();
        let sig = hex::encode(
            self.mac(material_id, teacher_id, ts)
                .finalize()
                .into_bytes(),
        );
        (format!("{ts}.{teacher_id}.{sig}"), expires)
    }

    /// The teacher the token was issued to, if it is a valid, unexpired token for this material.
    pub fn verify(&self, material_id: i64, token: &str) -> Option<i64> {
        self.verify_at(material_id, token, Utc::now())
    }

    pub fn verify_at(&self, material_id: i64, token: &str, now: DateTime<Utc>) -> Option<i64> {
        let mut parts = token.splitn(3, '.');
        let expires: i64 = parts.next()?.parse().ok()?;
        let teacher_id: i64 = parts.next()?.parse().ok()?;
        let sig = hex::decode(parts.next()?).ok()?;
        if expires <= now.timestamp() {
            return None;
        }
        // Constant-time comparison.
        self.mac(material_id, teacher_id, expires)
            .verify_slice(&sig)
            .ok()?;
        Some(teacher_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_opens_one_material_for_one_teacher_for_a_while() {
        let signer = LinkSigner::new();
        let now = Utc::now();
        let (token, expires) = signer.issue_at(12, 7, now);
        assert_eq!(expires, now + Duration::minutes(LINK_MINUTES));
        assert_eq!(signer.verify_at(12, &token, now), Some(7));
        // Another material, an expired link, a changed teacher, another process: all refused.
        assert_eq!(signer.verify_at(13, &token, now), None);
        assert_eq!(
            signer.verify_at(12, &token, now + Duration::minutes(LINK_MINUTES + 1)),
            None
        );
        let forged = token.replacen(".7.", ".8.", 1);
        assert_eq!(signer.verify_at(12, &forged, now), None);
        assert_eq!(LinkSigner::new().verify_at(12, &token, now), None);
        for junk in ["", "abc", "1.2", "x.y.z", "9999999999.7.zz"] {
            assert_eq!(signer.verify_at(12, junk, now), None, "{junk}");
        }
    }
}
