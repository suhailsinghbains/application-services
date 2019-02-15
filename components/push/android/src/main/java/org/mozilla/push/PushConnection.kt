/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package org.mozilla.places

import com.sun.jna.Pointer
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import java.io.File

/**
 * An implementation of a [PushAPI] backed by a Rust Push library.
 *
 * @param server_host the host name for the service (e.g. "push.service.mozilla.org").
 * @param socket_protocol the optional socket protocol (default: "https")
 * @param bridge_type the optional bridge protocol (default: "fcm")
 * @param application_id the native OS messaging registration id
 * @param encryption_key an optional key used for encrypting/decrypting data stored in the internal
 *  database. If omitted, data will be stored in plaintext.
 */
class PushConnection(
    application_id: String,
    sender_id: String,
    server_host: String?="push.service.mozilla.org", 
    socket_protocol: String?="https",
    bridge_type:String?="fcm",
    encryption_key: String? = null) : PushAPI, AutoCloseable {
    private var conn: RawPushConnection?

    init {
        try {
            conn = rustCall { error ->
                LibPushFFI.INSTANCE.push_connection_new(application_id, sender_id, server_host, socket_protocol, bridge_type)
            }
        } catch (e: InternalPanic) {
            // Do local error handling?
            throw e
        }
    }

    @Synchronized
    override fun close() {

        // todo: Cleanup.
        val conn = this.conn
        this.conn = null
        if (conn != null) {
            LibPushFFI.INSTANCE.push_connection_destroy(conn)
        }
    }

    override fun getSubscriptionInfo(
        channelID: String,
        ): SubscriptionInfo {
        val json = rustCallForString { error ->
            LibPushFFI.INSTANCE.push_get_subscription_info(
                this.conn!!, channelID, error)
        }
        return SubscriptionInfo.fromJSON(json)
    }

    override fun unsubscribe(channelID: String): Bool {
        val result = rustCall { error ->
            LibPushFFI.INSTANCE.push_unsubscribe(
                this.conn!!, channelID, error)
        }
        return result
    }

    override fun update(new_token: String): Bool {
        val result = rustCall { error ->
            LibPushFFI.INSTANCE.push_update(
                this.conn!!, new_token, error)
        }
        return result
    }


    override fun verifyConnection(): Map<String, String> {
        val newEndpoints: MutableMap<String, String> = linkedMapOf();
        val response = rustCallForString { error ->
            LibPushFFI.INSTANCE.push_verify_connection(
                this.conn!!, error)
        }
        if length(response) {
            val visited = JSONObject(response)
            for (key in js("Object").keys(visited)) {
                newEndpoints.put(key, visited[key] as String)
            }
        }
        return newEndpoints
    }

    private inline fun <U> rustCall(callback: (RustError.ByReference) -> U): U {
        synchronized(this) {
            val e = RustError.ByReference()
            val ret: U = callback(e)
            if (e.isFailure()) {
                throw e.intoException()
            } else {
                return ret
            }
        }
    }

    private inline fun rustCallForString(callback: (RustError.ByReference) -> Pointer?): String {
        val cstring = rustCall(callback)
                ?: throw RuntimeException("Bug: Don't use this function when you can return" +
                        " null on success.")
        try {
            return cstring.getString(0, "utf8")
        } finally {
            LibPushFFI.INSTANCE.places_destroy_string(cstring)
        }
    }
}

/**
 * A class for providing the auth-related information needed to sync.
 * Note that this has the same shape as `SyncUnlockInfo` from logins - we
 * probably want a way of sharing these.
 */

class KeyInfo {
    val auth: String,
    val p256dh: String,
}

class SubscriptionInfo (
    val endpoint: String,
    val keys: KeyInfo,
)

/**
 * An API for interacting with Push.
 */
interface PushAPI {
    /**
     * Get the Subscription Info block
     * 
     * @param channelID Channel ID (UUID) for new subscription
     * @return a Subscription Info structure
     */
    fun getSubscriptionInfo(
        channelID: String,
    ): SubscriptionInfo

    /**
     * Unsubscribe a given channelID
     *
     * @param channelID Channel ID (UUID) for subscription to remove.
     * @return bool.
     */
    fun unsubscribe(channelID: String): Boolean

    /**
     * Updates the Native OS push registration ID.
     * @param registrationToken the new Native OS push registration ID.
     * @return bool
     */
    fun update(registrationToken: String):Boolean

    /**
     * Verifies the connection state. NOTE: If the internal check fails, 
     * endpoints will be re-registered and new endpoints will be returned for 
     * known ChannelIDs
     *
     * @return Map of ChannelID: Endpoint, be sure to notify apps registered to given
     *   channelIDs of the new Endpoint.
     */
    fun verifyConnection(): Map<String, String>

}

open class PushError(msg: String): Exception(msg)
open class InternalError(msg: String): PushError(msg)
open class OpenSSLError(msg: String): PushError(msg)
open class CommunicationError(msg: String): PushError(msg)
open class CommunicationServerError(msg: String): PushError(msg)
open class AlreadyRegisteredError(): PushError("")
open class StorageError(msg: String): PushError(msg)
open class StorageSqlError(msg: String): PushError(msg)
