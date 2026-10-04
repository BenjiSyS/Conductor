//! Device trust: pairing codes, tokens (stored as hashes), scopes, revocation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    /// Project ids this device may access. Empty = none.
    pub projects: Vec<String>,
    pub created: u64,
    pub last_seen: Option<u64>,
    /// Whether this device may send prompts / approvals (not just observe).
    #[serde(default)]
    pub can_control: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: u64,
    pub projects: Vec<String>,
    pub can_control: bool,
    #[serde(skip)]
    failures: u32,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PairError {
    #[error("pairing code is invalid or expired")]
    Invalid,
    #[error("too many attempts; generate a new code on the host")]
    Locked,
    #[error("device name is required")]
    Name,
}

#[derive(Default, Serialize, Deserialize)]
struct DeviceFile {
    devices: Vec<Device>,
}

pub struct DeviceStore {
    path: PathBuf,
    devices: BTreeMap<String, Device>,
    codes: Vec<PairingCode>,
}

const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // no 0/O/1/I
pub const CODE_TTL_SECS: u64 = 300;
const MAX_FAILURES: u32 = 5;

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl DeviceStore {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let devices = match std::fs::read(&path) {
            Ok(b) => {
                serde_json::from_slice::<DeviceFile>(&b)
                    .unwrap_or_default()
                    .devices
            }
            Err(_) => Vec::new(),
        };
        Ok(Self {
            path,
            devices: devices.into_iter().map(|d| (d.id.clone(), d)).collect(),
            codes: Vec::new(),
        })
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let f = DeviceFile {
            devices: self.devices.values().cloned().collect(),
        };
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&f).unwrap_or_default())?;
        std::fs::rename(tmp, &self.path)
    }

    /// Create a single-use pairing code valid for 5 minutes.
    pub fn new_code(&mut self, projects: Vec<String>, can_control: bool, now: u64) -> PairingCode {
        self.codes.retain(|c| c.expires_at > now);
        let mut rng = rand::thread_rng();
        let code: String = (0..8)
            .map(|_| CODE_ALPHABET[(rng.next_u32() as usize) % CODE_ALPHABET.len()] as char)
            .collect();
        let c = PairingCode {
            code,
            expires_at: now + CODE_TTL_SECS,
            projects,
            can_control,
            failures: 0,
        };
        self.codes.push(c.clone());
        c
    }

    /// Exchange a pairing code for a device token. Returns (device, token);
    /// the raw token is only ever returned here.
    pub fn pair(
        &mut self,
        code: &str,
        name: &str,
        now: u64,
    ) -> Result<(Device, String), PairError> {
        if name.trim().is_empty() {
            return Err(PairError::Name);
        }
        let code = code.trim().to_ascii_uppercase().replace(['-', ' '], "");
        self.codes
            .retain(|c| c.expires_at > now && c.failures < MAX_FAILURES);
        let Some(idx) = self.codes.iter().position(|c| ct_eq(&c.code, &code)) else {
            // Count a failure against every live code (brute-force defence).
            for c in &mut self.codes {
                c.failures += 1;
            }
            if self.codes.iter().any(|c| c.failures >= MAX_FAILURES) {
                self.codes.retain(|c| c.failures < MAX_FAILURES);
                return Err(PairError::Locked);
            }
            return Err(PairError::Invalid);
        };
        let pc = self.codes.remove(idx);
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let token = B64.encode(raw);
        let d = Device {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.trim().chars().take(64).collect(),
            token_hash: hash_token(&token),
            projects: pc.projects,
            created: now,
            last_seen: None,
            can_control: pc.can_control,
        };
        self.devices.insert(d.id.clone(), d.clone());
        let _ = self.save();
        Ok((d, token))
    }

    pub fn authenticate(&mut self, token: &str, now: u64) -> Option<Device> {
        let h = hash_token(token);
        let d = self
            .devices
            .values_mut()
            .find(|d| ct_eq(&d.token_hash, &h))?;
        d.last_seen = Some(now);
        Some(d.clone())
    }

    pub fn revoke(&mut self, id: &str) -> bool {
        let removed = self.devices.remove(id).is_some();
        if removed {
            let _ = self.save();
        }
        removed
    }

    pub fn set_projects(&mut self, id: &str, projects: Vec<String>) -> bool {
        match self.devices.get_mut(id) {
            Some(d) => {
                d.projects = projects;
                let _ = self.save();
                true
            }
            None => false,
        }
    }

    pub fn list(&self) -> Vec<Device> {
        self.devices.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_authenticate_revoke() {
        let d = tempfile::tempdir().unwrap();
        let mut s = DeviceStore::open(d.path().join("devices.json")).unwrap();
        let c = s.new_code(vec!["p1".into()], true, 100);
        assert_eq!(c.code.len(), 8);
        let (dev, token) = s.pair(&c.code.to_lowercase(), "Phone", 110).unwrap();
        assert!(!dev.token_hash.contains(&token));
        // single use
        assert_eq!(
            s.pair(&c.code, "Again", 111).unwrap_err(),
            PairError::Invalid
        );
        assert_eq!(s.authenticate(&token, 120).unwrap().id, dev.id);
        assert!(s.authenticate("wrong", 120).is_none());
        // persisted without the raw token
        let raw = std::fs::read_to_string(d.path().join("devices.json")).unwrap();
        assert!(!raw.contains(&token));
        let mut s2 = DeviceStore::open(d.path().join("devices.json")).unwrap();
        assert!(s2.authenticate(&token, 130).is_some());
        assert!(s2.revoke(&dev.id));
        assert!(s2.authenticate(&token, 140).is_none());
    }

    #[test]
    fn codes_expire_and_lock_after_failures() {
        let d = tempfile::tempdir().unwrap();
        let mut s = DeviceStore::open(d.path().join("devices.json")).unwrap();
        let c = s.new_code(vec![], false, 0);
        assert_eq!(
            s.pair(&c.code, "x", CODE_TTL_SECS + 1).unwrap_err(),
            PairError::Invalid
        );
        let c = s.new_code(vec![], false, 1000);
        let mut last = Ok(());
        for _ in 0..MAX_FAILURES {
            last = s.pair("WRONGCOD", "x", 1001).map(|_| ());
        }
        assert_eq!(last.unwrap_err(), PairError::Locked);
        assert_eq!(
            s.pair(&c.code, "x", 1002).unwrap_err(),
            PairError::Invalid,
            "locked code is gone"
        );
        assert_eq!(s.pair("x", " ", 0).unwrap_err(), PairError::Name);
    }
}
