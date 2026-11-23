//! Backups encrypted with a key per user, destroyed at erasure (MAIR-500, "crypto-shredding").
//!
//! The live database stays in clear: it is used in real time. A backup Job restores the live dump
//! into a throwaway Postgres, `compliance-backup seal` moves each user's personal values (per the
//! inventory) into a blob encrypted with a data key of that user, wrapped by the user's key in the
//! instance's key manager (Scaleway Key Manager), and scrubs them from the copy; the dump of the
//! copy and the blobs then go to restic, whose repository key is the instance key (outside the
//! backup too). Erasing a user destroys their key (`erasure::connectors::backup_key`): their data
//! becomes unreadable in every old backup, which nobody modifies. `compliance-backup unseal`, after
//! a restore, puts back the users whose key still exists; the erased ones come back anonymized.

pub mod crypto;
pub mod keys;
pub mod plan;
pub mod seal;
