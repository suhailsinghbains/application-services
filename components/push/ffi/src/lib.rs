/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use ffi_support::{
    define_box_destructor, define_handle_map_deleter, define_string_destructor, rust_str_from_c,
    rust_string_from_c, ConcurrentHandleMap, ExternError,
};
use sync15::telemetry;

use std::os::raw::c_char;

use config::PushConfiguration;
use communications::connect;
use crypto::{Key, get_bytes, SER_AUTH_LENGTH};

// indirection to help `?` figure out the target error type
fn parse_url(url: &str) -> sync15::Result<url::Url> {
    Ok(url::Url::parse(url)?)
}

#[no_mangle]
pub extern "C" fn push_enable_logcat_logging() {
    #[cfg(target_os = "android")]
    {
        let _ = std::panic::catch_unwind(|| {
            android_logger::init_once(
                android_logger::Filter::default().with_min_level(log::Level::Debug),
                Some("libpush_ffi"),
            );
            log::debug!("Android logging should be hooked up!")
        });
    }
}

lazy_static::lazy_static! {
    static ref  CONNECTIONS: ConcurrentHandleMap<PlacesDb> = ConcurrentHandleMap::new();
}

/// Instantiate a Http connection. Returned connection must be freed with
/// `push_connection_destroy`. Returns null and logs on errors (for now).
#[no_mangle]
pub unsafe extern "C" fn push_connection_new(
    server_host: *const c_char,
    socket_protocol: *const c_char,
    bridge_type: *const c_char,
    application_id: *const c_char,
    sender_id: *const c_char,
    error: &mut ExternError,
) -> u64 {
    log::debug!("push_connection_new {} {} -> {} {}=>{}",
        socket_protocol, server_host, bridge_type, sender_id, application_id);
    // return this as a reference to the map since that map contains the actual handles that rust uses.
    // see ffi layer for details.
    CONNECTIONS.insert_with_result(error, || {
        let host = ffi_support::rust_string_from_c(server_host);
        let protocol = ffi_support::opt_rust_string_from_c(socket_protocol);
        let bridge = ffi_support::opt_rust_string_from_c(bridge_type);
        let app_id = ffi_support::opt_rust_string_from_c(application_id);
        let sender = ffi_support::rust_string_from_c(sender_id);
        let key = ffi_support::opt_rust_string_from_c(encryption_key);
        let config = PushConfiguration{
            server_host: host,
            http_protocol: protocol,
            bridge_type: bridge,
            application_id: app_id,
            sender_id: sender,
            .. default()
        }
        connect(config)
    })
}

// Add a subscription
/// Errors are logged.
#[no_mangle]
pub unsafe extern "C" fn push_get_subscription_info(
    handle: u64,
    channel_id: *const c_char,
    vapid_key: *const c_char,
    token: *const c_char,
    error: &mut ExternError,
) -> *mut c_char{
    log::debug!("push_get_subscription");
    CONNECTIONS.call_with_result_mut(error, handle, |conn| {
        let channel = ffi_support::rust_str_from_c(channel_id);
        let key = ffi_support::opt_rust_string_from_c(vapid_key);
        let reg_token = ffi_support::opt_rust_string_from_c(token);
        let subscription_key = crypto::generate_key().unwrap();
        let auth = base64::encode_config(
            &crypto::get_bytes(SER_AUTH_LENGTH), base64::URL_SAFE_NO_PAD);
        let info = conn.subscribe(channel, key, reg_token).unwrap();
        // TODO: store the channelid => auth + subscription_key
        let subscription_info = json!({
            "endpoint": info.endpoint
            "keys": {
                "auth": auth,
                "p256dh": base64::encode_config(subscription_key.public,
                                                base64::URL_SAFE_NO_PAD)
            }
        })
        return subscription_info.to_string()
    })
}

// Unsubscribe a channel
#[no_mangle]
pub unsafe extern "C" fn push_unsubscribe(
    handle: u64,
    channel_id: *const c_char,
    error: &mut ExternError,
) -> bool{
    log::debug!("push_unsubscribe");
    CONNECTIONS.call_with_result_mut(error, handle, |conn| {
        let channel = ffi_support::opt_rust_str_from_c(channel_id);
        conn.subscribe(channel).unwrap();
    })
}

// Update the OS token
#[no_mangle]
pub unsafe extern "C" fn push_update(
    handle: u64,
    new_token: *const c_char,
    error: &mut ExternError,
) -> bool{
    log::debug!("push_update");
    CONNECTIONS.call_with_result_mut(error, handle, |conn| {
        let token = ffi_support::opt_rust_str_from_c(new_token);
        conn.update(token).unwrap();
    })
}

// verify connection using channel list
#[no_mangle]
pub unsafe extern "C" fn push_verify_connection(
    handle: u64,
    vapid_key: *const c_char,
    registration_token: *const c_char,
    error: &mut ExternError,
) -> Option<HashMap<String, String>>{
    //TODO: Can you return an option hashmap and have it mean something? How to flatten?
    log::debug!("push_verify");
    CONNECTIONS.call_with_result_mut(error, handle, |conn| {
        let known_channels = storage::get_channel_list(conn.uaid.unwrap());
        let key = ffi_support::opt_rust_string_from_c(vapid_key);
        let reg_token = ffi_support::opt_rust_string_from_c(token);
        if ! conn.verify_connection(known_channels){
            Some(conn.regenerate_endpoints(
                known_channels,
                key,
                reg_token
                ))
        }
        None
    })
}

// TODO: modify these to be relevant.

define_string_destructor!(places_destroy_string);
define_handle_map_deleter!(CONNECTIONS, places_connection_destroy);
define_box_destructor!(PlacesInterruptHandle, places_interrupt_handle_destroy);
