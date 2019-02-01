/* Server Communications.
 * Handles however communication to and from the remote Push Server should be done. For Desktop
 * this will be over Websocket. For mobile, it will probably be calls into the local operating
 * system and HTTPS to the web push server.
 *
 * In the future, it could be using gRPC and QUIC, or quantum relay.
 */

extern crate config;
extern crate http;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

use std::time::Duration;

use config::PushConfiguration;
use push_errors as error;
use push_errors::ErrorKind::{AlreadyRegisteredError, CommunicationError, StorageError};
use reqwest::header;
use serde_json::Value;
use std::collections::HashMap;
use storage::{Storage, Store};

#[derive(Debug)]
pub struct RegisterResponse {
    // the UAID & Channel ID associated with the request
    pub uaid: String,
    pub channelID: String,

    // Auth token for subsequent calls (note, only generated on new UAIDs)
    pub secret: Option<String>,

    // Push endpoint for 3rd parties
    pub endpoint: String,

    // The Sender/Group ID echoed back (if applicable.)
    pub senderid: Option<String>,
}

#[serde(untagged)]
#[derive(Serialize, Deserialize)]
pub enum BroadcastValue {
    Value(String),
    Nested(HashMap<String, BroadcastValue>),
}

pub trait Connection {
    // get the connection UAID
    // reset UAID. This causes all known subscriptions to be reset.
    fn reset_uaid(&self) -> error::Result<bool>;

    // send a new subscription request to the server, get back the server registration response.
    fn subscribe(
        &mut self,
        channel_id: &str,
        vapid_public_key: Option<&str>,
        registration_token: Option<&str>,
    ) -> error::Result<RegisterResponse>;

    // Drop an endpoint
    fn unsubscribe(&self, channel_id: Option<&str>, auth: &str) -> error::Result<bool>;

    // Update an endpoint with new info
    fn update(&self, channel_id: &str, auth: &str, new_token: &str) -> error::Result<bool>;

    // Get a list of server known channels. If it differs from what we have, reset the UAID, and refresh channels.
    // Should be done once a day.
    fn channel_list(&self) -> error::Result<Vec<String>>;

    // Verify that the known channel list matches up with the server list.
    fn verify_connection(&self, store: &Store) -> error::Result<bool>;

    // Regenerate the subscription info for all known, registered channelids
    // Returns HashMap<ChannelID, Endpoint>>
    // In The Future: This should be called by a subscription manager that bundles the returned endpoint along
    // with keys in a Subscription Info object {"endpoint":..., "keys":{"p256dh": ..., "auth": ...}}
    fn regenerate_endpoints(
        &mut self,
        store: &mut Store,
        vapid_public_key: Option<&str>,
        registration_token: Option<&str>,
    ) -> error::Result<HashMap<String, String>>;

    // Add one or more new broadcast subscriptions.
    fn broadcast_subscribe(&self, broadcast: BroadcastValue) -> error::Result<BroadcastValue>;

    // get the list of broadcasts
    fn broadcasts(&self) -> error::Result<BroadcastValue>;

    //impl TODO: Handle a Ping response with updated Broadcasts.
    //impl TODO: Handle an incoming Notification
}

pub struct ConnectHttp {
    options: PushConfiguration,
    client: reqwest::Client,
    uaid: Option<String>,
    secret: Option<String>,
    pub auth: Option<String>, // Server auth token
}

// TODO: Need to figure out how to build this using types and impls, because this seems seriously locked down.
pub fn connect(options: PushConfiguration) -> error::Result<ConnectHttp> {
    // find connection via options

    if options.socket_protocol.is_some() && options.http_protocol.is_some() {
        return Err(
            CommunicationError("Both socket and HTTP protocols cannot be set.".to_owned()).into(),
        );
    };
    if options.socket_protocol.is_some() {
        return Err(error::ErrorKind::GeneralError("Unsupported".to_owned()).into());
    };
    let connection = ConnectHttp {
        uaid: None,
        options: options.clone(),
        client: match reqwest::Client::builder()
            .timeout(Duration::from_secs(options.request_timeout))
            .build()
        {
            Ok(v) => v,
            Err(e) => {
                return Err(CommunicationError(format!("Could not build client: {:?}", e)).into());
            }
        },
        auth: None,
        secret: None,
    };

    Ok(connection)
}

impl Connection for ConnectHttp {
    // TODO:: reset UAID. This causes all known subscriptions to be reset.
    fn reset_uaid(&self) -> error::Result<bool> {
        // Get new uaid
        // write to storage
        Err(CommunicationError("Unsupported".to_string()).into())
    }

    // send a new subscription request to the server, get back the server registration response.
    fn subscribe(
        &mut self,
        channel_id: &str,
        vapid_public_key: Option<&str>,
        registration_token: Option<&str>,
    ) -> error::Result<RegisterResponse> {
        // check that things are set
        if self.options.http_protocol.is_none()
            || self.options.bridge_type.is_none()
            || self.options.application_id.is_none()
        {
            return Err(
                CommunicationError("Bridge type or application id not set.".to_owned()).into(),
            );
        }

        let url = format!(
            "{}://{}/v1/{}/{}/registration",
            &self.options.http_protocol.clone().unwrap(),
            &self.options.server_host,
            &self.options.bridge_type.clone().unwrap(),
            &self.options.application_id.clone().unwrap()
        );
        let mut body = HashMap::new();
        body.insert("token", self.options.application_id.clone());
        let mut request = match self.client.post(&url).json(&body).send() {
            Ok(v) => v,
            Err(e) => {
                // TODO: Check for 409 response (channelid conflict)
                return Err(CommunicationError(format!("Could not fetch endpoint: {:?}", e)).into());
            }
        };
        if request.status().is_server_error() {
            dbg!(request);
            return Err(CommunicationError(format!("Server error")).into());
        }
        if request.status().is_client_error() {
            dbg!(&request);
            if request.status() == http::StatusCode::CONFLICT {
                return Err(AlreadyRegisteredError.into());
            }
            return Err(CommunicationError(format!("Unhandled client error {:?}", request)).into());
        }
        let response: Value = match request.json() {
            Ok(v) => v,
            Err(e) => {
                return Err(CommunicationError(format!("Could not parse response: {:?}", e)).into());
            }
        };

        self.uaid = response["uaid"].as_str().map({ |s| s.to_owned() });
        self.auth = response["secret"].as_str().map({ |s| s.to_owned() });
        Ok(RegisterResponse {
            uaid: self.uaid.clone().unwrap(),
            channelID: response["channelID"].as_str().unwrap().to_owned(),
            secret: self.auth.clone(),
            endpoint: response["endpoint"].as_str().unwrap().to_owned(),
            senderid: response["senderid"].as_str().map({ |s| s.to_owned() }),
        })
    }

    // Drop an endpoint
    fn unsubscribe(&self, channel_id: Option<&str>, auth: &str) -> error::Result<bool> {
        if self.auth.is_none() {
            return Err(CommunicationError("Connection is unauthorized".into()).into());
        }
        if self.uaid.is_none() {
            return Err(CommunicationError("No UAID set".into()).into());
        }
        let mut url = format!(
            "{}://{}/v1/{}/{}/registration/{}",
            &self.options.http_protocol.clone().unwrap(),
            &self.options.server_host,
            &self.options.bridge_type.clone().unwrap(),
            &self.options.application_id.clone().unwrap(),
            &self.uaid.clone().unwrap(),
        );
        if channel_id.is_some() {
            url = format!("{}/subscription/{}", url, channel_id.unwrap())
        }
        match self
            .client
            .delete(&url)
            .header(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&self.auth.clone().unwrap()).unwrap(),
            )
            .send()
        {
            Ok(_) => Ok(true),
            Err(e) => Err(CommunicationError(format!("Could not unsubscribe: {:?}", e)).into()),
        }
    }

    // Update an endpoint with new info
    fn update(&self, channel_id: &str, auth: &str, new_token: &str) -> error::Result<bool> {
        if self.auth.is_none() {
            return Err(CommunicationError("Connection is unauthorized".into()).into());
        }
        if self.uaid.is_none() {
            return Err(CommunicationError("No UAID set".into()).into());
        }
        let url = format!(
            "{}://{}/v1/{}/{}/registration/{}/subscription/{}",
            &self.options.http_protocol.clone().unwrap(),
            &self.options.server_host,
            &self.options.bridge_type.clone().unwrap(),
            &self.options.application_id.clone().unwrap(),
            &self.uaid.clone().unwrap(),
            channel_id,
        );
        let mut body = HashMap::new();
        body.insert("token", new_token);
        match self
            .client
            .put(&url)
            .json(&body)
            .header(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&self.auth.clone().unwrap()).unwrap(),
            )
            .send()
        {
            Ok(_) => Ok(true),
            Err(e) => Err(CommunicationError(format!("Could not update token: {:?}", e)).into()),
        }
    }

    // Get a list of server known channels. If it differs from what we have, reset the UAID, and refresh channels.
    // Should be done once a day.
    fn channel_list(&self) -> error::Result<Vec<String>> {
        if self.auth.is_none() {
            return Err(CommunicationError("Connection is unauthorized".into()).into());
        }
        if self.uaid.is_none() {
            return Err(CommunicationError("No UAID set".into()).into());
        }
        let url = format!(
            "{}://{}/v1/{}/{}/registration/{}/",
            &self.options.http_protocol.clone().unwrap(),
            &self.options.server_host,
            &self.options.bridge_type.clone().unwrap(),
            &self.options.application_id.clone().unwrap(),
            &self.uaid.clone().unwrap(),
        );
        match self
            .client
            .get(&url)
            .header(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&self.auth.clone().unwrap()).unwrap(),
            )
            .send()
        {
            Ok(v) => {
                // TODO: convert the array of v.get("channelIDs") to strings
                Ok(Vec::new())
            }
            Err(e) => {
                Err(CommunicationError(format!("Could not fetch channel_list: {:?}", e)).into())
            }
        }
    }

    // Add one or more new broadcast subscriptions.
    fn broadcast_subscribe(&self, broadcast: BroadcastValue) -> error::Result<BroadcastValue> {
        Err(CommunicationError("Unsupported".to_string()).into())
    }

    // get the list of broadcasts
    fn broadcasts(&self) -> error::Result<BroadcastValue> {
        Err(CommunicationError("Unsupported".to_string()).into())
    }

    fn verify_connection(&self, store: &Store) -> error::Result<bool> {
        if self.uaid.is_none() {
            return Err(CommunicationError("Connection uninitiated".to_owned()).into());
        }
        let uaid = self.uaid.clone().unwrap();
        let remote = match self.channel_list() {
            Ok(v) => v,
            Err(e) => {
                return Err(
                    CommunicationError(format!("Could not fetch channel list: {:?}", e)).into(),
                );
            }
        };
        // verify the lengths of both lists match. Either side could have lost it's mind.
        if let Some(stored) = store.get_channel_list(&uaid)? {
            return Ok(remote == stored);
        } else {
            return Ok(remote.len() == 0);
        }
    }

    fn regenerate_endpoints(
        &mut self,
        store: &mut Store,
        vapid_public_key: Option<&str>,
        registration_token: Option<&str>,
    ) -> error::Result<HashMap<String, String>> {
        if self.uaid.is_none() {
            return Err(CommunicationError("Connection uninitiated".to_owned()).into());
        }
        let mut results: HashMap<String, String> = HashMap::new();
        if let Some(uaid) = self.uaid.clone() {
            if let Some(channels) = store.get_channel_list(&uaid)? {
                for channel in channels {
                    let info = self.subscribe(&channel, vapid_public_key, registration_token)?;
                    // TODO: fix this (or remove it when storage is not stubbed.)
                    // map_err() pukes with a can't infer type for F err
                    store.update_endpoint(&channel, &info.endpoint)?;
                    results.insert(channel, info.endpoint);
                }
            }
        }
        Ok(results)
    }
    //impl TODO: Handle a Ping response with updated Broadcasts.
    //impl TODO: Handle an incoming Notification
}

#[cfg(test)]
mod comms_test {
    use super::*;

    use super::Connection;

    use std::collections::HashMap;

    use hex;

    use crypto::{get_bytes, Key};

    // TODO mock out the reqwest calls.

    // Local test SENDER_ID
    const SENDER_ID: &'static str = "308358850242";

    #[test]
    fn test_connect() {
        let mut config = PushConfiguration {
            http_protocol: Some("http".to_owned()),
            server_host: String::from("localhost:8082"),
            application_id: Some(SENDER_ID.to_owned()),
            bridge_type: Some("gcm".to_owned()),
            ..Default::default()
        };
        let mut conn = connect(config).unwrap();
        let channelID = String::from(hex::encode(crypto::get_bytes(16).unwrap()));
        let registration_token = "SomeSytemProvidedRegistrationId";
        let response = conn
            .subscribe(&channelID, None, Some(registration_token))
            .unwrap();
        // println!("{:?}", response);
    }

}
