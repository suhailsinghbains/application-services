/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 * Handle Push data storage
 */
extern crate crypto;
#[cfg(test)]
extern crate hex;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use openssl::ec::EcKey;
use openssl::pkey::Private;

use crypto::{get_bytes, Key};
use push_errors::{self as errors, Result};
use push_errors::ErrorKind::StorageError;

pub type ChannelID = String;

#[derive(Clone, Debug, PartialEq)]
pub struct PushRecord {
    // Endpoint provided from the push server
    pub endpoint: String,

    // Designation label provided by the subscribing service
    pub designator: String,

    // List of origin Host attributes.
    pub origin_attributes: HashMap<String, String>,

    // Number of pushes for this record
    pub push_count: u8,

    // Last push rec'vd
    pub last_push: u64,

    // Private EC Prime256v1 key info. (Public key can be derived from this)
    pub key: Vec<u8>,

    // Is this as priviledged system record
    pub system_record: bool,

    // VAPID public key to restrict subscription updates for only those that sign
    // using the private VAPID key.
    pub app_server_key: Option<String>,

    // List of the most recent message IDs from the server.
    pub recent_message_ids: Vec<String>,

    // Time this subscription was created.
    pub ctime: u64,

    // Max quota count for sub
    pub quota: u8,

    // (if this is a bridged connection (e.g. on Android), this is the native OS Push ID)
    pub native_id: Option<String>,
}

pub fn now_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl PushRecord {
    fn increment(&mut self) -> Result<Self> {
        self.push_count += 1;
        self.last_push = now_u64();
        // TODO check for quotas, etc
        // use push_errors::ErrorKind::StorageError;
        // write to storage.
        Ok(self.clone())
    }
}

//TODO: Add broadcasts storage

pub trait Storage {
    // Connect to the storage system
    // fn connect<S: Storage>() -> S;

    // Generate a Push Record from the Subscription info, which has the endpoint,
    // encryption keys, etc.
    fn create_record(
        uaid: &str,
        chid: &str,
        origin_attributes: HashMap<String, String>,
        endpoint: &str,
        auth: &str,
        private_key: &Key,
        system_record: bool,
    ) -> PushRecord;

    fn update_record(&mut self, uaid: &str, chid: &str, endpoint: &str) -> Result<PushRecord>;

    fn get_record(&self, uaid: &str, chid: &str) -> Option<PushRecord>;

    fn put_record(&mut self, uaid: &str, chid: &str, record: &PushRecord) -> Result<bool>;
    fn purge(&mut self, uaid: &str, chid: Option<&str>) -> Result<bool>;

    fn generate_channel_id(&self) -> String;

    fn get_channel_list(&self, uaid: &str) -> Option<Vec<String>>;
}

// Connect may need to be struct specific.

pub struct Store;

// TODO: Fill this out (pretty skeletal)
impl Storage for Store {
    fn create_record(
        _uaid: &str,
        chid: &str,
        origin_attributes: HashMap<String, String>,
        endpoint: &str,
        server_auth: &str,
        private_key: &Key,
        _system_record: bool,
    ) -> PushRecord {
        // TODO: fill this out properly
        PushRecord {
            endpoint: String::from(endpoint),
            designator: String::from(chid),
            origin_attributes: origin_attributes.clone(),
            push_count: 0,
            last_push: 0,
            key: private_key.serialize().unwrap(),
            system_record: false,
            app_server_key: None,
            recent_message_ids: Vec::new(),
            // do we need sub second resolution?
            ctime: now_u64(),
            quota: 0,
            native_id: None,
        }
    }

    fn get_record(&self, _uaid: &str, _chid: &str) -> Option<PushRecord> {
        None
    }

    fn put_record(
        &mut self,
        _uaid: &str,
        _chid: &str,
        _record: &PushRecord,
    ) -> Result<bool> {
        Ok(false)
    }

    fn purge(&mut self, _uaid: &str, _chid: Option<&str>) -> Result<bool> {
        Ok(false)
    }

    fn generate_channel_id(&self) -> String {
        String::from("deadbeef00000000decafbad00000000")
    }

    fn update_record(&mut self, uaid: &str, chid: &str, endpoint: &str) -> Result<PushRecord> {
        // swap out endpoint
        Err(errors::ErrorKind::StorageError("unimplemented".to_owned()).into())
    }

    fn get_channel_list(&self, uaid: &str) -> Option<Vec<String>> {
        Some(Vec::new())
    }
}

#[cfg(test)]
struct MockStore {
    pub reply: Option<PushRecord>,
    pub stored: HashMap<String, PushRecord>,
}

#[cfg(test)]
impl Storage for MockStore {
    fn create_record(
        uaid: &str,
        chid: &str,
        origin_attributes: HashMap<String, String>,
        endpoint: &str,
        server_auth: &str,
        private_key: &Key,
        system_record: bool,
    ) -> PushRecord {
        PushRecord {
            endpoint: String::from(endpoint),
            designator: String::from(chid),
            origin_attributes: origin_attributes.clone(),
            push_count: 0,
            last_push: 0,
            key: private_key.serialize().unwrap(),
            system_record: system_record,
            app_server_key: None,
            recent_message_ids: Vec::new(),
            ctime: now_u64(),
            quota: 0,
            native_id: None,
        }
    }

    fn get_record(&self, uaid: &str, chid: &str) -> Option<PushRecord> {
        match self.stored.get(&format!("{}-{}", uaid, chid)) {
            Some(p) => Some(p.clone()),
            None => None,
        }
    }

    fn put_record(&mut self, uaid: &str, chid: &str, record: &PushRecord) -> Result<bool> {
        self.stored
            .insert(format!("{}-{}", uaid, chid), record.clone());
        Ok(true)
    }

    fn update_record(&mut self, uaid: &str, chid: &str, endpoint: &str) -> Result<bool> {
        // swap out endpoint
        Err(errors::ErrorKind::StorageError("unimplemented".to_owned()).into())
    }

    fn purge(&mut self, uaid: &str, chid: Option<&str>) -> Result<bool> {
        self.stored
            .remove(&format!("{}-{}", uaid, chid.unwrap_or("")));
        Ok(true)
    }

    fn generate_channel_id(&self) -> String {
        hex::encode(get_bytes(16).unwrap())
    }

    fn get_channel_list(&self, uaid: &str) -> Option<Vec<String>> {
        None
    }
}
