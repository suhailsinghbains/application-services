/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    commands::send_tab::{
        self, EncryptedSendTabPayload, PrivateSendTabKeys, PublicSendTabKeys, SendTabPayload,
    },
    errors::*,
    http_client::DeviceResponse,
    scopes, FirefoxAccount,
};

impl FirefoxAccount {
    pub fn ensure_send_tab_registered(&mut self) -> Result<()> {
        let own_keys: PrivateSendTabKeys =
            match self.state.commands_data.get(send_tab::COMMAND_NAME) {
                Some(s) => serde_json::from_str(s)?,
                None => {
                    let keys = PrivateSendTabKeys::from_random()?;
                    self.state.commands_data.insert(
                        send_tab::COMMAND_NAME.to_owned(),
                        serde_json::to_string(&keys)?,
                    );
                    self.maybe_call_persist_callback();
                    keys
                }
            };
        let public_keys: PublicSendTabKeys = own_keys.into();
        let kek = self.sync_keys_as_send_tab_kek()?;
        let command_data: String = public_keys.as_command_data(&kek)?;
        self.register_command(send_tab::COMMAND_NAME, &command_data)?;
        Ok(())
    }

    pub fn send_tab(&mut self, target_device_id: &str, title: &str, url: &str) -> Result<()> {
        let devices = self.get_devices()?;
        let target = devices
            .iter()
            .find(|d| d.id == target_device_id)
            .ok_or_else(|| ErrorKind::UnknownTargetDevice(target_device_id.to_owned()))?;
        let payload = SendTabPayload::single_tab(title, url);
        let kek = self.sync_keys_as_send_tab_kek()?;
        let command_payload = send_tab::build_send_command(&kek, target, &payload)?;
        self.invoke_command(send_tab::COMMAND_NAME, target, &command_payload)
    }

    pub(crate) fn handle_send_tab_command(
        &self,
        sender: Option<DeviceResponse>,
        payload: serde_json::Value,
    ) -> Result<(Option<DeviceResponse>, SendTabPayload)> {
        let send_tab_key: PrivateSendTabKeys =
            match self.state.commands_data.get(send_tab::COMMAND_NAME) {
                Some(s) => serde_json::from_str(s)?,
                None => {
                    return Err(ErrorKind::IllegalState(
                        "Cannot find send-tab keys. Has ensure_send_tab been called before?"
                            .to_string(),
                    )
                    .into());
                }
            };
        let encrypted_payload: EncryptedSendTabPayload = serde_json::from_value(payload)?;
        Ok((sender, encrypted_payload.decrypt(&send_tab_key)?))
    }

    fn sync_keys_as_send_tab_kek(&self) -> Result<send_tab::KeyEncryptingKey> {
        let oldsync_key = self.get_scoped_key(scopes::OLD_SYNC)?;
        let ksync = base64::decode_config(&oldsync_key.k, base64::URL_SAFE_NO_PAD)?;
        let kxcs: &str = oldsync_key.kid.splitn(2, '-').collect::<Vec<_>>()[1];
        let kxcs = base64::decode_config(&kxcs, base64::URL_SAFE_NO_PAD)?;
        Ok(send_tab::KeyEncryptingKey::SyncKeys(ksync, kxcs))
    }
}
