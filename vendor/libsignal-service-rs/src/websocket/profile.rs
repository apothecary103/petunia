use libsignal_protocol::Aci;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use zkgroup::profiles::{ProfileKeyCommitment, ProfileKeyVersion};

use crate::{
    content::ServiceError,
    push_service::{AttachmentV2UploadAttributes, AvatarWrite},
    utils::{serde_base64, serde_optional_base64},
    websocket::{self, account::DeviceCapabilities, SignalWebSocket},
};

/// UNVERIFIED: the CDN0 bucket takes the profile avatar at its root, with the
/// object name coming from the signed policy rather than from the request path
/// — Signal-Android's `PushServiceSocket.AVATAR_UPLOAD_PATH`, which is the
/// empty string. Not exercised against a live server from this tree.
const AVATAR_UPLOAD_PATH: &str = "";

/// UNVERIFIED: the multipart file part's filename. Signal-Android names it
/// `file`; S3 ignores it, since the object name is the policy's `key`.
const AVATAR_UPLOAD_FILENAME: &str = "file";

/// A donation badge returned by the server on profile fetch.
///
/// Mirrors the JSON shape of Signal-Android's `SignalServiceProfile.Badge`.
/// Display metadata is render-ready (name, description, sprites6 image URLs).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    /// Server catalog id (e.g. "BOOSTING").
    #[serde(default)]
    pub id: String,
    /// Badge category string.
    #[serde(default)]
    pub category: String,
    /// Render-ready display name.
    #[serde(default)]
    pub name: String,
    /// Render-ready description.
    #[serde(default)]
    pub description: String,
    /// Sprite image URLs (density-tagged).
    #[serde(default)]
    pub sprites6: Vec<String>,
    /// Expiration epoch millis. Java sends this as BigDecimal.
    #[serde(default)]
    pub expiration: Option<f64>,
    /// Whether the badge is displayed on the profile.
    #[serde(default)]
    pub visible: bool,
    /// Duration badge is valid for, in seconds.
    #[serde(default)]
    pub duration: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalServiceProfile {
    #[serde(default, with = "serde_optional_base64")]
    pub identity_key: Option<Vec<u8>>,
    #[serde(default, with = "serde_optional_base64")]
    pub name: Option<Vec<u8>>,
    #[serde(default, with = "serde_optional_base64")]
    pub about: Option<Vec<u8>>,
    #[serde(default, with = "serde_optional_base64")]
    pub about_emoji: Option<Vec<u8>>,

    // TODO: not sure whether this is via optional_base64
    // #[serde(default, with = "serde_optional_base64")]
    // pub payment_address: Option<Vec<u8>>,
    pub avatar: Option<String>,
    pub unidentified_access: Option<String>,

    #[serde(default)]
    pub unrestricted_unidentified_access: bool,

    pub capabilities: DeviceCapabilities,

    /// Donation badges the server reports for this profile.
    #[serde(default)]
    pub badges: Vec<Badge>,

    /// The blinded profile key credential, present only when the profile was
    /// fetched with a credential request. Adding somebody to a group needs one.
    #[serde(default, with = "serde_optional_base64")]
    pub credential: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignalServiceProfileWrite<'s> {
    /// Hex-encoded
    version: &'s str,
    #[serde(with = "serde_base64")]
    name: &'s [u8],
    #[serde(with = "serde_base64")]
    about: &'s [u8],
    #[serde(with = "serde_base64")]
    about_emoji: &'s [u8],
    avatar: bool,
    same_avatar: bool,
    #[serde(with = "serde_base64")]
    commitment: &'s [u8],
}

impl SignalWebSocket<websocket::Identified> {
    pub async fn retrieve_profile_by_id(
        &mut self,
        address: Aci,
        profile_key: Option<zkgroup::profiles::ProfileKey>,
    ) -> Result<SignalServiceProfile, ServiceError> {
        let path = if let Some(key) = profile_key {
            let version =
                bincode::serialize(&key.get_profile_key_version(address))?;
            let version = std::str::from_utf8(&version)
                .expect("hex encoded profile key version");
            format!("/v1/profile/{}/{}", address.service_id_string(), version)
        } else {
            format!("/v1/profile/{}", address.service_id_string())
        };
        // TODO: set locale to en_US
        self.http_request(Method::GET, path)?
            .send()
            .await?
            .service_error_for_status()
            .await?
            .json()
            .await
    }

    /// Fetches somebody's expiring profile key credential, which is what proves
    /// to the group server that this account knows their profile key.
    ///
    /// Adding a member to a group means presenting one of these; without it the
    /// best that can be done is an invitation the other side has to accept.
    ///
    /// UNVERIFIED: modelled on Signal-Android's
    /// `PushServiceSocket.getVersionedProfileAndCredential`. Not exercised
    /// against a live server from this tree.
    pub async fn expiring_profile_key_credential(
        &mut self,
        aci: Aci,
        profile_key: zkgroup::profiles::ProfileKey,
        server_public_params: &zkgroup::ServerPublicParams,
    ) -> Result<zkgroup::profiles::ExpiringProfileKeyCredential, ServiceError> {
        let mut randomness = [0u8; 32];
        rand::Rng::fill(&mut rand::rng(), &mut randomness);

        let context = server_public_params
            .create_profile_key_credential_request_context(
                randomness,
                aci,
                profile_key,
            );

        // Both of these cross the wire hex-encoded, which is what bincode's
        // transparent encoding of these types already produces.
        let version =
            bincode::serialize(&profile_key.get_profile_key_version(aci))?;
        let version = std::str::from_utf8(&version)
            .expect("hex encoded profile key version");
        let request = hex::encode(zkgroup::serialize(&context.get_request()));

        let profile: SignalServiceProfile = self
            .http_request(
                Method::GET,
                format!(
                    "/v1/profile/{}/{}/{}?credentialType=expiringProfileKey",
                    aci.service_id_string(),
                    version,
                    request
                ),
            )?
            .send()
            .await?
            .service_error_for_status()
            .await?
            .json()
            .await?;

        let credential = profile.credential.ok_or(ServiceError::InvalidFrame {
            reason: "profile carried no credential",
        })?;
        let response = zkgroup::deserialize::<
            zkgroup::profiles::ExpiringProfileKeyCredentialResponse,
        >(&credential)
        .map_err(|_| ServiceError::InvalidFrame {
            reason: "undecodable profile key credential",
        })?;

        server_public_params
            .receive_expiring_profile_key_credential(
                &context,
                &response,
                zkgroup::Timestamp::from_epoch_seconds(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("system clock before the epoch")
                        .as_secs(),
                ),
            )
            .map_err(|_| ServiceError::InvalidFrame {
                reason: "profile key credential did not verify",
            })
    }

    /// Writes a profile and returns the avatar URL, if one was provided.
    ///
    /// The name, about and emoji fields are encrypted with an [`ProfileCipher`][struct@crate::profile_cipher::ProfileCipher].
    /// See [`AccountManager`][struct@crate::AccountManager] for a convenience method.
    ///
    /// The avatar is expected already encrypted; see
    /// [`ProfileCipher::encrypt_avatar`][crate::profile_cipher::ProfileCipher::encrypt_avatar].
    ///
    /// UNVERIFIED: when an avatar is written, the server answers with the
    /// signed S3 policy Signal-Server calls `ProfileAvatarUploadAttributes`,
    /// which is deserialised here as [`AttachmentV2UploadAttributes`] — the two
    /// carry the same fields. Not exercised against a live server from this
    /// tree; test against a real account before relying on it.
    ///
    /// Java equivalent: `writeProfile`
    pub async fn write_profile<'s, C, S>(
        &mut self,
        version: &ProfileKeyVersion,
        name: &[u8],
        about: &[u8],
        emoji: &[u8],
        commitment: &ProfileKeyCommitment,
        avatar: AvatarWrite<&mut C>,
    ) -> Result<Option<String>, ServiceError>
    where
        C: std::io::Read + Send + 's,
        S: AsRef<str>,
    {
        // Bincode is transparent and will return a hex-encoded string.
        let version = bincode::serialize(version)?;
        let version = std::str::from_utf8(&version)
            .expect("profile_key_version is hex encoded string");
        let commitment = bincode::serialize(commitment)?;

        let command = SignalServiceProfileWrite {
            version,
            name,
            about,
            about_emoji: emoji,
            avatar: !matches!(avatar, AvatarWrite::NoAvatar),
            same_avatar: matches!(avatar, AvatarWrite::RetainAvatar),
            commitment: &commitment,
        };

        let response = self
            .http_request(Method::PUT, "/v1/profile")?
            .send_json(&command)
            .await?
            .service_error_for_status()
            .await?;

        match avatar {
            AvatarWrite::NewAvatar(avatar) => {
                let attributes: AttachmentV2UploadAttributes =
                    response.json().await?;
                let key = attributes.key().to_owned();

                self.unidentified_push_service
                    .upload_to_cdn0(
                        AVATAR_UPLOAD_PATH,
                        attributes,
                        AVATAR_UPLOAD_FILENAME.into(),
                        avatar,
                    )
                    .await?;

                Ok(Some(key))
            },
            AvatarWrite::RetainAvatar | AvatarWrite::NoAvatar => {
                // OWS sends an empty string when there's no attachment
                Ok(None)
            },
        }
    }
}

impl SignalWebSocket<websocket::Unidentified> {
    pub async fn retrieve_profile_avatar(
        &mut self,
        path: &str,
    ) -> Result<impl futures::io::AsyncRead + Send + Unpin, ServiceError> {
        self.unidentified_push_service.get_from_cdn(0, path).await
    }

    pub async fn retrieve_groups_v2_profile_avatar(
        &mut self,
        path: &str,
    ) -> Result<impl futures::io::AsyncRead + Send + Unpin, ServiceError> {
        self.unidentified_push_service.get_from_cdn(0, path).await
    }
}
