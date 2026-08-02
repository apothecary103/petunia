use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::TimeZone;
use futures::{future, stream, AsyncReadExt, Stream, StreamExt};
use libsignal_service::libsignal_account_keys::AccountEntropyPool;
use libsignal_service::prelude::SessionStoreExt;
use libsignal_service::proto::addressable_message::Author;
use libsignal_service::protocol::{ProtocolAddress, SessionStore};
use libsignal_service::provisioning::ProvisioningSecrets;
use libsignal_service::{
    attachment_cipher::decrypt_in_place,
    cipher,
    configuration::{ServiceConfiguration, SignalServers},
    content::{Content, ContentBody, Metadata},
    encrypt_device_name,
    groups_v2::{decrypt_group, GroupMemberCandidate, GroupsManager, InMemoryCredentialsCache},
    master_key::StorageServiceKey,
    messagepipe::{Incoming, MessagePipe, ServiceCredentials},
    prelude::{phonenumber::PhoneNumber, MasterKey, MessageSenderError, ProtobufMessage, Uuid},
    profile_cipher::ProfileCipher,
    proto::{
        contact_record,
        data_message::Delete,
        manifest_record,
        storage_record,
        sync_message::{self, sticker_pack_operation, StickerPackOperation},
        AccountRecord, AttachmentPointer, ContactRecord, DataMessage, EditMessage, GroupContextV2,
        ManifestRecord, NullMessage, SyncMessage, Verified, WriteOperation,
    },
    protocol::{
        Aci, DeviceId, IdentityKeyStore, SenderCertificate, ServiceId, ServiceIdKind, Username,
    },
    provisioning::ProvisioningError,
    push_service::{AvatarWrite, PushService, ServiceIds, DEFAULT_DEVICE_ID},
    receiver::MessageReceiver,
    sender::{AttachmentSpec, AttachmentUploadError},
    sticker_cipher::derive_key,
    unidentified_access::UnidentifiedAccess,
    utils::TryIntoE164,
    websocket::{
        self,
        account::{AccountAttributes, DeviceCapabilities, DeviceInfo, WhoAmIResponse},
        usernames::generate_username_link,
        SignalWebSocket,
    },
    zkgroup::{
        groups::{GroupMasterKey, GroupSecretParams},
        profiles::ProfileKey,
    },
    AccountManager, Profile, ServiceIdExt, StorageService, StorageServiceError,
};
use rand::rng;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};
use url::Url;

use crate::model::contacts::Contact;
use crate::serde::serde_profile_key;
use crate::store::{ContentsStore, Sticker, StickerPack, StickerPackManifest, Store, Thread};
use crate::{model::groups::Group, AvatarBytes, Error, Manager};

pub use crate::model::messages::Received;

type ServiceCipher<S> = cipher::ServiceCipher<S>;
type MessageSender<S> = libsignal_service::prelude::MessageSender<S>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistrationType {
    Primary,
    Secondary,
}

/// Manager state when the client is registered and can send and receive messages from Signal
pub struct Registered {
    pub(crate) identified_push_service: OnceLock<PushService>,
    pub(crate) unidentified_push_service: OnceLock<PushService>,
    pub(crate) identified_websocket: Arc<Mutex<Option<SignalWebSocket<websocket::Identified>>>>,
    pub(crate) unidentified_websocket: Arc<Mutex<Option<SignalWebSocket<websocket::Unidentified>>>>,
    pub(crate) unidentified_sender_certificate: Arc<Mutex<Option<SenderCertificate>>>,

    pub(crate) data: RegistrationData,
}

impl fmt::Debug for Registered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registered").finish_non_exhaustive()
    }
}

impl Registered {
    pub(crate) fn with_data(data: RegistrationData) -> Self {
        Self {
            identified_push_service: Default::default(),
            unidentified_push_service: Default::default(),
            identified_websocket: Default::default(),
            unidentified_websocket: Default::default(),
            unidentified_sender_certificate: Default::default(),
            data,
        }
    }

    fn servers(&self) -> SignalServers {
        self.data.signal_servers
    }

    fn service_configuration(&self) -> ServiceConfiguration {
        self.servers().into()
    }

    pub fn device_id(&self) -> DeviceId {
        self.data
            .device_id
            .and_then(|d| d.try_into().ok())
            .unwrap_or(*DEFAULT_DEVICE_ID)
    }

    pub(crate) fn identified_push_service(&self) -> PushService {
        self.identified_push_service
            .get_or_init(|| {
                PushService::new(self.servers(), Some(self.credentials()), crate::USER_AGENT)
            })
            .clone()
    }

    pub(crate) fn credentials(&self) -> ServiceCredentials {
        ServiceCredentials {
            aci: Some(self.data.service_ids.aci),
            pni: Some(self.data.service_ids.pni),
            phonenumber: (&self.data.phone_number)
                .try_into_e164()
                .expect("valid phone number"),
            password: Some(self.data.password.clone()),
            device_id: self.data.device_id.and_then(|d| d.try_into().ok()),
        }
    }
}

/// Registration data like device name, and credentials to connect to Signal
#[derive(Serialize, Deserialize, Clone)]
pub struct RegistrationData {
    pub signal_servers: SignalServers,
    pub device_name: Option<String>,
    pub phone_number: PhoneNumber,
    #[serde(flatten)]
    pub service_ids: ServiceIds,
    pub(crate) password: String,
    pub device_id: Option<u32>,
    pub registration_id: u32,
    #[serde(default)]
    pub pni_registration_id: Option<u32>,
    #[serde(with = "serde_profile_key")]
    pub(crate) profile_key: ProfileKey,
}

impl RegistrationData {
    /// Our own profile key
    pub fn profile_key(&self) -> ProfileKey {
        self.profile_key
    }

    /// The name of the device (if linked as secondary)
    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
    }
}

impl<S: Store> Manager<S, Registered> {
    /// Loads a previously registered account from the implemented [Store].
    ///
    /// Returns a instance of [Manager] you can use to send & receive messages.
    pub async fn load_registered(store: S) -> Result<Self, Error<S::Error>> {
        let registration_data = store
            .load_registration_data()
            .await?
            .ok_or(Error::NotYetRegisteredError)?;

        let registered = Registered::with_data(registration_data);

        if let Some(sender_certificate) = store.sender_certificate().await? {
            registered
                .unidentified_sender_certificate
                .lock()
                .await
                .replace(sender_certificate);
        }

        Ok(Self {
            store,
            state: Arc::new(registered),
        })
    }

    /// Returns a handle to the [Store] implementation.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Returns a handle on the [RegistrationData].
    pub fn registration_data(&self) -> &RegistrationData {
        &self.state.data
    }

    /// Returns a clone of a cached push service (with credentials).
    ///
    /// If no service is yet cached, it will create and cache one.
    fn identified_push_service(&self) -> PushService {
        self.state.identified_push_service()
    }

    /// Returns a clone of a cached push service (without credentials).
    ///
    /// If no service is yet cached, it will create and cache one.
    fn unidentified_push_service(&self) -> PushService {
        self.state
            .unidentified_push_service
            .get_or_init(|| PushService::new(self.state.servers(), None, crate::USER_AGENT))
            .clone()
    }

    /// Returns the current identified websocket, or creates a new one
    ///
    /// A new one is created if the current websocket is closed, or if there is none yet.
    async fn identified_websocket(
        &self,
        require_unused: bool,
    ) -> Result<SignalWebSocket<websocket::Identified>, Error<S::Error>> {
        let mut identified_ws = self.state.identified_websocket.lock().await;
        match identified_ws
            .as_ref()
            .filter(|ws| !ws.is_closed())
            .filter(|ws| !(require_unused && ws.is_used()))
        {
            Some(ws) => Ok(ws.clone()),
            None => {
                let headers = &[("X-Signal-Receive-Stories", "false")];
                let ws = self
                    .identified_push_service()
                    .ws(
                        "/v1/websocket/",
                        "/v1/keepalive",
                        headers,
                        Some(self.credentials()),
                    )
                    .await?;
                identified_ws.replace(ws.clone());
                debug!("initialized identified websocket");

                Ok(ws)
            }
        }
    }

    /// Opens a *new* identified websocket for the message stream, replacing
    /// whatever was cached.
    ///
    /// `identified_websocket(true)` hands back the cached socket whenever it is
    /// not already carrying a stream, and on a reconnect that socket is routinely
    /// a zombie: a websocket whose peer stopped listening without sending a close
    /// frame — the machine slept, the network changed — stays open on this side
    /// until a keep-alive goes unanswered, which is a minute or two away. Handed
    /// that socket, the new stream ends the moment it is read, so the caller
    /// reconnects, is handed the same dead socket again, and reports itself as
    /// reconnecting every few seconds while delivering nothing.
    ///
    /// Dropping the cached handle is what closes the old socket: the process
    /// behind it ends when the last sender for its request channel goes.
    async fn fresh_identified_websocket(
        &self,
    ) -> Result<SignalWebSocket<websocket::Identified>, Error<S::Error>> {
        let mut identified_ws = self.state.identified_websocket.lock().await;
        let headers = &[("X-Signal-Receive-Stories", "false")];
        let ws = self
            .identified_push_service()
            .ws(
                "/v1/websocket/",
                "/v1/keepalive",
                headers,
                Some(self.credentials()),
            )
            .await?;
        identified_ws.replace(ws.clone());
        debug!("opened a fresh identified websocket");

        Ok(ws)
    }

    /// Returns the current unidentified websocket, or creates a new one
    ///
    /// A new one is created if the current websocket is closed, or if there is none yet.
    async fn unidentified_websocket(
        &self,
    ) -> Result<SignalWebSocket<websocket::Unidentified>, Error<S::Error>> {
        let mut unidentified_ws = self.state.unidentified_websocket.lock().await;
        match unidentified_ws.as_ref().filter(|ws| !ws.is_closed()) {
            Some(ws) => Ok(ws.clone()),
            None => {
                let ws = self
                    .unidentified_push_service()
                    .ws("/v1/websocket/", "/v1/keepalive", &[], None)
                    .await?;
                unidentified_ws.replace(ws.clone());
                debug!("initialized unidentified websocket");

                Ok(ws)
            }
        }
    }

    /// Request the primary device to encrypt & send all of its contacts.
    ///
    /// **Note**: If successful, the contacts are not yet received and stored, but will only be
    /// processed when they're received after polling on the
    pub async fn request_contacts(&mut self) -> Result<(), Error<S::Error>> {
        trace!("requesting contacts sync");
        let sync_message = SyncMessage {
            request: Some(sync_message::Request {
                r#type: Some(sync_message::request::Type::Contacts.into()),
            }),
            ..SyncMessage::with_padding(&mut rand::rng())
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        self.send_message(self.state.data.service_ids.aci(), sync_message, timestamp)
            .await?;

        Ok(())
    }

    async fn sender_certificate(&self) -> Result<SenderCertificate, Error<S::Error>> {
        let needs_renewal = |sender_certificate: Option<&SenderCertificate>| -> bool {
            if sender_certificate.is_none() {
                return true;
            }

            let seconds_since_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();

            if let Some(expiration) = sender_certificate.and_then(|s| s.expiration().ok()) {
                expiration.epoch_millis() / 1000 <= seconds_since_epoch + 600
            } else {
                true
            }
        };

        let mut unidentified_sender_certificate =
            self.state.unidentified_sender_certificate.lock().await;
        if needs_renewal(unidentified_sender_certificate.as_ref()) {
            let sender_certificate = self
                .identified_websocket(false)
                .await?
                .get_uuid_only_sender_certificate()
                .await?;
            self.store
                .save_sender_certificate(&sender_certificate)
                .await?;
            unidentified_sender_certificate.replace(sender_certificate);
        }

        Ok(unidentified_sender_certificate
            .clone()
            .expect("logic error"))
    }

    async fn master_key(&self) -> Result<Option<MasterKey>, Error<S::Error>> {
        let from_store = self.store().fetch_master_key().await?;

        if let Some(key) = from_store {
            Ok(Some(key))
        } else {
            let aep = self.account_entropy_pool().await?;
            Ok(aep.map(|aep| {
                MasterKey::from_slice(aep.derive_svr_key().as_slice())
                    .expect("Derived SVR key from account entropy pool to be a valid master key")
            }))
        }
    }

    async fn account_entropy_pool(&self) -> Result<Option<AccountEntropyPool>, Error<S::Error>> {
        let from_store = self.store().fetch_account_entropy_pool().await?;

        if let Some(key) = from_store {
            Ok(Some(key))
        } else if self.registration_type() == RegistrationType::Primary {
            let key = AccountEntropyPool::generate(&mut rand::rng());
            self.store().store_account_entropy_pool(Some(&key)).await?;
            Ok(Some(key))
        } else {
            Ok(None)
        }
    }

    pub async fn submit_recaptcha_challenge(
        &self,
        token: &str,
        captcha: &str,
    ) -> Result<(), Error<S::Error>> {
        let mut account_manager = AccountManager::new(
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            None,
        );
        account_manager
            .submit_recaptcha_challenge(token, captcha)
            .await?;
        Ok(())
    }

    /// Fetches basic information on the registered device.
    pub async fn whoami(&self) -> Result<WhoAmIResponse, Error<S::Error>> {
        Ok(self.identified_websocket(false).await?.whoami().await?)
    }

    pub fn device_id(&self) -> DeviceId {
        self.state.device_id()
    }

    /// Fetches the profile (name, about, status emoji) of the registered user.
    pub async fn retrieve_profile(&mut self) -> Result<Profile, Error<S::Error>> {
        self.retrieve_profile_by_uuid(self.state.data.service_ids.aci, self.state.data.profile_key)
            .await
    }

    /// Fetches the profile of the provided user by UUID and profile key.
    pub async fn retrieve_profile_by_uuid(
        &mut self,
        aci: impl Into<Aci>,
        profile_key: ProfileKey,
    ) -> Result<Profile, Error<S::Error>> {
        let aci = aci.into();

        // Check if profile is cached.
        // TODO: Create a migration in the store removing all profiles.
        // TODO: Is there some way to know if this is outdated?
        if let Some(profile) = self
            .store
            .profile(aci.into(), profile_key)
            .await
            .ok()
            .flatten()
        {
            return Ok(profile);
        }

        let mut account_manager = AccountManager::new(
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            Some(profile_key),
        );

        let profile = account_manager.retrieve_profile(aci).await?;

        let _ = self
            .store
            .save_profile(aci.into(), profile_key, profile.clone())
            .await;
        Ok(profile)
    }

    /// Updates the user's profile information, retaining the current avatar.
    pub async fn update_profile(
        &mut self,
        name: libsignal_service::profile_name::ProfileName<String>,
        about: Option<String>,
        emoji: Option<String>,
    ) -> Result<(), Error<S::Error>> {
        self.update_profile_with_avatar(name, about, emoji, None)
            .await
    }

    /// Updates the user's profile information and, when `avatar` carries the
    /// bytes of an image, replaces the profile avatar with it. `None` retains
    /// the avatar already set.
    pub async fn update_profile_with_avatar(
        &mut self,
        name: libsignal_service::profile_name::ProfileName<String>,
        about: Option<String>,
        emoji: Option<String>,
        avatar: Option<Vec<u8>>,
    ) -> Result<(), Error<S::Error>> {
        let aci = self.state.data.service_ids.aci();
        let mut account_manager = AccountManager::new(
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            Some(self.state.data.profile_key),
        );

        let mut avatar = avatar.map(std::io::Cursor::new);
        account_manager
            .upload_versioned_profile::<std::io::Cursor<Vec<u8>>, _, String>(
                aci,
                name,
                about,
                emoji,
                match &mut avatar {
                    Some(reader) => AvatarWrite::NewAvatar(reader),
                    None => AvatarWrite::RetainAvatar,
                },
                &mut rand::rng(),
            )
            .await?;

        // Retrieve and save locally so we have the updated version
        let profile = account_manager.retrieve_profile(aci).await?;
        let _ = self
            .store
            .save_profile(aci.into(), self.state.data.profile_key, profile)
            .await;

        Ok(())
    }

    pub async fn retrieve_group_avatar(
        &mut self,
        context: GroupContextV2,
    ) -> Result<Option<AvatarBytes>, Error<S::Error>> {
        let master_key_bytes = context
            .master_key()
            .try_into()
            .expect("Master key bytes to be of size 32.");

        // Check if group avatar is cached.
        // TODO: Is there some way to know if this is outdated?
        if let Some(avatar) = self
            .store
            .group_avatar(master_key_bytes)
            .await
            .ok()
            .flatten()
        {
            return Ok(Some(avatar));
        }

        let mut gm = Box::pin(self.groups_manager()).await?;
        let Some(group) = upsert_group(
            &self.store,
            &mut gm,
            context.master_key(),
            &context.revision(),
        )
        .await?
        else {
            return Ok(None);
        };

        // Empty path means no avatar was set.
        if group.avatar.is_empty() {
            return Ok(None);
        }

        let avatar = gm
            .retrieve_avatar(
                &group.avatar,
                GroupSecretParams::derive_from_master_key(GroupMasterKey::new(master_key_bytes)),
            )
            .await?;
        if let Some(avatar) = &avatar {
            let _ = self.store.save_group_avatar(master_key_bytes, avatar).await;
        }
        Ok(avatar)
    }

    pub async fn retrieve_profile_avatar_by_uuid(
        &mut self,
        uuid: Uuid,
        profile_key: ProfileKey,
    ) -> Result<Option<AvatarBytes>, Error<S::Error>> {
        // Check if profile avatar is cached.
        // TODO: Is there some way to know if this is outdated?
        if let Some(avatar) = self
            .store
            .profile_avatar(uuid, profile_key)
            .await
            .ok()
            .flatten()
        {
            return Ok(Some(avatar));
        }

        let profile =
            if let Some(profile) = self.store.profile(uuid, profile_key).await.ok().flatten() {
                profile
            } else {
                self.retrieve_profile_by_uuid(uuid, profile_key).await?
            };

        let Some(avatar) = profile.avatar.as_ref() else {
            return Ok(None);
        };

        let mut websocket = self.unidentified_websocket().await?;

        let mut avatar_stream = websocket.retrieve_profile_avatar(avatar).await?;
        // 10MB is what Signal Android allocates
        let mut contents = Vec::with_capacity(10 * 1024 * 1024);
        let len = avatar_stream.read_to_end(&mut contents).await?;
        contents.truncate(len);

        let cipher = ProfileCipher::new(profile_key);

        let avatar = cipher.decrypt_avatar(&contents)?;
        let _ = self
            .store
            .save_profile_avatar(uuid, profile_key, &avatar)
            .await;
        Ok(Some(avatar))
    }

    async fn groups_manager(
        &self,
    ) -> Result<GroupsManager<InMemoryCredentialsCache>, Error<S::Error>> {
        let service_configuration = self.state.service_configuration();
        let server_public_params = service_configuration.zkgroup_server_public_params;

        let groups_credentials_cache = InMemoryCredentialsCache::default();
        let groups_manager = GroupsManager::new(
            self.state.data.service_ids.clone(),
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            self.unidentified_websocket().await?,
            groups_credentials_cache,
            server_public_params,
        );

        Ok(groups_manager)
    }

    /// Starts receiving and storing messages.
    ///
    /// As a client, it is heavily recommended to process incoming messages and wait for the `Received::QueueEmpty` messages
    /// until giving the ability for users to send messages. That way, all possible updates (sessions, profile keys, sender keys)
    /// are processed _before_ trying to encrypt and send messages, which might get rejected by recipients otherwise.
    ///
    /// Returns a [futures::Stream] of messages to consume. Messages will also be stored by the implementation of the [Store].
    pub async fn receive_messages(
        &mut self,
    ) -> Result<impl Stream<Item = Received>, Error<S::Error>> {
        struct StreamState<Receiver, Store, AciStore, PniStore> {
            store: Store,
            identified_websocket: SignalWebSocket<websocket::Identified>,
            unidentified_websocket: SignalWebSocket<websocket::Unidentified>,
            encrypted_messages: Receiver,
            message_receiver: MessageReceiver,
            service_cipher_aci: ServiceCipher<AciStore>,
            service_cipher_pni: ServiceCipher<PniStore>,
            groups_manager: GroupsManager<InMemoryCredentialsCache>,
            service_ids: ServiceIds,
            message_sender: MessageSender<AciStore>,
            master_key: Option<MasterKey>,
            account_entropy_pool: Option<AccountEntropyPool>,
            registration_type: RegistrationType,
        }

        let identified_push_service = self.identified_push_service();
        // NB: here, we initialise a *fresh* Signal websocket, which means any other use of the previous one will go into nirvana
        let identified_websocket = self.fresh_identified_websocket().await?;

        let mut account_manager = AccountManager::new(
            identified_push_service.clone(),
            identified_websocket.clone(),
            None,
        );

        let store_inner = self.store.clone();
        let registration_data_inner = self.registration_data().clone();

        // We make a task to update the account attributes and refresh pre keys as needed that will
        // only yield a value if one of the two operations fail (stop signal).
        //
        // This is necessary because in this context, we can't do the classic tokio::spawn with a
        // oneshot::channel() or CancellationToken because of !Send constraints in the Store.
        let refresh_registration_task = async move {
            if let Err(error) =
                set_account_attributes(&mut account_manager, &store_inner, &registration_data_inner)
                    .await
            {
                error!(%error, "failed to set account attributes, this is problematic and should never happen!");
            }

            if let Err(error) = register_pre_keys(&store_inner, &mut account_manager).await {
                error!(%error, "failed to register pre-keys, this is problematic and should never happen!");
            }

            // Never return, which keeps the messages stream alive.
            future::pending::<()>().await
        };

        let encrypted_messages = MessagePipe::from_socket(identified_websocket.clone());

        let init = StreamState {
            store: self.store.clone(),
            identified_websocket,
            unidentified_websocket: self.unidentified_websocket().await?,
            encrypted_messages: Box::pin(encrypted_messages.stream()),
            message_receiver: MessageReceiver::new(identified_push_service),
            service_cipher_aci: self.new_service_cipher_aci(),
            service_cipher_pni: self.new_service_cipher_pni(),
            groups_manager: Box::pin(self.groups_manager()).await?,
            service_ids: self.state.data.service_ids.clone(),
            message_sender: self.new_message_sender().await?,
            master_key: self.master_key().await?,
            account_entropy_pool: self.account_entropy_pool().await?,
            registration_type: self.registration_type(),
        };

        debug!("starting to consume incoming message stream");

        let incoming_messages_stream = futures::stream::unfold(init, |mut state| {
            async move {
                loop {
                    match state.encrypted_messages.next().await {
                        Some(Ok(Incoming::Envelope(envelope))) => {
                            let envelope = {
                                // the permit is released at the end of the block (impl Drop)
                                match ServiceId::parse_from_service_id_string(
                                    envelope.destination_service_id(),
                                ) {
                                    None | Some(ServiceId::Aci(_)) => {
                                        state
                                            .service_cipher_aci
                                            .open_envelope(envelope, &mut rng())
                                            .await
                                    }
                                    Some(ServiceId::Pni(pni)) => {
                                        if pni == state.service_ids.pni()
                                            && envelope.source_service_id.is_none()
                                        {
                                            warn!("Got a sealed sender message to our PNI? Invalid message, ignoring.");
                                            continue;
                                        }
                                        state
                                            .service_cipher_pni
                                            .open_envelope(envelope, &mut rng())
                                            .await
                                    }
                                }
                            };
                            match envelope {
                                Ok(Some(content)) => {
                                    if let ContentBody::DecryptionErrorMessage(e) = &content.body {
                                        error!(
                                            error = ?e,
                                            "got error decrypting a message"
                                        );
                                        continue;
                                    }

                                    if let ContentBody::SynchronizeMessage(SyncMessage {
                                        request: Some(request),
                                        ..
                                    }) = &content.body
                                    {
                                        use libsignal_service::content::sync_message::request::Type as RequestType;

                                        match request.r#type() {
                                            RequestType::Contacts => {
                                                let contacts = state
                                                    .store
                                                    .contacts()
                                                    .await
                                                    .map(|i| {
                                                        i.collect::<Result<Vec<_>, _>>()
                                                            .unwrap_or_default()
                                                    })
                                                    .unwrap_or_default();

                                                let mut message_sender =
                                                    state.message_sender.clone();
                                                let aci = state.service_ids.aci();
                                                tokio::task::spawn_local(async move {
                                                    let result = message_sender
                                                    .send_contact_details(
                                                        &ServiceId::Aci(aci),
                                                        None,
                                                        contacts.into_iter().map(|c| libsignal_service::sender::ContactDetails {
                                                            number: c.phone_number.map(|p| p.to_string()),
                                                            aci: Some(c.uuid.to_string()),
                                                            aci_binary: Some(c.uuid.into_bytes().into()),
                                                            name: Some(c.name),
                                                            avatar: c.avatar.map(|a| libsignal_service::proto::contact_details::Avatar {
                                                                content_type: Some(a.content_type),
                                                                length: a.reader.len().try_into().ok(),
                                                            }),
                                                            expire_timer: Some(c.expire_timer),
                                                            expire_timer_version: Some(c.expire_timer_version),
                                                            inbox_position: None,
                                                        }),
                                                        false,
                                                        true,
                                                    )
                                                    .await;

                                                    if let Err(error) = result {
                                                        warn!(%error, "Error sending contact details to other devices");
                                                    }
                                                });
                                            }
                                            RequestType::Keys => {
                                                let mut message_sender =
                                                    state.message_sender.clone();
                                                let account_entropy_pool = state
                                                    .account_entropy_pool
                                                    .as_ref()
                                                    .map(|aep| aep.to_string());
                                                let master = state
                                                    .master_key
                                                    .as_ref()
                                                    .map(|m| m.inner.to_vec());
                                                tokio::task::spawn_local(async move {
                                                    let result = message_sender.send_sync_message(SyncMessage {
                                                        keys: Some(libsignal_service::content::sync_message::Keys {
                                                            master,
                                                            account_entropy_pool,
                                                            media_root_backup_key: None,
                                                        }),
                                                        ..SyncMessage::with_padding(&mut rand::rng())
                                                    }).await;

                                                    if let Err(error) = result {
                                                        warn!(%error, "Error sending keys to other devices");
                                                    }
                                                });
                                            }
                                            RequestType::Blocked => {
                                                warn!("storing blocked user is not implemented yet! we will not report blocked users to the device requesting the sync.");
                                                let mut message_sender =
                                                    state.message_sender.clone();
                                                tokio::task::spawn_local(async move {
                                                    let result = message_sender.send_sync_message(SyncMessage {
                                                    blocked: Some(libsignal_service::content::sync_message::Blocked {
                                                        numbers: vec![],
                                                        acis: vec![],
                                                        acis_binary: vec![],
                                                        group_ids: vec![],
                                                    }),
                                                    ..SyncMessage::with_padding(&mut rand::rng())
                                                }).await;

                                                    if let Err(error) = result {
                                                        warn!(%error, "Error sending blocked contacts to other devices");
                                                    }
                                                });
                                            }
                                            t => {
                                                info!(type = ?t, "Got sync request of currently unhandled type")
                                            }
                                        }
                                    }

                                    // contacts synchronization sent from the primary device (happens after linking, or on demand)
                                    if let ContentBody::SynchronizeMessage(SyncMessage {
                                        contacts: Some(contacts),
                                        ..
                                    }) = &content.body
                                    {
                                        match state
                                            .message_receiver
                                            .retrieve_contacts(contacts)
                                            .await
                                        {
                                            Ok(contacts) => {
                                                info!("saving contacts");
                                                for contact in contacts.filter_map(Result::ok) {
                                                    if let Err(error) = state
                                                        .store
                                                        .save_contact(&contact.into())
                                                        .await
                                                    {
                                                        warn!(%error, "failed to save contacts");
                                                        break;
                                                    }
                                                }
                                            }
                                            Err(error) => {
                                                warn!(%error, "failed to retrieve contacts");
                                            }
                                        }

                                        return Some((Received::Contacts, state));
                                    }

                                    // sticker pack operations
                                    if let ContentBody::SynchronizeMessage(SyncMessage {
                                        sticker_pack_operation,
                                        ..
                                    }) = &content.body
                                    {
                                        for operation in sticker_pack_operation {
                                            match operation.r#type() {
                                                sticker_pack_operation::Type::Install => {
                                                    let store = state.store.clone();
                                                    let unidentified_websocket =
                                                        state.unidentified_websocket.clone();
                                                    let operation = operation.clone();

                                                    // download stickers in the background
                                                    tokio::spawn(async move {
                                                        match download_sticker_pack(
                                                            store,
                                                            unidentified_websocket,
                                                            &operation,
                                                        )
                                                        .await
                                                        {
                                                            Ok(sticker_pack) => {
                                                                debug!(
                                                                "downloaded sticker pack: {} made by {}",
                                                                sticker_pack.manifest.title,
                                                                sticker_pack.manifest.author
                                                            );
                                                            }
                                                            Err(error) => error!(
                                                                %error,
                                                                "failed to download sticker pack"
                                                            ),
                                                        }
                                                    });
                                                }
                                                sticker_pack_operation::Type::Remove => match state
                                                    .store
                                                    .remove_sticker_pack(operation.pack_id())
                                                    .await
                                                {
                                                    Ok(was_present) => {
                                                        debug!(was_present, "removed stick pack")
                                                    }
                                                    Err(error) => {
                                                        error!(
                                                            %error,
                                                            "failed to remove sticker pack"
                                                        )
                                                    }
                                                },
                                            }
                                        }
                                    }

                                    // key synchronization sent from the primary device
                                    if let ContentBody::SynchronizeMessage(SyncMessage {
                                        keys: Some(keys),
                                        ..
                                    }) = &content.body
                                    {
                                        debug!("received key sync message");
                                        if state.registration_type == RegistrationType::Primary {
                                            warn!("received a key sync message as a primary device; ignoring")
                                        } else {
                                            match keys
                                                .account_entropy_pool
                                                .as_ref()
                                                .map(|s| AccountEntropyPool::from_str(s))
                                            {
                                                Some(Ok(aep)) => {
                                                    if let Err(error) = state
                                                        .store
                                                        .store_account_entropy_pool(Some(&aep))
                                                        .await
                                                    {
                                                        error!(%error, "failed to store account entropy pool");
                                                    }
                                                    state.account_entropy_pool = Some(aep);
                                                }
                                                Some(Err(error)) => {
                                                    warn!(%error, "cannot convert account entropy pool from string")
                                                }
                                                None => {}
                                            }
                                            match keys
                                                .master
                                                .as_ref()
                                                .map(|m| MasterKey::from_slice(m.as_slice()))
                                            {
                                                Some(Ok(master)) => {
                                                    if let Err(error) = state
                                                        .store
                                                        .store_master_key(Some(&master))
                                                        .await
                                                    {
                                                        error!(%error, "failed to store master key");
                                                    }
                                                    state.master_key = Some(master);
                                                }
                                                Some(Err(error)) => {
                                                    warn!(%error, "cannot convert master key from bytes; trying to populate from account entropy pool");
                                                    if let Some(aep) =
                                                        state.account_entropy_pool.as_ref()
                                                    {
                                                        state.master_key = Some(MasterKey::from_slice(aep.derive_svr_key().as_slice()).expect("svr key derived from account entropy pool to be a master key"));
                                                    }
                                                }
                                                None => {
                                                    trace!("master key not given in the sync message; trying to populate from account entropy pool");
                                                    if let Some(aep) =
                                                        state.account_entropy_pool.as_ref()
                                                    {
                                                        state.master_key = Some(MasterKey::from_slice(aep.derive_svr_key().as_slice()).expect("svr key derived from account entropy pool to be a master key"));
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // group update
                                    if let ContentBody::DataMessage(DataMessage {
                                        group_v2:
                                            Some(GroupContextV2 {
                                                master_key: Some(master_key_bytes),
                                                revision: Some(revision),
                                                ..
                                            }),
                                        ..
                                    })
                                    | ContentBody::SynchronizeMessage(SyncMessage {
                                        sent:
                                            Some(sync_message::Sent {
                                                message:
                                                    Some(DataMessage {
                                                        group_v2:
                                                            Some(GroupContextV2 {
                                                                master_key: Some(master_key_bytes),
                                                                revision: Some(revision),
                                                                ..
                                                            }),
                                                        ..
                                                    }),
                                                ..
                                            }),
                                        ..
                                    }) = &content.body
                                    {
                                        // there's two things to implement: the group metadata (fetched from HTTP API)
                                        // and the group changes, which are part of the protobuf messages
                                        // this means we kinda need our own internal representation of groups inside of presage?
                                        if let Ok(Some(group)) = upsert_group(
                                            &state.store,
                                            &mut state.groups_manager,
                                            master_key_bytes,
                                            revision,
                                        )
                                        .await
                                        {
                                            trace!(?group, "upserted group");
                                        }
                                    }

                                    if let Err(error) = save_message(
                                        &mut state.store,
                                        &mut state.identified_websocket,
                                        content.clone(),
                                        None,
                                    )
                                    .await
                                    {
                                        error!(%error, "error saving message to store");
                                    }

                                    return Some((Received::Content(Box::new(content)), state));
                                }
                                Ok(None) => {
                                    debug!("empty envelope, message will be skipped!")
                                }
                                Err(error) => {
                                    error!(%error, "error opening envelope, message will be skipped!");
                                }
                            }
                        }
                        Some(Ok(Incoming::QueueEmpty)) => {
                            debug!("got empty queue");
                            if state.account_entropy_pool.is_none() {
                                debug!("device does not have the needed keys; requesting from primary device");

                                let mut message_sender = state.message_sender.clone();
                                tokio::task::spawn_local(async move {
                                    let result = message_sender
                                        .send_sync_message(SyncMessage {
                                            request: Some(sync_message::Request {
                                                r#type: Some(
                                                    sync_message::request::Type::Keys.into(),
                                                ),
                                            }),
                                            ..SyncMessage::with_padding(&mut rand::rng())
                                        })
                                        .await;

                                    if let Err(error) = result {
                                        warn!(%error, "Error sending blocked contacts to other devices");
                                    }
                                });
                            }
                            return Some((Received::QueueEmpty, state));
                        }
                        Some(Err(error)) => {
                            error!(%error, "unexpected error in message receiving loop")
                        }
                        None => return None,
                    }
                }
            }
        });

        Ok(Box::pin(
            // We use the returning of the async closure in take_until as a stop signal
            // if the future resolves *anything* the stream will end.
            incoming_messages_stream.take_until(refresh_registration_task),
        ))
    }

    /// Uses Signal's SGX contact discovery service to resolve a phone number to its matching account identity
    #[cfg(feature = "cdsi")]
    pub async fn discover_contacts_by_phone_number<P: TryIntoE164>(
        &mut self,
        phone_numbers: impl IntoIterator<Item = P>,
    ) -> Result<Vec<(PhoneNumber, Option<ServiceId>)>, Error<S::Error>> {
        use libsignal_service::websocket::directory::LookupRequest;

        let mut ws = self.identified_websocket(false).await?;

        let lookup_request = LookupRequest {
            new_e164s: phone_numbers
                .into_iter()
                .filter_map(|p| p.try_into_e164().ok())
                .collect(),
            ..Default::default()
        };

        Ok(ws
            .discover_contacts(lookup_request)
            .await?
            .into_iter()
            .map(|(e164, service_id)| {
                use libsignal_service::utils::phonenumber_from_signal;
                (phonenumber_from_signal(&e164), service_id)
            })
            .collect())
    }

    /// Resolves a username (which has a text part and an additional random number) to its account identity
    /// for sending messages.
    pub async fn lookup_username(
        &mut self,
        username: &str,
    ) -> Result<Option<Aci>, Error<S::Error>> {
        let username = Username::new(username)?;
        let mut ws = self.unidentified_websocket().await?;
        let resolved_username = ws.look_up_username(&username).await?;
        Ok(resolved_username)
    }

    /// Sends a messages to the provided [ServiceId].
    /// The timestamp should be set to now and is used by Signal mobile apps
    /// to order messages later, and apply reactions.
    ///
    /// This method will automatically update the [DataMessage::expire_timer] if it is set to
    /// [None] such that the chat will keep the current expire timer. If the expire timer is set,
    /// it will be used as is, and the expire timer version will be incremented.
    pub async fn send_message(
        &mut self,
        recipient: impl Into<ServiceId>,
        message: impl Into<ContentBody>,
        timestamp: u64,
    ) -> Result<(), Error<S::Error>> {
        let mut sender = self.new_message_sender().await?;
        let recipient = recipient.into();

        let online_only = false;
        // TODO: Populate this flag based on the recipient information
        //
        // Issue <https://github.com/whisperfish/presage/issues/252>
        let include_pni_signature = false;
        let thread = Thread::Contact(recipient);
        let mut content_body: ContentBody = message.into();

        self.restore_thread_timer(&thread, &mut content_body).await;

        let sender_certificate = self.sender_certificate().await?;
        let unidentified_access = self
            .store
            .profile_key(&recipient)
            .await?
            .map(|profile_key| UnidentifiedAccess {
                key: profile_key.derive_access_key().to_vec(),
                certificate: sender_certificate.clone(),
            });

        // we need to put our profile key in DataMessage
        if let ContentBody::DataMessage(message) = &mut content_body {
            message
                .profile_key
                .get_or_insert(self.state.data.profile_key().get_bytes().to_vec());
            message.required_protocol_version = Some(0);
        }

        ensure_data_message_timestamp(&mut content_body, timestamp);

        sender
            .send_message(
                &recipient,
                unidentified_access,
                content_body.clone(),
                timestamp,
                include_pni_signature,
                online_only,
            )
            .await?;

        // save the message
        let content = Content {
            metadata: Metadata {
                sender: self.state.data.service_ids.aci().into(),
                sender_device: self.state.device_id(),
                destination: recipient,
                server_guid: None,
                timestamp: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
                // Note: Currently no way to get the timestamp the server received the message; just use our timestamp as a fallback.
                server_timestamp: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
                needs_receipt: false,
                unidentified_sender: false,
                was_plaintext: false,
            },
            body: content_body,
        };

        let mut identified_websocket = self.identified_websocket(false).await?;
        save_message(
            &mut self.store,
            &mut identified_websocket,
            content,
            Some(thread),
        )
        .await?;

        Ok(())
    }

    /// Uploads one attachment prior to linking them in a message.
    pub async fn upload_attachment(
        &self,
        spec: AttachmentSpec,
        contents: Vec<u8>,
    ) -> Result<Result<AttachmentPointer, AttachmentUploadError>, Error<S::Error>> {
        Ok(self
            .new_message_sender()
            .await?
            .upload_attachment(spec, contents, &mut rng())
            .await)
    }

    /// Uploads attachments prior to linking them in a message.
    pub async fn upload_attachments(
        &self,
        attachments: Vec<(AttachmentSpec, Vec<u8>)>,
    ) -> Result<Vec<Result<AttachmentPointer, AttachmentUploadError>>, Error<S::Error>> {
        if attachments.is_empty() {
            return Ok(Vec::new());
        }
        let sender = self.new_message_sender().await?;
        let upload = future::join_all(attachments.into_iter().map(move |(spec, contents)| {
            let mut sender = sender.clone();
            async move { sender.upload_attachment(spec, contents, &mut rng()).await }
        }));
        Ok(upload.await)
    }

    /// Sends one message in a group (v2). The `master_key_bytes` is required to have 32 elements.
    ///
    /// This method will automatically update the [DataMessage::expire_timer] if it is set to
    /// [None] such that the chat will keep the current expire timer.
    pub async fn send_message_to_group(
        &mut self,
        master_key_bytes: &[u8],
        message: impl Into<ContentBody>,
        timestamp: u64,
    ) -> Result<(), Error<S::Error>> {
        let mut content_body = message.into();
        let master_key_bytes = master_key_bytes
            .try_into()
            .expect("Master key bytes to be of size 32.");
        let thread = Thread::Group(master_key_bytes);

        self.restore_thread_timer(&thread, &mut content_body).await;
        ensure_data_message_timestamp(&mut content_body, timestamp);

        let mut sender = self.new_message_sender().await?;

        let mut groups_manager = Box::pin(self.groups_manager()).await?;
        let Some(group) =
            upsert_group(&self.store, &mut groups_manager, &master_key_bytes, &0).await?
        else {
            return Err(Error::UnknownGroup);
        };

        let sender_certificate = self.sender_certificate().await?;
        let mut recipients = Vec::new();
        for member in group
            .members
            .into_iter()
            .filter(|m| m.aci != self.state.data.service_ids.aci())
        {
            let unidentified_access =
                self.store
                    .profile_key(&member.aci.into())
                    .await?
                    .map(|profile_key| UnidentifiedAccess {
                        key: profile_key.derive_access_key().to_vec(),
                        certificate: sender_certificate.clone(),
                    });
            let include_pni_signature = false;
            recipients.push((
                member.aci.into(),
                unidentified_access,
                include_pni_signature,
            ));
        }

        let online_only = false;
        let results = sender
            .send_message_to_group(recipients, content_body.clone(), timestamp, online_only)
            .await;

        // TODO: Handle the NotFound error in the future by removing all sessions to this UUID and marking it as unregistered, not sending any messages to this contact anymore.
        results
            .into_iter()
            .find(|res| match res {
                Ok(_) => false,
                // Ignore any NotFound errors, those mean that e.g. some contact in a group deleted his account.
                Err(MessageSenderError::NotFound { service_id }) => {
                    debug!(service_id = %service_id.service_id_string(), "recipient not found, skipping sent message result");
                    false
                }
                // return first error if any
                Err(_) => true,
            })
            .transpose()?;

        let content = Content {
            metadata: Metadata {
                sender: self.state.data.service_ids.aci().into(),
                destination: self.state.data.service_ids.aci().into(),
                sender_device: self.state.device_id(),
                server_guid: None,
                timestamp: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
                // Note: Currently no way to get the timestamp the server received the message; just use our timestamp as a fallback.
                server_timestamp: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
                needs_receipt: false, // TODO: this is just wrong
                unidentified_sender: false,
                was_plaintext: false,
            },
            body: content_body,
        };

        let mut identified_websocket = self.identified_websocket(false).await?;
        save_message(
            &mut self.store,
            &mut identified_websocket,
            content,
            Some(thread),
        )
        .await?;

        Ok(())
    }

    async fn restore_thread_timer(&mut self, thread: &Thread, content_body: &mut ContentBody) {
        let store_expire_timer = self.store.expire_timer(thread).await.unwrap_or_default();

        if let ContentBody::DataMessage(DataMessage {
            expire_timer: ref mut timer,
            expire_timer_version: ref mut version,
            ..
        }) = content_body
        {
            if timer.is_none() {
                *timer = store_expire_timer.and_then(|(t, _)| if t == 0 { None } else { Some(t) });
                *version = Some(store_expire_timer.map(|(_, v)| v).unwrap_or_default());
            } else {
                *version = Some(store_expire_timer.map(|(_, v)| v).unwrap_or_default() + 1);
            }
        }
    }

    /// Clears all sessions established with [recipient](ServiceId).
    pub async fn clear_sessions(&self, recipient: &ServiceId) -> Result<(), Error<S::Error>> {
        use libsignal_service::session_store::SessionStoreExt;
        self.store
            .aci_protocol_store()
            .delete_all_sessions(recipient)
            .await?;
        self.store
            .pni_protocol_store()
            .delete_all_sessions(recipient)
            .await?;
        Ok(())
    }

    /// Downloads and decrypts a single attachment.
    pub async fn get_attachment(
        &self,
        attachment_pointer: &AttachmentPointer,
    ) -> Result<Vec<u8>, Error<S::Error>> {
        self.get_attachment_reporting(attachment_pointer, |_| {})
            .await
    }

    /// The same, reporting how many bytes have arrived as they arrive.
    ///
    /// Upstream reads the whole stream in one `read_to_end`, so there is
    /// nothing to report until there is nothing left to report — which is why a
    /// client can only draw an indeterminate bar for a download. The bytes are
    /// there; only the counting was missing. `progress` is called with the
    /// running total as each chunk lands, and is a plain closure rather than a
    /// channel so a caller that does not care pays nothing.
    pub async fn get_attachment_reporting(
        &self,
        attachment_pointer: &AttachmentPointer,
        mut progress: impl FnMut(u64),
    ) -> Result<Vec<u8>, Error<S::Error>> {
        let expected_digest = attachment_pointer
            .digest
            .as_ref()
            .ok_or_else(|| Error::UnexpectedAttachmentChecksum)?;

        let mut service = self.identified_push_service();
        let mut attachment_stream = service.get_attachment(attachment_pointer).await?;

        let plaintext_len = attachment_pointer.size.and_then(|len| len.try_into().ok());

        // We need the whole file for the crypto to check out
        let mut ciphertext = Vec::with_capacity(plaintext_len.unwrap_or(0));
        let mut chunk = [0u8; 64 * 1024];
        let mut size_bytes = 0usize;
        loop {
            let read = attachment_stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            ciphertext.extend_from_slice(&chunk[..read]);
            size_bytes += read;
            progress(size_bytes as u64);
        }
        trace!(size_bytes, "downloaded encrypted attachment");

        let digest = sha2::Sha256::digest(&ciphertext);
        if &digest[..] != expected_digest {
            return Err(Error::UnexpectedAttachmentChecksum);
        }

        let key: [u8; 64] = attachment_pointer.key().try_into()?;

        // Offload decryption of large attachments to another thread.
        // Chose arbitrary threshold here.
        const DECRYPT_IN_THREAD_THRESHOLD: usize = 100 * 1024;
        if ciphertext.len() > DECRYPT_IN_THREAD_THRESHOLD {
            ciphertext = tokio::task::spawn_blocking(move || {
                decrypt_in_place(key, &mut ciphertext).map(|_| ciphertext)
            })
            .await
            .expect("decryption in another thread")?;
        } else {
            decrypt_in_place(key, &mut ciphertext)?;
        };

        if let Some(len) = plaintext_len {
            if len < ciphertext.len() {
                // remove padding
                ciphertext.truncate(len);
            }
        }

        Ok(ciphertext)
    }

    /// Gets the metadata of a sticker
    pub async fn sticker_metadata(
        &mut self,
        pack_id: &[u8],
        sticker_id: u32,
    ) -> Result<Option<Sticker>, Error<S::Error>> {
        Ok(self.store.sticker_pack(pack_id).await?.and_then(|pack| {
            pack.manifest
                .stickers
                .iter()
                .find(|&x| x.id == sticker_id)
                .cloned()
        }))
    }

    /// Reads a sticker pack without installing it: its title, its author and
    /// every sticker in it, for a client that wants to show one before adding
    /// it. Nothing is stored and no other device is told.
    pub async fn preview_sticker_pack(
        &self,
        pack_id: &[u8],
        pack_key: &[u8],
    ) -> Result<StickerPack, Error<S::Error>> {
        let unidentified_websocket = self.unidentified_websocket().await?;
        fetch_sticker_pack(unidentified_websocket, pack_id, pack_key).await
    }

    /// Installs a sticker pack and notifies other registered devices
    pub async fn install_sticker_pack(
        &mut self,
        pack_id: &[u8],
        pack_key: &[u8],
    ) -> Result<(), Error<S::Error>> {
        let sticker_pack_operation = StickerPackOperation {
            pack_id: Some(pack_id.to_vec()),
            pack_key: Some(pack_key.to_vec()),
            r#type: Some(sticker_pack_operation::Type::Install as i32),
        };

        let unidentified_websocket = self.unidentified_websocket().await?;
        download_sticker_pack(
            self.store.clone(),
            unidentified_websocket,
            &sticker_pack_operation,
        )
        .await?;

        // Sync the change with the other devices
        let sync_message = SyncMessage {
            sticker_pack_operation: vec![sticker_pack_operation],
            ..Default::default()
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        self.send_message(self.state.data.service_ids.aci(), sync_message, timestamp)
            .await?;

        Ok(())
    }

    /// Removes an installed sticker pack
    pub async fn remove_sticker_pack(
        &mut self,
        pack_id: &[u8],
        pack_key: &[u8],
    ) -> Result<(), Error<S::Error>> {
        // Sync the change with the other clients
        let sync_message = SyncMessage {
            sticker_pack_operation: vec![StickerPackOperation {
                pack_id: Some(pack_id.to_vec()),
                pack_key: Some(pack_key.to_vec()), // The pack key might not be neccesary in the message
                r#type: Some(sticker_pack_operation::Type::Remove as i32),
            }],
            ..Default::default()
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        self.send_message(self.state.data.service_ids.aci(), sync_message, timestamp)
            .await?;

        self.store.remove_sticker_pack(pack_id).await?;

        Ok(())
    }

    pub async fn send_session_reset(
        &mut self,
        recipient: &ServiceId,
        timestamp: u64,
    ) -> Result<(), Error<S::Error>> {
        trace!(recipient = %recipient.service_id_string(), "resetting session for address");

        let mut store = self.store.aci_protocol_store();

        // Archive all sessions with all receiver devices.
        // Note that the "get_sub_device_sessions" does not include the main device, therefore add it.
        // Note that deleting the session is not equivalent to archiving:
        // If we deleted the session, we would also delete the information on the list of devices the peer has, failing message sending.
        for device in store
            .get_sub_device_sessions(recipient)
            .await?
            .into_iter()
            .chain(vec![*DEFAULT_DEVICE_ID])
        {
            let address = recipient
                .aci()
                .expect("Recipient to be given as ACI")
                .to_protocol_address(device)
                .expect("Could not construct protocol address from recipient");
            if let Some(mut session) = store.load_session(&address).await? {
                session.archive_current_state()?;
                store.store_session(&address, &session).await?;
            }
        }

        // TODO: Signal Android also deletes entries in some sender_key_shared table; we don't have such a table yet.
        // Does not seem necessary though.

        // Send a null message.
        let message = NullMessage::generate(&mut rand::rng());
        self.send_message(*recipient, message, timestamp).await?;

        Ok(())
    }

    fn credentials(&self) -> ServiceCredentials {
        self.state.credentials()
    }

    /// Creates a new message sender.
    async fn new_message_sender(&self) -> Result<MessageSender<S::AciStore>, Error<S::Error>> {
        let identified_websocket = self.identified_websocket(false).await?;
        let unidentified_websocket = self.unidentified_websocket().await?;

        let aci_protocol_store = self.store.aci_protocol_store();
        let aci_identity_keypair = aci_protocol_store.get_identity_key_pair().await?;
        let pni_identity_keypair = self
            .store
            .pni_protocol_store()
            .get_identity_key_pair()
            .await?;

        Ok(MessageSender::new(
            identified_websocket,
            unidentified_websocket,
            self.identified_push_service(),
            self.new_service_cipher_aci(),
            aci_protocol_store,
            self.state.data.service_ids.aci,
            self.state.data.service_ids.pni,
            aci_identity_keypair,
            Some(pni_identity_keypair),
            self.state.device_id(),
        ))
    }

    fn new_service_cipher_aci(&self) -> ServiceCipher<S::AciStore> {
        ServiceCipher::new(
            self.store.aci_protocol_store(),
            self.state
                .service_configuration()
                .unidentified_sender_trust_roots,
            ProtocolAddress::new(
                self.state.data.service_ids.aci.to_string(),
                self.state.device_id(),
            ),
        )
    }

    fn new_service_cipher_pni(&self) -> ServiceCipher<S::PniStore> {
        ServiceCipher::new(
            self.store.pni_protocol_store(),
            self.state
                .service_configuration()
                .unidentified_sender_trust_roots,
            ProtocolAddress::new(
                self.state.data.service_ids.pni.to_string(),
                self.state.device_id(),
            ),
        )
    }

    /// Returns the title of a thread (contact or group).
    pub async fn thread_title(&self, thread: &Thread) -> Result<String, Error<S::Error>> {
        match thread {
            Thread::Contact(service_id) => {
                let contact = match self.store.contact_by_id(service_id).await {
                    Ok(contact) => contact,
                    Err(error) => {
                        info!(%error, service_id =% service_id.service_id_string(), "error getting contact by id");
                        None
                    }
                };
                Ok(match contact {
                    Some(contact) => contact.name,
                    None => service_id.service_id_string(),
                })
            }
            Thread::Group(id) => match self.store.group(*id).await? {
                Some(group) => Ok(group.title),
                None => Ok("".to_string()),
            },
        }
    }

    /// Returns how this client was registered, either as a primary or secondary device.
    pub fn registration_type(&self) -> RegistrationType {
        if self.state.data.device_name.is_some() {
            RegistrationType::Secondary
        } else {
            RegistrationType::Primary
        }
    }

    /// As a primary device, link a secondary device.
    pub async fn link_secondary(&mut self, secondary: Url) -> Result<(), Error<S::Error>> {
        // XXX: What happens if secondary device? Possible to use static typing to make this method call impossible in that case?
        if self.registration_type() != RegistrationType::Primary {
            return Err(Error::NotPrimaryDevice);
        }

        let credentials = self.credentials();
        let mut account_manager = AccountManager::new(
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            Some(self.state.data.profile_key),
        );

        account_manager
            .link_device(
                &mut rand::rng(),
                secondary,
                &self.store.aci_protocol_store(),
                &self.store.pni_protocol_store(),
                ProvisioningSecrets {
                    credentials,
                    account_entropy_pool: self
                        .account_entropy_pool()
                        .await?
                        .expect("Primary device to always have an account entropy pool"),
                    master_key: self.master_key().await?,
                    ephemeral_backup_key: None,
                    media_root_backup_key: None,
                },
            )
            .await?;
        Ok(())
    }

    /// As a primary device, unlink a secondary device.
    pub async fn unlink_secondary(
        &self,
        device_id: impl TryInto<DeviceId>,
    ) -> Result<(), Error<S::Error>> {
        // secondary devices cannot unlink themselves or other devices, it will fail with an unauthorized error
        if self.registration_type() != RegistrationType::Primary {
            return Err(Error::NotPrimaryDevice);
        }
        self.identified_websocket(false)
            .await?
            .unlink_device(device_id.try_into().map_err(|_| Error::InvalidDeviceId)?)
            .await?;
        Ok(())
    }

    /// Claims `nickname` as this account's username and publishes a link for it.
    ///
    /// Reserves a discriminated form of the nickname, confirms the reservation,
    /// then sets the username link. Returns the username the server settled on
    /// and its shareable `https://signal.me/#eu/...` link.
    ///
    /// Nothing is persisted locally: the caller owns the returned pair.
    pub async fn set_username(
        &self,
        nickname: &str,
    ) -> Result<(libsignal_service::protocol::Username, url::Url), Error<S::Error>> {
        let mut websocket = self.identified_websocket(false).await?;
        let reserved = websocket.reserve_username(nickname).await?;
        websocket.confirm_username(&reserved.username).await?;
        let link = websocket
            .set_username_link(&reserved.username, false)
            .await?;
        Ok((reserved.username, link))
    }

    /// Clears this account's username and username link.
    pub async fn delete_username(&self) -> Result<(), Error<S::Error>> {
        self.identified_websocket(false)
            .await?
            .delete_username()
            .await?;
        Ok(())
    }

    /// Unlink *this* device from the account.
    ///
    /// Unlike [`Manager::unlink_secondary`], which is a primary device removing
    /// some other device, this targets the device id this manager registered
    /// with. A linked (secondary) device is allowed to remove itself.
    ///
    /// The local store is left untouched: clearing it is the caller's job.
    pub async fn unlink_self(&self) -> Result<(), Error<S::Error>> {
        let device_id = self.state.device_id();
        self.identified_websocket(false)
            .await?
            .unlink_device(device_id)
            .await?;
        Ok(())
    }

    /// As a primary device, list all the devices (including the current device).
    pub async fn devices(&self) -> Result<Vec<DeviceInfo>, Error<S::Error>> {
        let aci_protocol_store = self.store.aci_protocol_store();
        let mut account_manager = AccountManager::new(
            self.identified_push_service(),
            self.identified_websocket(false).await?,
            Some(self.state.data.profile_key),
        );

        Ok(account_manager.linked_devices(&aci_protocol_store).await?)
    }

    async fn storage_service(
        &self,
    ) -> Result<(StorageService, StorageServiceKey), Error<S::Error>> {
        let master_key = self.master_key().await?.ok_or(Error::NoMasterKey)?;
        let key = StorageServiceKey::from_master_key(&master_key);
        let service = StorageService::new(self.identified_push_service(), key.clone()).await?;
        Ok((service, key))
    }

    /// Every [`ContactRecord`] in the account's storage service manifest.
    ///
    /// These carry the per-contact overrides the official clients sync between
    /// linked devices — among them `nickname` and `note`, which exist nowhere
    /// else in the protocol.
    pub async fn contact_records(&mut self) -> Result<Vec<ContactRecord>, Error<S::Error>> {
        let (storage, _) = self.storage_service().await?;
        let manifest = storage.manifest().await?;
        let (keys, record_ikm) = contact_keys(&manifest);
        Ok(storage
            .read_items(keys, record_ikm)
            .await?
            .into_iter()
            .filter_map(|record| match record.record {
                Some(storage_record::Record::Contact(contact)) => Some(contact),
                _ => None,
            })
            .collect())
    }

    /// Creates a group called `title` with `members` in it, and returns the
    /// master key that is its identity.
    ///
    /// The master key is generated here and never leaves this account except in
    /// the `GroupContextV2` attached to messages sent to the group: it is what
    /// the members need to fetch and decrypt the group, and the server is given
    /// only the public parameters derived from it. Telling them is the caller's
    /// job -- sending anything at all to the returned thread does it.
    ///
    /// Anybody whose profile key this account holds joins as a member. Anybody
    /// else is invited instead, because vouching for a member means presenting a
    /// credential over their profile key, and there is none to present.
    pub async fn create_group(
        &mut self,
        title: &str,
        members: &[Uuid],
    ) -> Result<[u8; 32], Error<S::Error>> {
        let master_key: [u8; 32] = rand::random();
        let mut groups_manager = self.groups_manager().await?;

        let own = self.state.data.service_ids.aci();
        let self_credential = groups_manager
            .member_credential(own, self.state.data.profile_key())
            .await?;

        let mut candidates = Vec::with_capacity(members.len());
        for member in members {
            let aci = Aci::from(*member);
            let credential = match self.profile_key_for(*member).await {
                Some(profile_key) => groups_manager
                    .member_credential(aci, profile_key)
                    .await
                    .inspect_err(
                        |error| warn!(%error, %member, "no credential; inviting instead"),
                    )
                    .ok(),
                None => None,
            };
            candidates.push(GroupMemberCandidate {
                service_id: aci.into(),
                credential,
            });
        }

        groups_manager
            .create_group(
                &mut rng(),
                GroupMasterKey::new(master_key),
                title,
                &self_credential,
                &candidates,
            )
            .await?;

        // Read back rather than assumed: the server fills in the version, the
        // join timestamps and which invitations it actually accepted, and the
        // store is what every later message about this group is matched against.
        upsert_group(&self.store, &mut groups_manager, &master_key.to_vec(), &0).await?;

        Ok(master_key)
    }

    /// The profile key held for somebody, which is what a group membership has
    /// to be vouched for with.
    async fn profile_key_for(&self, uuid: Uuid) -> Option<ProfileKey> {
        let contact = self
            .store
            .contact_by_id(&ServiceId::Aci(uuid.into()))
            .await
            .ok()??;
        let bytes: [u8; 32] = contact.profile_key.try_into().ok()?;
        Some(ProfileKey::create(bytes))
    }

    /// This account's [`AccountRecord`] from the storage service manifest.
    ///
    /// The record the official clients keep their own account-wide settings in.
    /// There is exactly one; `None` means the manifest has not got it yet.
    pub async fn account_record(&mut self) -> Result<Option<AccountRecord>, Error<S::Error>> {
        let (storage, _) = self.storage_service().await?;
        let manifest = storage.manifest().await?;
        let (keys, record_ikm) = records_of(&manifest, manifest_record::identifier::Type::Account);
        Ok(storage
            .read_items(keys, record_ikm)
            .await?
            .into_iter()
            .find_map(|record| match record.record {
                Some(storage_record::Record::Account(account)) => Some(account),
                _ => None,
            }))
    }

    /// The username this account already has, and the `signal.me` link that
    /// shares it.
    ///
    /// The server only ever stores a *hash* of a username, so it cannot answer
    /// this: the plaintext lives in the account's storage service record and
    /// nowhere else. A client that does not read it there has no way of knowing
    /// its own username, which is why one never set from this device used to
    /// read as no username at all.
    ///
    /// The link is rebuilt from the entropy and handle the record carries, so it
    /// is the same link every other device shares.
    pub async fn username(&mut self) -> Result<Option<(Username, Option<Url>)>, Error<S::Error>> {
        let Some(record) = self.account_record().await? else {
            return Ok(None);
        };
        if record.username.is_empty() {
            return Ok(None);
        }
        let username = Username::new(&record.username)?;
        let link = record.username_link.as_ref().and_then(|link| {
            let entropy: [u8; 32] = link.entropy.as_slice().try_into().ok()?;
            let handle = Uuid::from_slice(&link.server_id).ok()?;
            Some(generate_username_link(handle, &entropy))
        });
        Ok(Some((username, link)))
    }

    /// Sets the nickname and note a contact is shown under, on this account and
    /// every device linked to it.
    ///
    /// UNVERIFIED: the write half of the storage service has never run against a
    /// live account from here. It rewrites the account manifest; a wrong one
    /// scrambles the synced contact list on every linked device.
    pub async fn set_contact_nickname(
        &mut self,
        contact: ServiceId,
        given: Option<String>,
        family: Option<String>,
        note: Option<String>,
    ) -> Result<(), Error<S::Error>> {
        self.update_contact(contact, move |record| {
            record.nickname = match (&given, &family) {
                (None, None) => None,
                (given, family) => Some(contact_record::Name {
                    given: given.clone().unwrap_or_default(),
                    family: family.clone().unwrap_or_default(),
                }),
            };
            record.note = note.clone().unwrap_or_default();
        })
        .await
    }

    /// Blocks or unblocks a contact, on this account and every device linked to
    /// it.
    ///
    /// Signal's block list is not a server-side thing you can ask about: it is
    /// the `blocked` flag on the contact's own Storage Service record, which
    /// every device reads out of the shared manifest. Writing it here is
    /// therefore the whole of blocking somebody -- the phone applies it the next
    /// time it reads the manifest, which the version bump is what tells it to do.
    ///
    /// UNVERIFIED, for the same reason `set_contact_nickname` is.
    pub async fn set_contact_blocked(
        &mut self,
        contact: ServiceId,
        blocked: bool,
    ) -> Result<(), Error<S::Error>> {
        self.update_contact(contact, move |record| {
            record.blocked = blocked;
            // Blocking somebody un-approves them: `whitelisted` is what says
            // this account has accepted a message request from them, and leaving
            // it set means unblocking silently re-approves.
            if blocked {
                record.whitelisted = false;
            }
        })
        .await
    }

    /// Reads one contact's record, applies `edit` to it, and writes it back
    /// under a fresh key with the manifest bumped.
    ///
    /// Retried once on a conflict, which is what a manifest version losing a
    /// race with another device is. The second attempt re-reads, so it applies
    /// `edit` to whatever the winner wrote rather than clobbering it.
    async fn update_contact(
        &mut self,
        contact: ServiceId,
        edit: impl Fn(&mut ContactRecord),
    ) -> Result<(), Error<S::Error>> {
        let (storage, key) = self.storage_service().await?;
        let source_device = u32::from(self.device_id());
        match self
            .write_contact(&storage, &key, source_device, &contact, &edit)
            .await
        {
            Err(Error::StorageService(StorageServiceError::Conflict)) => {
                self.write_contact(&storage, &key, source_device, &contact, &edit)
                    .await
            }
            result => result,
        }
    }

    async fn write_contact(
        &self,
        storage: &StorageService,
        key: &StorageServiceKey,
        source_device: u32,
        contact: &ServiceId,
        edit: &impl Fn(&mut ContactRecord),
    ) -> Result<(), Error<S::Error>> {
        let manifest = storage.manifest().await?;
        let (keys, record_ikm) = contact_keys(&manifest);
        let record_ikm = record_ikm.map(|ikm| ikm.to_vec());

        let (old_key, mut record) = storage
            .read_items_keyed(keys, record_ikm.as_deref())
            .await?
            .into_iter()
            .find(|(_, record)| match &record.record {
                Some(storage_record::Record::Contact(c)) => is_contact(c, contact),
                _ => false,
            })
            .ok_or(Error::UnknownStorageRecord)?;

        let Some(storage_record::Record::Contact(ref mut c)) = record.record else {
            return Err(Error::UnknownStorageRecord);
        };
        edit(c);

        let new_key: [u8; 16] = rand::random();
        let item =
            StorageService::encrypt_item(key, new_key.to_vec(), &record, record_ikm.as_deref());

        let identifiers = manifest
            .identifiers
            .iter()
            .map(|identifier| {
                if identifier.raw == old_key {
                    manifest_record::Identifier {
                        raw: new_key.to_vec(),
                        r#type: identifier.r#type,
                    }
                } else {
                    identifier.clone()
                }
            })
            .collect();

        let next = ManifestRecord {
            version: manifest.version + 1,
            source_device,
            identifiers,
            record_ikm: manifest.record_ikm.clone(),
        };

        storage
            .write(WriteOperation {
                manifest: Some(StorageService::encrypt_manifest(key, &next)),
                insert_item: vec![item],
                delete_key: vec![old_key],
                clear_all: false,
            })
            .await?;

        Ok(())
    }
}

fn contact_keys(manifest: &ManifestRecord) -> (Vec<Vec<u8>>, Option<&[u8]>) {
    records_of(manifest, manifest_record::identifier::Type::Contact)
}

/// The manifest keys of one kind of record, and the key material they were
/// encrypted under.
fn records_of(
    manifest: &ManifestRecord,
    kind: manifest_record::identifier::Type,
) -> (Vec<Vec<u8>>, Option<&[u8]>) {
    let keys = manifest
        .identifiers
        .iter()
        .filter(|identifier| identifier.r#type == kind as i32)
        .map(|identifier| identifier.raw.clone())
        .collect();
    let record_ikm = (!manifest.record_ikm.is_empty()).then_some(manifest.record_ikm.as_slice());
    (keys, record_ikm)
}

fn is_contact(record: &ContactRecord, service_id: &ServiceId) -> bool {
    let uuid = service_id.raw_uuid();
    let matches = |text: &str, binary: &[u8]| {
        text.parse::<Uuid>().is_ok_and(|parsed| parsed == uuid)
            || Uuid::from_slice(binary).is_ok_and(|parsed| parsed == uuid)
    };
    match service_id {
        ServiceId::Aci(_) => matches(&record.aci, &record.aci_binary),
        ServiceId::Pni(_) => matches(&record.pni, &record.pni_binary),
    }
}

/// Set the timestamp in any DataMessage so it matches its envelope's
fn ensure_data_message_timestamp(content_body: &mut ContentBody, timestamp: u64) {
    match content_body {
        ContentBody::DataMessage(message) => {
            message.timestamp = Some(timestamp);
        }
        ContentBody::EditMessage(EditMessage {
            data_message: Some(data_message),
            ..
        }) => {
            data_message.timestamp = Some(timestamp);
        }
        ContentBody::SynchronizeMessage(SyncMessage {
            sent:
                Some(sync_message::Sent {
                    message: Some(data_message),
                    ..
                }),
            ..
        }) => {
            data_message.timestamp = Some(timestamp);
        }
        _ => (),
    }
}

async fn upsert_group<S: Store>(
    store: &S,
    groups_manager: &mut GroupsManager<InMemoryCredentialsCache>,
    master_key_bytes: &[u8],
    revision: &u32,
) -> Result<Option<Group>, Error<S::Error>> {
    let upsert_group = match store.group(master_key_bytes.try_into()?).await {
        Ok(Some(group)) => {
            debug!(group_name =% group.title, "loaded group from local db");
            group.revision < *revision
        }
        Ok(None) => true,
        Err(error) => {
            warn!(%error, "failed to retrieve group from local db");
            true
        }
    };

    if upsert_group {
        debug!("fetching and saving group");
        match groups_manager
            .fetch_encrypted_group(&mut rand::rng(), master_key_bytes)
            .await
        {
            Ok(encrypted_group) => {
                let group = decrypt_group(master_key_bytes, encrypted_group)?;
                if let Err(error) = store.save_group(master_key_bytes.try_into()?, group).await {
                    error!(%error, "failed to save group");
                }
            }
            Err(error) => {
                warn!(%error, "failed to fetch encrypted group")
            }
        }
    }

    Ok(store.group(master_key_bytes.try_into()?).await?)
}

/// Download and decrypt a sticker manifest
/// How many of a pack's stickers are fetched at once.
const STICKERS_AT_ONCE: usize = 16;

/// How many times one sticker is asked for before it is given up on.
const STICKER_ATTEMPTS: usize = 3;

async fn download_sticker_pack<C: ContentsStore>(
    mut store: C,
    unidentified_websocket: SignalWebSocket<websocket::Unidentified>,
    operation: &StickerPackOperation,
) -> Result<StickerPack, Error<C::ContentsStoreError>> {
    let sticker_pack = fetch_sticker_pack(
        unidentified_websocket,
        operation.pack_id(),
        operation.pack_key(),
    )
    .await?;

    // save everything in store
    store.add_sticker_pack(&sticker_pack).await?;

    Ok(sticker_pack)
}

/// Download and decrypt a sticker pack without installing it.
///
/// The manifest lives behind the pack key and there is no way to read one but to
/// fetch it, so this is what a client needs to show a pack — its title, its
/// author, every sticker in it — before deciding whether to add it. Nothing is
/// written to the store and no other device is told.
async fn fetch_sticker_pack<E: std::error::Error>(
    mut unidentified_websocket: SignalWebSocket<websocket::Unidentified>,
    pack_id: &[u8],
    pack_key: &[u8],
) -> Result<StickerPack, Error<E>> {
    debug!("downloading sticker pack");
    let key = derive_key(pack_key)?;

    let mut ciphertext = Vec::new();

    let size_bytes = unidentified_websocket
        .get_sticker_pack_manifest(&hex::encode(pack_id))
        .await?
        .read_to_end(&mut ciphertext)
        .await?;

    trace!(size_bytes, "downloaded encrypted sticker pack manifest");

    decrypt_in_place(key, &mut ciphertext)?;

    let mut sticker_pack_manifest: StickerPackManifest =
        libsignal_service::proto::Pack::decode(ciphertext.as_slice())
            .map_err(ProvisioningError::from)?
            .into();

    // One sticker is one CDN round trip, and a pack is routinely a hundred of
    // them: fetched one after another that is half a minute of waiting for a
    // sheet the phone opens at once. The socket is a facade over an id-matched
    // request channel and the CDN client pools its connections, so a clone per
    // download is a request in flight rather than a second connection. Ordered,
    // so the results zip back onto the manifest they came from.
    let ids: Vec<u32> = sticker_pack_manifest
        .stickers
        .iter()
        .map(|sticker| sticker.id)
        .collect();
    let downloaded: Vec<Option<Vec<u8>>> = stream::iter(ids)
        .map(|id| {
            let mut socket = unidentified_websocket.clone();
            async move { fetch_sticker::<E>(&mut socket, pack_id, pack_key, id).await }
        })
        .buffered(STICKERS_AT_ONCE)
        .collect()
        .await;

    for (sticker, bytes) in sticker_pack_manifest.stickers.iter_mut().zip(downloaded) {
        sticker.bytes = bytes;
    }

    Ok(StickerPack {
        id: pack_id.to_vec(),
        key: pack_key.to_vec(),
        manifest: sticker_pack_manifest,
    })
}

/// One sticker, asked for again when the answer was not usable.
///
/// A hundred requests go out at once and any of them can come back a truncated
/// body, a CDN error page or a connection that was reset -- all of which fail
/// the MAC and read as "failed to decrypt". Given up on at the first attempt,
/// that is a permanent hole: the sticker is stored with no bytes, nothing ever
/// asks again, and the tile draws nothing for as long as the pack is installed.
/// Three attempts, spaced, because the failure is nearly always the burst.
async fn fetch_sticker<E: std::error::Error>(
    socket: &mut SignalWebSocket<websocket::Unidentified>,
    pack_id: &[u8],
    pack_key: &[u8],
    id: u32,
) -> Option<Vec<u8>> {
    for attempt in 1..=STICKER_ATTEMPTS {
        match download_sticker::<E>(socket, pack_id, pack_key, id).await {
            Ok(bytes) => return Some(bytes),
            Err(error) if attempt == STICKER_ATTEMPTS => {
                error!(id, %error, "failed to download sticker");
            }
            Err(error) => {
                debug!(id, attempt, %error, "retrying a sticker download");
                tokio::time::sleep(std::time::Duration::from_millis(200 * attempt as u64)).await;
            }
        }
    }
    None
}

/// Downloads and decrypts a single sticker
async fn download_sticker<E: std::error::Error>(
    unidentified_websocket: &mut SignalWebSocket<websocket::Unidentified>,
    pack_id: &[u8],
    pack_key: &[u8],
    sticker_id: u32,
) -> Result<Vec<u8>, Error<E>> {
    let key = derive_key(pack_key)?;

    let mut sticker_stream = unidentified_websocket
        .get_sticker(&hex::encode(pack_id), sticker_id)
        .await?;

    let mut ciphertext = Vec::new();
    let size_bytes = sticker_stream.read_to_end(&mut ciphertext).await?;

    trace!(size_bytes, "downloaded encrypted sticker");

    decrypt_in_place(key, &mut ciphertext)?;

    Ok(ciphertext)
}

/// Save a message into the store.
/// Note that `override_thread` can be used to specify the thread the message will be stored in.
/// This is required when storing outgoing messages, as in this case the appropriate storage place cannot be derived from the message itself.
async fn save_message<S: Store>(
    store: &mut S,
    identified_websocket: &mut websocket::SignalWebSocket<websocket::Identified>,
    message: Content,
    override_thread: Option<Thread>,
) -> Result<(), Error<S::Error>> {
    // derive the thread from the message type
    let thread = override_thread.unwrap_or(Thread::try_from(&message)?);

    // only save DataMessage and SynchronizeMessage (sent)
    let message = match message.body {
        ContentBody::DecryptionErrorMessage(e) => {
            warn!(error = ?e, "was asked to save a DecryptionErrorMessage; this should not happen");
            None
        }
        ContentBody::NullMessage(_) => Some(message),
        ContentBody::DataMessage(
            ref data_message @ DataMessage {
                ref profile_key, ..
            },
        )
        | ContentBody::SynchronizeMessage(SyncMessage {
            sent:
                Some(sync_message::Sent {
                    message:
                        Some(
                            ref data_message @ DataMessage {
                                ref profile_key, ..
                            },
                        ),
                    ..
                }),
            ..
        }) => {
            // update recipient profile key if changed
            if let Some(profile_key_bytes) = profile_key.clone().and_then(|p| p.try_into().ok()) {
                let sender = message.metadata.sender;
                let profile_key = ProfileKey::create(profile_key_bytes);
                debug!(sender = %sender.service_id_string(), "inserting profile key for");

                // Either:
                // - insert a new contact with the profile information
                // - update the contact if the profile key has changed
                // TODO: mark this contact as "created by us" maybe to know whether we should update it or not
                // NOTE: this needs to happen in the background!
                let store_inner = store.clone();
                let websocket_inner = identified_websocket.clone();
                let data_message_inner = data_message.clone();
                tokio::spawn(async move {
                    if let Err(error) = upsert_contact_from_profile(
                        store_inner,
                        websocket_inner,
                        &data_message_inner,
                        sender,
                        profile_key,
                    )
                    .await
                    {
                        error!(%error, "failed to upsert newly seen contact!");
                    }
                });
            }

            // Note: The expire timer fields of data messages are only for contacts.
            // Expire timers are handled for groups via upsert_group due to a revision change.
            if let Thread::Contact(_) = thread {
                let version = data_message.expire_timer_version.unwrap_or(1);
                store
                    .update_expire_timer(
                        &thread,
                        data_message.expire_timer.unwrap_or_default(),
                        version,
                    )
                    .await?;
            }

            match data_message {
                DataMessage {
                    delete:
                        Some(Delete {
                            target_sent_timestamp: Some(ts),
                        }),
                    ..
                } => {
                    // replace an existing message by an empty NullMessage
                    if let Some(mut existing_msg) = store.message(&thread, *ts).await? {
                        existing_msg.metadata.sender = Aci::from(Uuid::nil()).into();
                        existing_msg.body = NullMessage::default().into();
                        store.save_message(&thread, existing_msg).await?;
                        debug!(%thread, ts, "message in thread deleted");
                        None
                    } else {
                        warn!(%thread, ts, "could not find message to delete in thread");
                        None
                    }
                }
                _ => Some(message),
            }
        }
        ContentBody::SynchronizeMessage(SyncMessage {
            delete_for_me: Some(ref delete),
            ..
        }) => {
            // TODO: Conversations, local-only deletes, attachments
            for d in delete.message_deletes.iter().flat_map(|m| &m.messages) {
                let sender = match &d.author {
                    Some(Author::AuthorServiceId(id)) => {
                        ServiceId::parse_from_service_id_string(id)
                    }
                    Some(Author::AuthorServiceIdBinary(id)) => {
                        ServiceId::parse_from_service_id_binary(id)
                    }
                    Some(Author::AuthorE164(_)) => None,
                    None => None,
                };
                let Some(sender) = sender else {
                    tracing::warn!("Could not parse author of delete-for-self message; ignoring.");
                    continue;
                };
                let Some(timestamp) = d.sent_timestamp else {
                    tracing::warn!("Timestamp of delete-for-self message not given; ignoring.");
                    continue;
                };
                let Ok(Some(thread)) = store
                    .thread_for_sender_and_timestamp(&sender, timestamp)
                    .await
                else {
                    tracing::warn!(
                        "Message referenced by delete-for-self message not found; ignoring."
                    );
                    continue;
                };
                // Note: Not marking the message as deleted, like when receiving deletion requests by others.
                // This matches the behavior of Signal Desktop, where the message completely disappears from the timeline.
                let result = store.delete_message(&thread, timestamp).await;
                if !result.is_ok_and(|d| d) {
                    tracing::warn!(
                        "Could not delete message referenced by delete-for-self message; ignoring."
                    );
                }
            }
            None
        }
        ContentBody::EditMessage(EditMessage {
            target_sent_timestamp: Some(_),
            data_message: Some(_),
        })
        | ContentBody::SynchronizeMessage(SyncMessage {
            sent:
                Some(sync_message::Sent {
                    edit_message:
                        Some(EditMessage {
                            target_sent_timestamp: Some(_),
                            data_message: Some(_),
                        }),
                    ..
                }),
            ..
        }) => Some(message),
        ContentBody::CallMessage(_)
        | ContentBody::SynchronizeMessage(SyncMessage {
            call_event: Some(_),
            ..
        }) => Some(message),
        ContentBody::SynchronizeMessage(msg) => {
            debug!(
                ?msg,
                "skipping saving sync message without interesting fields"
            );
            None
        }
        ContentBody::ReceiptMessage(_) => Some(message),
        ContentBody::TypingMessage(msg) => {
            debug!(?msg, "skipping saving typing message");
            None
        }
        ContentBody::StoryMessage(msg) => {
            debug!(?msg, "skipping story message");
            None
        }
        ContentBody::PniSignatureMessage(msg) => {
            debug!(?msg, "skipping PNI signature message");
            None
        }
        ContentBody::EditMessage(msg) => {
            debug!(?msg, "invalid edited");
            None
        }
    };

    if let Some(message) = message {
        store.save_message(&thread, message).await?;
    }

    Ok(())
}

async fn upsert_contact_from_profile<S: Store>(
    mut store: S,
    mut identified_websocket: SignalWebSocket<websocket::Identified>,
    data_message: &DataMessage,
    sender: ServiceId,
    profile_key: ProfileKey,
) -> Result<(), Error<<S as Store>::Error>> {
    if store.contact_by_id(&sender).await?.is_none()
        || store
            .profile_key(&sender)
            .await?
            .is_none_or(|p| p.bytes != profile_key.bytes)
    {
        if let Some(aci) = sender.aci() {
            let sender_uuid: Uuid = aci.into();
            let encrypted_profile = identified_websocket
                .retrieve_profile_by_id(aci, Some(profile_key))
                .await?;
            let profile_cipher = ProfileCipher::new(profile_key);
            let decrypted_profile = profile_cipher.decrypt(encrypted_profile).unwrap();

            let contact = Contact {
                uuid: sender_uuid,
                phone_number: None,
                name: decrypted_profile
                    .name
                    // FIXME: this assumes [firstname] [lastname]
                    .map(|pn| {
                        if let Some(family_name) = pn.family_name {
                            format!("{} {}", pn.given_name, family_name)
                        } else {
                            pn.given_name
                        }
                    })
                    .unwrap_or_default(),
                profile_key: profile_key.bytes.to_vec(),
                expire_timer: data_message.expire_timer.unwrap_or_default(),
                expire_timer_version: data_message.expire_timer_version.unwrap_or(1),
                inbox_position: 0,
                avatar: None,
                verified: Verified::default(),
            };

            info!(%sender_uuid, "saved contact on first sight");
            store.save_contact(&contact).await?;
            store.upsert_profile_key(&sender_uuid, profile_key).await?;
        } else {
            debug!("not storing profile for PNI contact");
        }
    }
    Ok(())
}

async fn set_account_attributes<S: Store>(
    account_manager: &mut AccountManager,
    store: &S,
    data: &RegistrationData,
) -> Result<(), Error<S::Error>> {
    trace!("setting account attributes");

    let pni_registration_id = data.pni_registration_id.ok_or(Error::RelinkNecessary)?;

    let name = if let Some(device_name) = data.device_name() {
        let aci_key_pair = store.aci_protocol_store().get_identity_key_pair().await?;
        let mut rng = rng();
        Some(encrypt_device_name(
            &mut rng,
            device_name,
            aci_key_pair.identity_key(),
        )?)
    } else {
        None
    };

    account_manager
        .set_account_attributes(AccountAttributes {
            fetches_messages: true,
            registration_id: data.registration_id,
            pni_registration_id,
            name,
            registration_lock: None,
            unidentified_access_key: Some(data.profile_key.derive_access_key().to_vec()),
            unrestricted_unidentified_access: false,
            capabilities: DeviceCapabilities {
                storage: true,
                transfer: false,
                attachment_backfill: false,
                spqr: true,
                profiles_v2: false,
                username_change_sync_message: true,
            },
            discoverable_by_phone_number: true,
            pin: None,
            recovery_password: None,
        })
        .await?;

    trace!("done setting account attributes");
    Ok(())
}

async fn register_pre_keys<S: Store>(
    store: &S,
    account_manager: &mut AccountManager,
) -> Result<(), Error<S::Error>> {
    trace!("registering pre keys");

    account_manager
        .update_pre_key_bundle(&mut store.aci_protocol_store(), ServiceIdKind::Aci, true)
        .await?;

    account_manager
        .update_pre_key_bundle(&mut store.pni_protocol_store(), ServiceIdKind::Pni, true)
        .await?;

    trace!("registered pre keys");
    Ok(())
}
